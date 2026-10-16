// SPDX-License-Identifier: GPL-3.0-only
use super::{MemberMenuChoice, conversation_participants, member_menu_items};
use crate::member_selection::{Participants, Roster};
use cayenchat_model::{Conversation, ConversationId, ConversationKind, NetworkId};

trait Rows {
    fn rows(&self) -> Vec<&str>;
}

impl Rows for Participants<'_> {
    fn rows(&self) -> Vec<&str> {
        self.entries().map(String::as_str).collect()
    }
}

fn conversation(kind: ConversationKind, name: &str) -> Conversation {
    Conversation {
        id: ConversationId(1),
        network: NetworkId(1),
        kind,
        name: name.to_owned(),
        topic: String::new(),
        messages: Vec::new(),
        members: Vec::new(),
        modes: None,
    }
}

fn private(peer: &str) -> Conversation {
    let peer_key = peer.to_ascii_lowercase();
    conversation(ConversationKind::Private { peer_key }, peer)
}

#[test]
fn channel_lists_its_members() {
    let mut channel = conversation(ConversationKind::Channel, "#c");
    channel.members = vec!["@op".to_owned(), "bob".to_owned()];
    let rows = conversation_participants(&channel, Some("me"));
    assert_eq!(rows.rows(), ["@op", "bob"]);
}

fn channel_with(members: &[&str]) -> Conversation {
    let mut channel = conversation(ConversationKind::Channel, "#c");
    channel.members = members.iter().map(|m| (*m).to_owned()).collect();
    channel
}

#[test]
fn channel_puts_our_row_first_and_keeps_the_rest_in_order() {
    let channel = channel_with(&["@Alice", "@Bob", "+Dave", "Eve", "Frank", "George"]);
    let rows = conversation_participants(&channel, Some("Frank"));
    assert_eq!(
        rows.rows(),
        ["Frank", "@Alice", "@Bob", "+Dave", "Eve", "George"]
    );
    assert_eq!(rows.len(), 6);
    assert_eq!(rows.get(0).map(String::as_str), Some("Frank"));
    assert_eq!(rows.get(1).map(String::as_str), Some("@Alice"));
    assert_eq!(rows.get(4).map(String::as_str), Some("Eve"));
    assert_eq!(rows.get(5).map(String::as_str), Some("George"));
    assert_eq!(rows.get(6), None);
    assert_eq!(
        channel.members[4], "Frank",
        "the roster itself is untouched"
    );
}

#[test]
fn channel_keeps_our_role_prefix_and_matches_with_rfc1459_case() {
    let channel = channel_with(&["a", "b", "@Nick[x]"]);
    let rows = conversation_participants(&channel, Some("nick{X}"));
    assert_eq!(rows.rows(), ["@Nick[x]", "a", "b"]);
}

#[test]
fn channel_without_us_or_with_us_first_is_unchanged() {
    let channel = channel_with(&["a", "b"]);
    assert_eq!(
        conversation_participants(&channel, Some("me")).rows(),
        ["a", "b"]
    );
    assert_eq!(conversation_participants(&channel, None).rows(), ["a", "b"]);
    assert_eq!(
        conversation_participants(&channel, Some("a")).rows(),
        ["a", "b"]
    );
}

#[test]
fn selection_follows_the_displayed_rows() {
    use crate::member_selection::{Click, MemberSelection};
    let channel = channel_with(&["@Alice", "Bob", "Me", "Carol"]);
    let rows = conversation_participants(&channel, Some("Me"));
    let id = channel.id;
    let mut selection = MemberSelection::default();
    selection.click(id, &rows, 0, Click::Only);
    assert_eq!(selection.nicknames(id, &rows), ["Me"]);
    assert_eq!(selection.anchor_index(id, &rows), Some(0));
    selection.click(id, &rows, 2, Click::Range);
    assert_eq!(selection.nicknames(id, &rows), ["Me", "Alice", "Bob"]);
    selection.click(id, &rows, 3, Click::Only);
    assert_eq!(selection.nicknames(id, &rows), ["Carol"]);
    assert_eq!(selection.anchor_index(id, &rows), Some(3));
}

#[test]
fn private_lists_us_then_the_peer() {
    let conversation = private("Alice");
    assert_eq!(
        conversation_participants(&conversation, Some("me")).rows(),
        ["me", "Alice"]
    );
}

#[test]
fn private_follows_nick_changes() {
    let mut conversation = private("Alice");
    assert_eq!(
        conversation_participants(&conversation, Some("me2")).rows(),
        ["me2", "Alice"]
    );
    conversation.name = "Alice_".to_owned();
    assert_eq!(
        conversation_participants(&conversation, Some("me2")).rows(),
        ["me2", "Alice_"]
    );
}

#[test]
fn private_without_own_nickname_lists_the_peer() {
    let conversation = private("Alice");
    assert_eq!(
        conversation_participants(&conversation, None).rows(),
        ["Alice"]
    );
}

#[test]
fn private_with_same_nickname_ignoring_case_is_one_row() {
    let conversation = private("Me");
    assert_eq!(
        conversation_participants(&conversation, Some("mE")).rows(),
        ["Me"]
    );
}

#[test]
fn the_menu_of_our_own_nickname_opens_with_the_change_to_it() {
    let (items, separator) = member_menu_items(false, true);
    assert_eq!(
        items.iter().map(|(choice, _)| *choice).collect::<Vec<_>>(),
        [
            MemberMenuChoice::Nick,
            MemberMenuChoice::Whois,
            MemberMenuChoice::PrivateMessage,
            MemberMenuChoice::Invite,
            MemberMenuChoice::GiveOp,
            MemberMenuChoice::Deop,
        ]
    );
    assert_eq!(separator, 3);

    // A member who is not us keeps the old menu.
    let (items, separator) = member_menu_items(false, false);
    assert_eq!(
        items.iter().map(|(choice, _)| *choice).collect::<Vec<_>>(),
        [
            MemberMenuChoice::Whois,
            MemberMenuChoice::PrivateMessage,
            MemberMenuChoice::Invite,
            MemberMenuChoice::GiveOp,
            MemberMenuChoice::Deop,
        ]
    );
    assert_eq!(separator, 3);

    // In a private conversation nothing is offered on ourselves, so the
    // change stays away there; the menu is too short for a separator.
    let (items, separator) = member_menu_items(true, true);
    assert_eq!(
        items.iter().map(|(choice, _)| *choice).collect::<Vec<_>>(),
        [MemberMenuChoice::Whois, MemberMenuChoice::PrivateMessage]
    );
    assert!(separator >= items.len());
}
