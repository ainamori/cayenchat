// SPDX-License-Identifier: GPL-3.0-only
use super::{ChatWindow, Selection, Worker};
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

fn rejected(channel: &str, draft: &str) -> Event {
    Event::OutgoingRejected {
        channel: channel.into(),
        draft: draft.into(),
        error: "too long".into(),
    }
}

#[gpui::test]
fn a_refused_draft_goes_back_to_the_input_it_came_from(cx: &mut TestAppContext) {
    let (chat, cx) = window(cx);
    let network = NetworkId(1);
    chat.update(cx, |chat, cx| {
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
        let a = Selection::Channel(chat.state.channel_id(network, "#a").unwrap());
        let b = Selection::Channel(chat.state.channel_id(network, "#b").unwrap());
        let bob = chat
            .state
            .private_conversation(network, "bob", "bob", Missing::Create)
            .unwrap();
        let bob = Selection::Channel(bob);
        chat.ensure_inputs(cx);

        // A plain message to a channel and to a person.
        chat.handle_events(network, vec![rejected("#a", "hello")], Worker::Running, cx);
        assert_eq!(text_of(chat, a, cx), "hello");
        chat.handle_events(
            network,
            vec![rejected("bob", "hi bob")],
            Worker::Running,
            cx,
        );
        assert_eq!(text_of(chat, bob, cx), "hi bob");

        // A command typed in another input goes back there, not to its target.
        chat.recent_sends
            .push_back((network, b, "/msg #a long".into()));
        chat.handle_events(
            network,
            vec![rejected("#a", "/msg #a long")],
            Worker::Running,
            cx,
        );
        assert_eq!(text_of(chat, b, cx), "/msg #a long");
        assert_eq!(text_of(chat, a, cx), "hello");

        // A command whose input is unknown is not guessed.
        chat.handle_events(
            network,
            vec![rejected("#b", "/me long")],
            Worker::Running,
            cx,
        );
        assert_eq!(text_of(chat, b, cx), "/msg #a long");

        // New text in the input is never replaced.
        chat.handle_events(network, vec![rejected("#a", "other")], Worker::Running, cx);
        assert_eq!(text_of(chat, a, cx), "hello");
    });
}
