// SPDX-License-Identifier: GPL-3.0-only
use super::{ChatWindow, ToggleToolbar, Worker, app_menus};
use crate::localization::Localizer;
use cayenchat_irc_core::Event;
use cayenchat_model::NetworkId;
use cayenchat_storage::Language;
use gpui::{Action, MenuItem, TestAppContext};

fn view_menu_label(toolbar_shown: bool) -> String {
    let menus = app_menus(false, toolbar_shown, &Localizer::new(Language::English));
    menus
        .iter()
        .flat_map(|menu| &menu.items)
        .find_map(|item| match item {
            MenuItem::Action { name, action, .. } if action.name() == ToggleToolbar.name() => {
                Some(name.to_string())
            }
            _ => None,
        })
        .expect("the View menu offers the toolbar")
}

#[test]
fn the_view_menu_offers_to_show_or_hide_the_toolbar() {
    assert_eq!(view_menu_label(true), "Hide Toolbar");
    assert_eq!(view_menu_label(false), "Show Toolbar");
}

#[gpui::test]
fn buttons_follow_the_connection_and_the_selection(cx: &mut TestAppContext) {
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
        let state = chat.toolbar_state();
        assert!(!state.can_disconnect && !state.can_join_or_part && !state.can_whois);
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
        let state = chat.toolbar_state();
        assert!(state.can_join_or_part && state.joined);
        // WHOIS needs one chosen member.
        assert!(!state.can_whois);
        // Back on the server, a channel button has nothing to act on.
        let server = chat.state.networks()[0].id;
        chat.state
            .dispatch(cayenchat_app::Command::SelectServer(server));
        assert!(!chat.toolbar_state().can_join_or_part);
    });
}
