// SPDX-License-Identifier: GPL-3.0-only
use super::{ChatWindow, MemberMenu};
use cayenchat_irc_core::Event;
use cayenchat_model::NetworkId;
use gpui::{TestAppContext, point, px};

#[gpui::test]
fn right_click_on_self_closes_the_peer_menu(cx: &mut TestAppContext) {
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
    let network = NetworkId(1);
    chat.update(cx, |chat, cx| {
        chat.handle_events(
            network,
            vec![Event::Registered {
                nickname: "alice".into(),
            }],
            false,
            cx,
        );
        chat.member_menu = Some(MemberMenu {
            position: point(px(0.), px(0.)),
            network,
            nickname: "bob".into(),
            channel: "bob".into(),
            private: true,
            group: Vec::new(),
        });
        // The peer keeps their menu; a channel is never ours to refuse.
        assert!(!chat.right_click_on_self(true, network, "bob"));
        assert!(!chat.right_click_on_self(false, network, "alice"));
        assert!(chat.member_menu.is_some());
        // Our own row closes it.
        assert!(chat.right_click_on_self(true, network, "Alice"));
        assert!(chat.member_menu.is_none());
    });
}
