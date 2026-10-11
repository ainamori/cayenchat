use super::ChatWindow;
use cayenchat_irc_core::Event;
use cayenchat_model::NetworkId;
use gpui::{Modifiers, TestAppContext, point, px};

#[gpui::test]
fn the_pointer_is_a_hand_over_a_url_in_the_channel_log(cx: &mut TestAppContext) {
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
    let message = |text: &str| Event::ChannelMessage {
        channel: "#a".into(),
        sender: "bob".into(),
        text: text.into(),
        notice: false,
        server_time: None,
        msgid: None,
        account: None,
        replayed: false,
    };
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
                message("https://example.org/x"),
                message("hello there"),
            ],
            false,
            cx,
        );
        let channel = chat.state.conversations()[0].id;
        chat.state
            .dispatch(cayenchat_app::Command::SelectChannel(channel));
        cx.notify();
    });
    cx.run_until_parked();
    let over = |row: usize, cx: &mut gpui::VisualTestContext| {
        let selector: &'static str = ["message-text-0", "message-text-1"][row];
        let bounds = cx.debug_bounds(selector).expect("row drawn");
        // Past the "bob: " prefix the nickname shares the text with.
        let at = point(bounds.origin.x + px(60.), bounds.center().y);
        cx.simulate_mouse_move(at, None, Modifiers::none());
        cx.run_until_parked();
        chat.read_with(cx, |chat, _| chat.url_hover.is_some())
    };
    assert!(over(0, cx), "on the link");
    assert!(!over(1, cx), "on plain text");
    assert!(over(0, cx), "back on the link");
}

#[gpui::test]
fn the_combined_log_flows_channel_network_and_nickname_into_the_text(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::secrets::install_memory(cx);
        cx.set_global(crate::theme::Theme::new(
            cayenchat_storage::ThemeMode::Light,
            gpui::WindowAppearance::Light,
            &cayenchat_storage::Appearance::default(),
        ));
    });
    let settings = crate::settings_with_channels("#a,#b");
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
                Event::Joined {
                    channel: "#b".into(),
                },
                Event::ChannelMessage {
                    channel: "#b".into(),
                    sender: "bob".into(),
                    text: "hello".into(),
                    notice: false,
                    server_time: None,
                    msgid: None,
                    account: None,
                    replayed: false,
                },
            ],
            false,
            cx,
        );
        let channel = chat.state.conversations()[0].id;
        chat.state
            .dispatch(cayenchat_app::Command::SelectChannel(channel));
        cx.notify();
    });
    cx.run_until_parked();
    let bounds = cx.debug_bounds("sub-text-0").expect("combined row drawn");
    // No fixed channel name column (162 px by default) before the text:
    // only the time column and its gap precede it.
    assert!(
        bounds.origin.x < px(100.),
        "text starts at {:?}",
        bounds.origin.x
    );
    chat.read_with(cx, |chat, _| assert_eq!(chat.sub_rows.len(), 1));
}
