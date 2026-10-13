// SPDX-License-Identifier: GPL-3.0-only
use super::{ChatWindow, Worker};
use cayenchat_irc_core::{ChannelActivityKind, Event};
use cayenchat_model::NetworkId;
use gpui::TestAppContext;

fn message(channel: &str, sender: &str, text: &str, replayed: bool) -> Event {
    Event::ChannelMessage {
        channel: channel.into(),
        sender: sender.into(),
        text: text.into(),
        notice: false,
        server_time: None,
        msgid: None,
        account: None,
        replayed,
    }
}

fn logged(directory: &std::path::Path, relative: &str) -> Vec<String> {
    let text = std::fs::read_to_string(directory.join(relative)).unwrap_or_default();
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split('\t').skip(1).collect::<Vec<_>>().join("|"))
        .collect()
}

/// The day's file of `conversation` below the first server's directory.
fn day_file(conversation: &str) -> String {
    let day = chrono::Local::now().format("%Y-%m-%d");
    format!("irc.ircnet.com/{conversation}/{day}.log")
}

#[gpui::test]
fn enabled_logs_save_live_lines_only(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::secrets::install_memory(cx);
        cx.set_global(crate::theme::Theme::new(
            cayenchat_storage::ThemeMode::Light,
            gpui::WindowAppearance::Light,
            &cayenchat_storage::Appearance::default(),
        ));
    });
    let directory = tempfile::tempdir().unwrap();
    let mut settings = crate::settings_with_channels("#Rust");
    settings.conversation_log.channels = true;
    settings.conversation_log.private = true;
    settings.conversation_log.directory = Some(directory.path().to_owned());
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
                    channel: "#Rust".into(),
                },
                message("#rust", "alice", "\u{2}hello\u{2}\tworld", false),
                message("#RUST", "bob", "\u{1}ACTION waves\u{1}", false),
                message("#rust", "carol", "replayed", true),
                message("#rust", "carol", "\u{1}VERSION\u{1}", false),
                Event::ChannelActivity {
                    channel: "#rust".into(),
                    actor: "dave".into(),
                    kind: ChannelActivityKind::Kicked {
                        target: "erin".into(),
                        reason: Some("spam".into()),
                        own: false,
                    },
                    server_time: None,
                },
                Event::ChannelActivity {
                    channel: "#rust".into(),
                    actor: "dave".into(),
                    kind: ChannelActivityKind::ModeChanged { modes: "+o".into() },
                    server_time: None,
                },
                Event::PrivateMessage {
                    sender: "Alice".into(),
                    text: "psst".into(),
                    notice: false,
                    server_time: None,
                    msgid: None,
                    account: None,
                    replayed: false,
                },
                Event::OutgoingAccepted {
                    local_id: None,
                    channel: "#rust".into(),
                    text: "from me".into(),
                    notice: false,
                },
                Event::OutgoingAccepted {
                    local_id: Some(7),
                    channel: "#rust".into(),
                    text: "refused".into(),
                    notice: false,
                },
                Event::OutgoingFailed {
                    local_id: 7,
                    reason: "no".into(),
                },
                Event::OutgoingAccepted {
                    local_id: Some(8),
                    channel: "#rust".into(),
                    text: "confirmed".into(),
                    notice: false,
                },
                Event::OutgoingConfirmed {
                    local_id: 8,
                    text: None,
                    msgid: None,
                    server_time: None,
                },
            ],
            Worker::Running,
            cx,
        );
        chat.conversation_logs.stop();
    });
    assert_eq!(
        logged(directory.path(), &day_file("channels/#rust")),
        [
            "msg|alice|hello world",
            "action|bob|waves",
            "kick|dave|erin spam",
            "msg|me|from me",
            "msg|me|confirmed",
        ]
    );
    assert_eq!(
        logged(directory.path(), &day_file("private/alice")),
        ["msg|Alice|psst"]
    );
}

#[gpui::test]
fn logs_are_off_by_default(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::secrets::install_memory(cx);
        cx.set_global(crate::theme::Theme::new(
            cayenchat_storage::ThemeMode::Light,
            gpui::WindowAppearance::Light,
            &cayenchat_storage::Appearance::default(),
        ));
    });
    let settings = crate::settings_with_channels("#a");
    assert!(!settings.conversation_log.enabled());
    let (chat, cx) = cx.add_window_view(|window, cx| {
        ChatWindow::with_settings(settings.clone(), None, window, cx)
    });
    chat.read_with(cx, |chat, _| {
        assert!(chat.conversation_logs.status().is_none());
    });
}
