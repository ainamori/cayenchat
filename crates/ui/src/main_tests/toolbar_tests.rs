// SPDX-License-Identifier: GPL-3.0-only
use super::{ChatWindow, ToggleToolbar, Worker, app_menus};
use crate::localization::Localizer;
use cayenchat_irc_core::Event;
use cayenchat_model::NetworkId;
use cayenchat_storage::Language;
use gpui::{Action, MenuItem, TestAppContext};

fn view_menu_label(toolbar_shown: bool) -> String {
    let menus = app_menus(
        false,
        toolbar_shown,
        false,
        &Localizer::new(Language::English),
    );
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

#[cfg(not(target_os = "macos"))]
#[test]
fn the_view_menu_offers_the_menu_bar_apart_from_the_toolbar() {
    let label = |menu_bar_hidden, toolbar_shown| {
        let menus = app_menus(
            false,
            toolbar_shown,
            menu_bar_hidden,
            &Localizer::new(Language::English),
        );
        menus
            .iter()
            .flat_map(|menu| &menu.items)
            .find_map(|item| match item {
                MenuItem::Action { name, action, .. }
                    if action.name() == super::ToggleMenuBar.name() =>
                {
                    Some(name.to_string())
                }
                _ => None,
            })
            .expect("the View menu offers the menu bar")
    };
    assert_eq!(label(false, true), "Hide Menu Bar");
    assert_eq!(label(true, true), "Show Menu Bar");
    assert_eq!(label(false, false), "Hide Menu Bar");
    assert_eq!(label(true, false), "Show Menu Bar");
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
        assert!(!state.connected && !state.can_join_or_part && !state.can_whois);
        let events =
            |chat: &mut ChatWindow, events: Vec<Event>, cx: &mut gpui::Context<ChatWindow>| {
                chat.handle_events(NetworkId(1), events, Worker::Running, cx);
            };
        events(
            chat,
            vec![
                Event::Registered {
                    nickname: "me".into(),
                },
                Event::Joined {
                    channel: "#a".into(),
                },
            ],
            cx,
        );
        let channel = chat.state.conversations()[0].id;
        chat.state
            .dispatch(cayenchat_app::Command::SelectChannel(channel));
        let state = chat.toolbar_state();
        assert!(state.can_join_or_part && state.joined);
        // WHOIS needs one chosen member.
        assert!(!state.can_whois);

        // The same button follows every way of leaving and of coming back.
        let joined = |chat: &ChatWindow| chat.toolbar_state().joined;
        events(
            chat,
            vec![Event::Parted {
                channel: "#a".into(),
            }],
            cx,
        );
        assert!(!joined(chat), "after PART");
        events(
            chat,
            vec![Event::Joined {
                channel: "#a".into(),
            }],
            cx,
        );
        assert!(joined(chat), "after JOIN completes");
        // A KICK of ours arrives as a part.
        events(
            chat,
            vec![Event::Parted {
                channel: "#a".into(),
            }],
            cx,
        );
        assert!(!joined(chat), "after KICK");
        events(
            chat,
            vec![Event::Joined {
                channel: "#a".into(),
            }],
            cx,
        );
        events(chat, vec![Event::Disconnected("gone".into())], cx);
        let state = chat.toolbar_state();
        assert!(!state.joined, "after the connection ended");
        assert!(!state.can_join_or_part, "nothing to join through");
        // Reconnecting and joining again (auto-join).
        events(
            chat,
            vec![
                Event::Registered {
                    nickname: "me".into(),
                },
                Event::Joined {
                    channel: "#a".into(),
                },
            ],
            cx,
        );
        let state = chat.toolbar_state();
        assert!(state.can_join_or_part && state.joined, "after auto-join");

        // Back on the server, a channel button has nothing to act on.
        let server = chat.state.networks()[0].id;
        chat.state
            .dispatch(cayenchat_app::Command::SelectServer(server));
        assert!(!chat.toolbar_state().can_join_or_part);
        // Selecting the channel again shows its state.
        chat.state
            .dispatch(cayenchat_app::Command::SelectChannel(channel));
        assert!(chat.toolbar_state().joined);
    });
}

#[gpui::test]
fn the_first_button_follows_a_connection_or_a_pending_retry(cx: &mut TestAppContext) {
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
    chat.update(cx, |chat, _| {
        let idle = chat.toolbar_state();
        assert!(!idle.connected && idle.can_connect && !idle.can_reconnect);
        let session = chat.sessions.get_mut(&NetworkId(1)).expect("a session");
        session.retry_pending = true;
        let waiting = chat.toolbar_state();
        assert!(waiting.connected, "a scheduled retry can be cancelled");
        chat.sessions.get_mut(&NetworkId(1)).unwrap().retry_pending = false;
        assert!(!chat.toolbar_state().connected);
    });
}

#[gpui::test]
fn the_upload_button_explains_a_missing_service_before_asking_for_a_file(cx: &mut TestAppContext) {
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
    // The test platform has no file dialog and panics if one is opened.
    chat.update_in(cx, |chat, window, cx| chat.toolbar_upload(window, cx));
    assert!(cx.has_pending_prompt(), "the missing service is announced");
}

#[test]
fn the_file_dialog_offers_the_images_the_upload_accepts() {
    let filter = crate::image_upload::image_file_filter(&Localizer::new(Language::English));
    assert_eq!(filter.name.as_ref(), "Images");
    let extensions: Vec<&str> = filter.extensions.iter().map(|e| e.as_ref()).collect();
    assert_eq!(
        extensions,
        cayenchat_model::attachment::ImageFormat::FILE_EXTENSIONS
    );
}
