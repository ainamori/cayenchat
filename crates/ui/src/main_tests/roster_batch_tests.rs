// SPDX-License-Identifier: GPL-3.0-only
use super::superseded_rosters;
use cayenchat_irc_core::Event;
use cayenchat_model::names::CaseMapping;

fn names(channel: &str) -> Event {
    Event::Names {
        channel: channel.into(),
        users: vec!["alice".into()],
    }
}

#[test]
fn only_the_last_roster_of_a_channel_is_applied() {
    let batch = [names("#a"), names("#b"), names("#a")];
    assert_eq!(superseded_rosters(&batch), [0].into());
}

#[test]
fn rosters_are_kept_across_a_case_mapping_change() {
    let batch = [
        names("#foo{1}"),
        Event::CaseMapping(CaseMapping::Rfc1459),
        names("#foo{1}"),
        Event::TransportConnected,
        names("#foo{1}"),
    ];
    assert!(superseded_rosters(&batch).is_empty());
}
