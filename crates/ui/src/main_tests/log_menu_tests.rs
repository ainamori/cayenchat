// SPDX-License-Identifier: GPL-3.0-only
use super::{ChatWindow, LogMenuChoice, LogPosition, LogSelection, Worker};
use cayenchat_irc_core::Event;
use cayenchat_model::NetworkId;
use gpui::{ClipboardItem, TestAppContext, point, px};

fn open_chat(cx: &mut TestAppContext) -> (gpui::Entity<ChatWindow>, &mut gpui::VisualTestContext) {
    cx.update(|cx| {
        crate::secrets::install_memory(cx);
        cx.set_global(crate::theme::Theme::new(
            cayenchat_storage::ThemeMode::Light,
            gpui::WindowAppearance::Light,
            &cayenchat_storage::Appearance::default(),
        ));
    });
    let settings = crate::settings_with_channels("#a");
    let (chat, cx) = cx.add_window_view(|window, cx| {
        ChatWindow::with_settings(settings.clone(), None, window, cx)
    });
    chat.update(cx, |chat, cx| {
        chat.handle_events(
            NetworkId(1),
            vec![
                Event::Registered {
                    nickname: "me".into(),
                },
                Event::Joined {
                    channel: "#a".into(),
                },
            ],
            Worker::Running,
            cx,
        );
        let channel = chat.state.conversations()[0].id;
        chat.state
            .dispatch(cayenchat_app::Command::SelectChannel(channel));
    });
    (chat, cx)
}

#[gpui::test]
fn the_menu_names_only_a_valid_other_speaker(cx: &mut TestAppContext) {
    let (chat, cx) = open_chat(cx);
    chat.update_in(cx, |chat, window, cx| {
        let at = point(px(10.), px(10.));
        chat.open_log_menu(at, Some("bob"), window, cx);
        assert_eq!(
            chat.log_menu.as_ref().unwrap().sender.as_deref(),
            Some("bob")
        );
        for refused in ["Me", "#a", "a b", ""] {
            chat.open_log_menu(at, Some(refused), window, cx);
            assert!(
                chat.log_menu.as_ref().unwrap().sender.is_none(),
                "{refused}"
            );
        }
        chat.open_log_menu(at, None, window, cx);
        assert!(chat.log_menu.as_ref().unwrap().sender.is_none());
        assert!(chat.dismiss_menus());
        assert!(chat.log_menu.is_none());
    });
}

#[gpui::test]
fn opening_a_private_conversation_selects_it_and_sends_nothing(cx: &mut TestAppContext) {
    let (chat, cx) = open_chat(cx);
    chat.update_in(cx, |chat, window, cx| {
        let at = point(px(10.), px(10.));
        for _ in 0..2 {
            let channel = chat.state.conversations()[0].id;
            chat.state
                .dispatch(cayenchat_app::Command::SelectChannel(channel));
            chat.open_log_menu(at, Some("Bob"), window, cx);
            chat.log_menu_choice(LogMenuChoice::Private, window, cx);
            let selected = chat.state.selected_channel().unwrap();
            assert!(selected.is_private());
            assert_eq!(selected.name, "Bob");
            assert!(selected.messages.is_empty());
        }
        // The second open found the first conversation.
        let privates = chat
            .state
            .conversations()
            .iter()
            .filter(|c| c.is_private())
            .count();
        assert_eq!(privates, 1);
        // A private conversation has no channel actions.
        chat.open_log_menu(at, Some("Bob"), window, cx);
        assert!(chat.log_menu.as_ref().unwrap().private);
    });
}

#[gpui::test]
fn opening_the_menu_keeps_the_selection_and_copy_uses_it(cx: &mut TestAppContext) {
    let (chat, cx) = open_chat(cx);
    chat.update_in(cx, |chat, window, cx| {
        let channel = chat.state.selected_channel().unwrap().id;
        chat.state.append_conversation_message(
            channel,
            "bob",
            "hello world",
            cayenchat_app::Kind::Message,
            Default::default(),
            cayenchat_app::Unread::Leave,
        );
        let selection = LogSelection {
            channel,
            anchor: LogPosition { row: 0, byte: 6 },
            cursor: LogPosition { row: 0, byte: 11 },
        };
        chat.log_selection = Some(selection);
        cx.write_to_clipboard(ClipboardItem::new_string("old".into()));
        chat.open_log_menu(point(px(5.), px(5.)), Some("bob"), window, cx);
        assert!(chat.has_log_selection(channel));
        assert!(chat.log_selection.is_some());
        chat.log_menu_choice(LogMenuChoice::Copy, window, cx);
        assert!(chat.log_selection.is_some());
        assert_eq!(
            cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
            Some("world")
        );
    });
}

#[gpui::test]
fn mode_and_topic_open_prompts_without_sending(cx: &mut TestAppContext) {
    let (chat, cx) = open_chat(cx);
    chat.update_in(cx, |chat, window, cx| {
        chat.state.set_topic(NetworkId(1), "#a", "old topic");
        let at = point(px(10.), px(10.));
        chat.open_log_menu(at, None, window, cx);
        chat.log_menu_choice(LogMenuChoice::Topic, window, cx);
        let prompt = chat.member_prompt.as_ref().unwrap();
        assert_eq!(prompt.nickname, "#a");
        assert_eq!(prompt.input.read(cx).text(), "old topic");
        chat.cancel_member_prompt(window, cx);
        assert!(chat.member_prompt.is_none());
        chat.open_log_menu(at, None, window, cx);
        chat.log_menu_choice(LogMenuChoice::Mode, window, cx);
        assert!(
            chat.member_prompt
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .text()
                .is_empty()
        );
        chat.open_log_menu(at, None, window, cx);
        chat.log_menu_choice(LogMenuChoice::Nick, window, cx);
        assert_eq!(
            chat.member_prompt.as_ref().unwrap().input.read(cx).text(),
            "me"
        );
    });
}
