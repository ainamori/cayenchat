// SPDX-License-Identifier: MPL-2.0
//! The modes set on each joined channel, for display. Only the mode letters
//! are kept: arguments (a key, a limit) are never read, so they cannot reach
//! the UI whatever the server's `CHANMODES` says.

use std::collections::{HashMap, HashSet};

use irc::proto::mode::Mode;
use irc::proto::{Command, Message, Response};

use crate::{CaseMapping, valid_channel};

/// Most letters kept for one channel; a server cannot grow the state.
const MAX_FLAGS: usize = 64;
/// List modes (type A) when the server announces no `CHANMODES`, and the
/// membership modes when it announces no `PREFIX`.
const DEFAULT_LIST_MODES: &str = "beI";
const DEFAULT_PREFIX_MODES: &str = "ov";

pub(crate) struct ChannelModes {
    list_modes: String,
    prefix_modes: String,
    /// The letters that are not channel flags: they take a list entry or a
    /// member as argument.
    not_flags: HashSet<char>,
    /// The channels we are in, by folded name; `None` until the `324`.
    channels: HashMap<String, Option<Vec<char>>>,
}

impl Default for ChannelModes {
    fn default() -> Self {
        Self {
            list_modes: DEFAULT_LIST_MODES.into(),
            prefix_modes: DEFAULT_PREFIX_MODES.into(),
            not_flags: DEFAULT_LIST_MODES
                .chars()
                .chain(DEFAULT_PREFIX_MODES.chars())
                .collect(),
            channels: HashMap::new(),
        }
    }
}

impl ChannelModes {
    /// Learns `CHANMODES` and `PREFIX` from an `RPL_ISUPPORT` line.
    pub(crate) fn isupport(&mut self, message: &Message) {
        let Command::Response(Response::RPL_ISUPPORT, args) = &message.command else {
            return;
        };
        let mut changed = false;
        for token in args.iter().skip(1).flat_map(|arg| arg.split_whitespace()) {
            if token.eq_ignore_ascii_case("-CHANMODES") {
                self.list_modes = DEFAULT_LIST_MODES.into();
                changed = true;
            } else if token.eq_ignore_ascii_case("-PREFIX") {
                self.prefix_modes = DEFAULT_PREFIX_MODES.into();
                changed = true;
            } else if let Some((name, value)) = token.split_once('=') {
                if name.eq_ignore_ascii_case("CHANMODES") {
                    self.list_modes = value.split(',').next().unwrap_or("").to_owned();
                    changed = true;
                } else if name.eq_ignore_ascii_case("PREFIX") {
                    // `(ov)@+`
                    self.prefix_modes = value
                        .strip_prefix('(')
                        .and_then(|rest| rest.split_once(')'))
                        .map(|(letters, _)| letters.to_owned())
                        .unwrap_or_default();
                    changed = true;
                }
            }
        }
        if changed {
            self.not_flags = self
                .list_modes
                .chars()
                .chain(self.prefix_modes.chars())
                .collect();
        }
    }

    /// The channel's flags after `message`, when it is a `324` or a `MODE`
    /// of a channel we are in. A `MODE` before the `324` is not applied:
    /// its changes have no baseline.
    pub(crate) fn observe(
        &mut self,
        message: &Message,
        mapping: CaseMapping,
        replayed: bool,
    ) -> Option<(String, String)> {
        if replayed {
            return None;
        }
        match &message.command {
            Command::Response(Response::RPL_CHANNELMODEIS, args) => {
                let channel = args.get(1).filter(|name| valid_channel(name))?;
                let key = mapping.fold(channel);
                if !self.channels.contains_key(&key) {
                    return None;
                }
                let mut flags = Vec::new();
                let letters = args.get(2).map(String::as_str).unwrap_or("");
                self.apply(&mut flags, true, letters.chars());
                let shown = flags.iter().collect();
                self.channels.insert(key, Some(flags));
                Some((channel.clone(), shown))
            }
            Command::ChannelMODE(channel, modes) => {
                let key = mapping.fold(channel);
                let mut updated = self.channels.get_mut(&key)?.take()?;
                for mode in modes {
                    let (plus, mode) = match mode {
                        Mode::Plus(mode, _) => (true, mode),
                        Mode::Minus(mode, _) => (false, mode),
                        Mode::NoPrefix(_) => continue,
                    };
                    if let Some(letter) = mode.to_string().chars().next() {
                        self.set(&mut updated, plus, letter);
                    }
                }
                let shown = updated.iter().collect();
                self.channels.insert(key, Some(updated));
                Some((channel.clone(), shown))
            }
            _ => None,
        }
    }

