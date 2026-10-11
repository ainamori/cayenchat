// SPDX-License-Identifier: GPL-3.0-only
use super::{conversation_participants, member_menu_choice_count};
use cayenchat_model::{Conversation, ConversationId, ConversationKind, NetworkId};

fn conversation(kind: ConversationKind, name: &str) -> Conversation {
    Conversation {
        id: ConversationId(1),
        network: NetworkId(1),
        kind,
        name: name.to_owned(),
        topic: String::new(),
        messages: Vec::new(),
        members: Vec::new(),
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
    assert!(matches!(rows, std::borrow::Cow::Borrowed(_)));
    assert_eq!(rows.as_ref(), ["@op", "bob"]);
}

#[test]
fn private_lists_us_then_the_peer() {
    let conversation = private("Alice");
    assert_eq!(
        conversation_participants(&conversation, Some("me")).as_ref(),
        ["me", "Alice"]
    );
}

#[test]
fn private_follows_nick_changes() {
    let mut conversation = private("Alice");
    assert_eq!(
        conversation_participants(&conversation, Some("me2")).as_ref(),
        ["me2", "Alice"]
    );
    conversation.name = "Alice_".to_owned();
    assert_eq!(
        conversation_participants(&conversation, Some("me2")).as_ref(),
        ["me2", "Alice_"]
    );
}

#[test]
fn private_without_own_nickname_lists_the_peer() {
    let conversation = private("Alice");
    assert_eq!(
        conversation_participants(&conversation, None).as_ref(),
        ["Alice"]
    );
}

#[test]
fn private_with_same_nickname_ignoring_case_is_one_row() {
    let conversation = private("Me");
    assert_eq!(
        conversation_participants(&conversation, Some("mE")).as_ref(),
        ["Me"]
    );
}

#[test]
fn private_menu_has_only_whois_and_private_message() {
    assert_eq!(member_menu_choice_count(true, 5), 2);
    assert_eq!(member_menu_choice_count(false, 5), 5);
}
