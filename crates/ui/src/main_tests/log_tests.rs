use super::{LogPosition, LogSelection, channel_activity_text};
use cayenchat_irc_core::ChannelActivityKind;
use cayenchat_model::ConversationId;

#[test]
fn channel_activity_uses_english_phrases_with_optional_details() {
    assert_eq!(
        channel_activity_text(
            "kaeru",
            ChannelActivityKind::Joined {
                mask: Some("~kaeru@host".into()),
            },
        ),
        "kaeru has joined (~kaeru@host)"
    );
    assert_eq!(
        channel_activity_text("kaeru", ChannelActivityKind::Left { reason: None }),
        "kaeru has left"
    );
    assert_eq!(
        channel_activity_text(
            "kaeru",
            ChannelActivityKind::Quit {
                reason: Some("bye".into()),
            },
        ),
        "kaeru has quit (bye)"
    );
    assert_eq!(
        channel_activity_text(
            "kaeru",
            ChannelActivityKind::NickChanged { to: "tepeu".into() },
        ),
        "kaeru is now known as tepeu"
    );
    assert_eq!(
        channel_activity_text(
            "tepeu",
            ChannelActivityKind::ModeChanged {
                modes: "+o kaeru".into(),
            },
        ),
        "tepeu has changed mode: +o kaeru"
    );
}

#[test]
fn log_selection_spans_partial_messages() {
    let selection = LogSelection {
        channel: ConversationId(1),
        anchor: LogPosition { row: 2, byte: 3 },
        cursor: LogPosition { row: 0, byte: 2 },
    };
    assert_eq!(selection.range(0, 5), Some(2..5));
    assert_eq!(selection.range(1, 5), Some(0..5));
    assert_eq!(selection.range(2, 5), Some(0..3));
    assert_eq!(selection.range(3, 5), None);
}
