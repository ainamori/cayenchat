// SPDX-License-Identifier: GPL-3.0-only
use super::{ChatWindow, PENDING_DRAFTS, Selection, Worker};
use cayenchat_app::Missing;
use cayenchat_irc_core::Event;
use cayenchat_model::NetworkId;
use gpui::{Entity, TestAppContext};

fn window(cx: &mut TestAppContext) -> (Entity<ChatWindow>, &mut gpui::VisualTestContext) {
    cx.update(|cx| {
        crate::secrets::install_memory(cx);
        cx.set_global(crate::theme::Theme::new(
            cayenchat_storage::ThemeMode::Light,
            gpui::WindowAppearance::Light,
            &cayenchat_storage::Appearance::default(),
        ));
    });
    let settings = crate::settings_with_channels("#a,#b");
    cx.add_window_view(|window, cx| ChatWindow::with_settings(settings.clone(), None, window, cx))
}

fn text_of(chat: &ChatWindow, selection: Selection, cx: &gpui::App) -> String {
    chat.inputs[&selection].read(cx).text().to_owned()
}

fn rejected(id: u64, channel: &str, draft: &str) -> Event {
    Event::OutgoingRejected {
        draft_id: id,
        channel: channel.into(),
        draft: draft.into(),
        error: "too long".into(),
    }
}

fn accepted(id: u64, channel: &str) -> Event {
    Event::OutgoingAccepted {
        local_id: None,
        draft_id: Some(id),
        channel: channel.into(),
        text: "x".into(),
        notice: false,
    }
}

fn join(chat: &mut ChatWindow, network: NetworkId, cx: &mut gpui::Context<ChatWindow>) {
    chat.handle_events(
        network,
        vec![
            Event::Registered {
                nickname: "alice".into(),
            },
            Event::Joined {
                channel: "#a".into(),
            },
            Event::Joined {
                channel: "#b".into(),
            },
        ],
        Worker::Running,
        cx,
    );
}

#[gpui::test]
fn a_refused_draft_goes_back_to_the_input_it_came_from(cx: &mut TestAppContext) {
    let (chat, cx) = window(cx);
    let network = NetworkId(1);
    chat.update(cx, |chat, cx| {
        join(chat, network, cx);
        let a = Selection::Channel(chat.state.channel_id(network, "#a").unwrap());
        let b = Selection::Channel(chat.state.channel_id(network, "#b").unwrap());
        let bob = chat
            .state
            .private_conversation(network, "bob", "bob", Missing::Create)
            .unwrap();
        let bob = Selection::Channel(bob);
        chat.ensure_inputs(cx);

        chat.pending_drafts
            .insert((network, 1), (a, "hello".into()));
        chat.pending_drafts
            .insert((network, 2), (bob, "hi bob".into()));
        // A command typed in another input goes back there, not to its target.
        chat.pending_drafts
            .insert((network, 3), (b, "/msg #a long".into()));
        chat.handle_events(
            network,
            vec![rejected(1, "#a", "hello")],
            Worker::Running,
            cx,
        );
        assert_eq!(text_of(chat, a, cx), "hello");
        chat.handle_events(
            network,
            vec![rejected(2, "bob", "hi bob")],
            Worker::Running,
            cx,
        );
        assert_eq!(text_of(chat, bob, cx), "hi bob");
        chat.handle_events(
            network,
            vec![rejected(3, "#a", "/msg #a long")],
            Worker::Running,
            cx,
        );
        assert_eq!(text_of(chat, b, cx), "/msg #a long");
        assert_eq!(text_of(chat, a, cx), "hello");
        assert!(chat.pending_drafts.is_empty());

        // A draft that was never kept (credentials) is not guessed at.
        chat.handle_events(
            network,
            vec![rejected(9, "#b", "/me long")],
            Worker::Running,
            cx,
        );
        assert_eq!(text_of(chat, b, cx), "/msg #a long");
    });
}

#[gpui::test]
fn the_same_text_from_two_inputs_is_told_apart_by_its_id(cx: &mut TestAppContext) {
    let (chat, cx) = window(cx);
    let network = NetworkId(1);
    chat.update(cx, |chat, cx| {
        join(chat, network, cx);
        let a = Selection::Channel(chat.state.channel_id(network, "#a").unwrap());
        let b = Selection::Channel(chat.state.channel_id(network, "#b").unwrap());
        chat.ensure_inputs(cx);

        chat.pending_drafts.insert((network, 1), (a, "same".into()));
        chat.pending_drafts.insert((network, 2), (b, "same".into()));
        // The later one went out; the earlier one was refused.
        chat.handle_events(
            network,
            vec![accepted(2, "#b"), rejected(1, "#a", "same")],
            Worker::Running,
            cx,
        );
        assert_eq!(text_of(chat, a, cx), "same");
        assert_eq!(text_of(chat, b, cx), "");
        assert!(chat.pending_drafts.is_empty());
    });
}

#[gpui::test]
fn a_draft_waits_for_its_outcome_however_many_others_follow(cx: &mut TestAppContext) {
    let (chat, cx) = window(cx);
    let network = NetworkId(1);
    chat.update(cx, |chat, cx| {
        join(chat, network, cx);
        let a = Selection::Channel(chat.state.channel_id(network, "#a").unwrap());
        chat.ensure_inputs(cx);

        for id in 1..=(PENDING_DRAFTS as u64 + 5) {
            chat.pending_drafts
                .insert((network, id), (a, format!("m{id}")));
        }
        chat.handle_events(network, vec![rejected(1, "#a", "m1")], Worker::Running, cx);
        assert_eq!(text_of(chat, a, cx), "m1");
        // Outcomes release their entries; the rest stay.
        chat.handle_events(network, vec![accepted(2, "#a")], Worker::Running, cx);
        assert!(!chat.pending_drafts.contains_key(&(network, 2)));
        assert!(chat.pending_drafts.contains_key(&(network, 3)));
    });
}

#[gpui::test]
fn a_refused_draft_never_replaces_new_input(cx: &mut TestAppContext) {
    let (chat, cx) = window(cx);
    let network = NetworkId(1);
    chat.update(cx, |chat, cx| {
        join(chat, network, cx);
        let a = Selection::Channel(chat.state.channel_id(network, "#a").unwrap());
        chat.ensure_inputs(cx);
        chat.inputs[&a].update(cx, |input, cx| input.set_text("typing", cx));
        chat.pending_drafts.insert((network, 1), (a, "old".into()));
        chat.handle_events(network, vec![rejected(1, "#a", "old")], Worker::Running, cx);
        assert_eq!(text_of(chat, a, cx), "typing");
    });
}

#[gpui::test]
fn disconnecting_drops_the_ids_of_that_connection(cx: &mut TestAppContext) {
    let (chat, cx) = window(cx);
    let network = NetworkId(1);
    chat.update(cx, |chat, cx| {
        join(chat, network, cx);
        let a = Selection::Channel(chat.state.channel_id(network, "#a").unwrap());
        chat.pending_drafts.insert((network, 1), (a, "m".into()));
        chat.pending_drafts
            .insert((NetworkId(2), 1), (a, "other".into()));
        chat.handle_events(
            network,
            vec![Event::Disconnected("gone".into())],
            Worker::Running,
            cx,
        );
        assert!(!chat.pending_drafts.contains_key(&(network, 1)));
        assert!(chat.pending_drafts.contains_key(&(NetworkId(2), 1)));
    });
}