    /// Our own join starts from unknown modes.
    pub(crate) fn joined(&mut self, channel: &str, mapping: CaseMapping) {
        self.channels.insert(mapping.fold(channel), None);
    }

    /// A part or kick ends tracking.
    pub(crate) fn left(&mut self, channel: &str, mapping: CaseMapping) {
        self.channels.remove(&mapping.fold(channel));
    }

    fn apply(&self, flags: &mut Vec<char>, plus: bool, letters: impl Iterator<Item = char>) {
        let mut plus = plus;
        for letter in letters {
            match letter {
                '+' => plus = true,
                '-' => plus = false,
                letter => self.set(flags, plus, letter),
            }
        }
    }

    fn set(&self, flags: &mut Vec<char>, plus: bool, letter: char) {
        if !letter.is_ascii_alphabetic() || self.not_flags.contains(&letter) {
            return;
        }
        if plus {
            if !flags.contains(&letter) && flags.len() < MAX_FLAGS {
                flags.push(letter);
            }
        } else {
            flags.retain(|known| *known != letter);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Message {
        line.parse().unwrap()
    }

    fn feed(modes: &mut ChannelModes, line: &str) -> Option<(String, String)> {
        modes.observe(&parse(line), CaseMapping::default(), false)
    }

    /// Tracking `#x`, as after our own join.
    fn in_x() -> ChannelModes {
        let mut modes = ChannelModes::default();
        modes.joined("#x", CaseMapping::default());
        modes
    }

    #[test]
    fn reply_sets_flags_and_changes_follow() {
        let mut modes = in_x();
        assert_eq!(
            feed(&mut modes, ":s 324 me #x +snt"),
            Some(("#x".into(), "snt".into()))
        );
        assert_eq!(
            feed(&mut modes, ":a!u@h MODE #x +m-s"),
            Some(("#x".into(), "ntm".into()))
        );
        // Repeats are not shown twice; membership and list modes are no flags.
        assert_eq!(
            feed(&mut modes, ":a!u@h MODE #x +mo-b+v a b c"),
            Some(("#x".into(), "ntm".into()))
        );
    }

    #[test]
    fn arguments_never_become_flags() {
        let mut modes = in_x();
        let shown = feed(&mut modes, ":s 324 me #x +ntkl secret 50").unwrap().1;
        assert_eq!(shown, "ntkl");
        let shown = feed(&mut modes, ":a!u@h MODE #x +Zk unknownarg secret")
            .unwrap()
            .1;
        assert!(!shown.contains("secret") && !shown.contains("unknownarg"));
    }

    #[test]
    fn changes_before_the_reply_and_other_channels_are_ignored() {
        let mut modes = in_x();
        assert_eq!(feed(&mut modes, ":a!u@h MODE #x +m"), None);
        assert_eq!(feed(&mut modes, ":s 324 me #other +n"), None);
        assert_eq!(feed(&mut modes, ":a!u@h MODE #other +m"), None);
    }

    #[test]
    fn replay_leaves_no_state_and_leaving_or_rejoining_forgets() {
        let mut modes = in_x();
        let replayed = modes.observe(&parse(":s 324 me #x +n"), CaseMapping::default(), true);
        assert_eq!(replayed, None);
        feed(&mut modes, ":s 324 me #x +n");
        modes.joined("#X", CaseMapping::default());
        assert_eq!(feed(&mut modes, ":a!u@h MODE #x +m"), None);
        feed(&mut modes, ":s 324 me #x +n");
        modes.left("#X", CaseMapping::default());
        assert_eq!(feed(&mut modes, ":a!u@h MODE #x +m"), None);
        assert_eq!(feed(&mut modes, ":s 324 me #x +n"), None);
    }

    #[test]
    fn isupport_overrides_the_grammar() {
        let mut modes = in_x();
        modes.isupport(&parse(
            ":s 005 me CHANMODES=beI,k,l,imnpst PREFIX=(qaohv)~&@%+ :are supported",
        ));
        let shown = feed(&mut modes, ":s 324 me #x +nqa").unwrap().1;
        assert_eq!(shown, "n");
    }

    #[test]
    fn an_empty_reply_is_known_and_empty() {
        let mut modes = in_x();
        assert_eq!(
            feed(&mut modes, ":s 324 me #x +"),
            Some(("#x".into(), String::new()))
        );
    }
}
