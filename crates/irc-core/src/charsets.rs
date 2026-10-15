// SPDX-License-Identifier: MPL-2.0
//! Character encodings of single channels.
//!
//! A connection reads and writes lines in the server's encoding. A channel
//! may override that for the text of its messages, in either direction; the
//! line codec applies it ([`irc::proto::charsets`]), while everything else on
//! the line, the channel name above all, keeps the server's encoding.

use std::sync::{
    RwLock,
    atomic::{AtomicBool, Ordering},
};

use cayenchat_model::names::CaseMapping;
use encoding_rs::Encoding;
use irc::proto::charsets::BodyCharsets;

use crate::valid_channel;

/// The encoding overrides of one channel, as WHATWG labels. A side that is
/// `None` follows the connection's encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelEncoding {
    pub channel: String,
    /// For the text of messages received in the channel.
    pub decode: Option<String>,
    /// For the text of messages sent to the channel.
    pub encode: Option<String>,
}

/// An encoding `encoding_rs` can both read and write, by its label.
pub(crate) fn resolve(label: &str) -> Result<&'static Encoding, String> {
    Encoding::for_label(label.as_bytes())
        .filter(|encoding| encoding.output_encoding() == *encoding)
        .ok_or_else(|| format!("Unsupported character encoding: {label}"))
}

struct Entry {
    channel: String,
    decode: Option<&'static Encoding>,
    encode: Option<&'static Encoding>,
}

struct State {
    mapping: CaseMapping,
    entries: Vec<Entry>,
    /// The server announced `UTF8ONLY`: only UTF-8 overrides apply.
    utf8_only: bool,
}

impl State {
    fn find(&self, channel: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| self.mapping.same(&entry.channel, channel))
    }

    /// `encoding` unless the server allows UTF-8 only.
    fn allowed(&self, encoding: Option<&'static Encoding>) -> Option<&'static Encoding> {
        encoding.filter(|encoding| !self.utf8_only || *encoding == encoding_rs::UTF_8)
    }
}

/// The overrides of one connection. The line codec reads them on its own
/// thread for every message, while the connection handle changes them.
pub(crate) struct ChannelCharsets {
    state: RwLock<State>,
    /// Whether any override exists, so a connection without one does not
    /// take the lock for each line.
    any: AtomicBool,
}

impl ChannelCharsets {
    pub(crate) fn new(entries: &[ChannelEncoding]) -> Result<Self, String> {
        let this = Self {
            state: RwLock::new(State {
                mapping: CaseMapping::default(),
                entries: Vec::new(),
                utf8_only: false,
            }),
            any: AtomicBool::new(false),
        };
        for entry in entries {
            this.set(
                &entry.channel,
                entry.decode.as_deref(),
                entry.encode.as_deref(),
            )?;
        }
        Ok(this)
    }

    /// Replaces the overrides of `channel`; labels that are `None` follow the
    /// connection, and none at all removes the channel's entry.
    pub(crate) fn set(
        &self,
        channel: &str,
        decode: Option<&str>,
        encode: Option<&str>,
    ) -> Result<(), String> {
        if !valid_channel(channel) {
            return Err(format!("Invalid channel name: {channel}"));
        }
        let decode = decode.map(resolve).transpose()?;
        let encode = encode.map(resolve).transpose()?;
        let Ok(mut state) = self.state.write() else {
            return Err("Character encodings are unavailable.".into());
        };
        if state.utf8_only
            && [decode, encode]
                .into_iter()
                .flatten()
                .any(|encoding| encoding != encoding_rs::UTF_8)
        {
            return Err(
                "The server requires UTF-8 (UTF8ONLY); only UTF-8 can be set for a channel.".into(),
            );
        }
        let mapping = state.mapping;
        state
            .entries
            .retain(|entry| !mapping.same(&entry.channel, channel));
        if decode.is_some() || encode.is_some() {
            state.entries.push(Entry {
                channel: channel.to_owned(),
                decode,
                encode,
            });
        }
        self.any.store(!state.entries.is_empty(), Ordering::Relaxed);
        Ok(())
    }

    /// The network compares channel names with this mapping from now on.
    pub(crate) fn set_casemapping(&self, mapping: CaseMapping) {
        if let Ok(mut state) = self.state.write() {
            state.mapping = mapping;
        }
    }

    /// The server announced `UTF8ONLY`. Returns whether that switches off an
    /// override that is not UTF-8.
    pub(crate) fn set_utf8_only(&self) -> bool {
        let Ok(mut state) = self.state.write() else {
            return false;
        };
        let was = state.utf8_only;
        state.utf8_only = true;
        !was && state.entries.iter().any(|entry| {
            [entry.decode, entry.encode]
                .into_iter()
                .flatten()
                .any(|encoding| encoding != encoding_rs::UTF_8)
        })
    }

    pub(crate) fn utf8_only(&self) -> bool {
        self.state.read().is_ok_and(|state| state.utf8_only)
    }
}

impl BodyCharsets for ChannelCharsets {
    fn is_empty(&self) -> bool {
        !self.any.load(Ordering::Relaxed)
    }

    fn decoder(&self, target: &str) -> Option<&'static Encoding> {
        let state = self.state.read().ok()?;
        state.allowed(state.find(target)?.decode)
    }

    fn encoder(&self, target: &str) -> Option<&'static Encoding> {
        let state = self.state.read().ok()?;
        state.allowed(state.find(target)?.encode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(channel: &str, decode: Option<&str>, encode: Option<&str>) -> ChannelCharsets {
        ChannelCharsets::new(&[ChannelEncoding {
            channel: channel.into(),
            decode: decode.map(str::to_owned),
            encode: encode.map(str::to_owned),
        }])
        .unwrap()
    }

    #[test]
    fn each_side_is_looked_up_on_its_own_under_the_case_mapping() {
        let charsets = one("#Jp[x]", Some("ISO-2022-JP"), None);
        assert!(!charsets.is_empty());
        assert_eq!(charsets.decoder("#jp{x}"), Some(encoding_rs::ISO_2022_JP));
        assert_eq!(charsets.encoder("#jp{x}"), None);
        assert_eq!(charsets.decoder("#other"), None);
        // `~` and `^` are the same only under RFC 1459 itself.
        let charsets = one("#a~", Some("Big5"), None);
        assert!(charsets.decoder("#a^").is_some());
        charsets.set_casemapping(CaseMapping::Ascii);
        assert!(charsets.decoder("#a^").is_none());
    }

    #[test]
    fn setting_replaces_and_clearing_removes() {
        let charsets = one("#a", Some("Big5"), Some("Big5"));
        charsets.set("#A", None, Some("GBK")).unwrap();
        assert_eq!(charsets.decoder("#a"), None);
        assert_eq!(charsets.encoder("#a"), Some(encoding_rs::GBK));
        charsets.set("#a", None, None).unwrap();
        assert!(charsets.is_empty());
        assert!(charsets.set("#a", Some("nope"), None).is_err());
        assert!(charsets.set("nick", Some("Big5"), None).is_err());
        assert!(charsets.is_empty());
    }

    #[test]
    fn utf8_only_ignores_other_encodings_and_refuses_new_ones() {
        let charsets = one("#a", Some("Big5"), Some("UTF-8"));
        assert!(charsets.set_utf8_only());
        // Announced again, or nothing to switch off: nothing to report.
        assert!(!charsets.set_utf8_only());
        assert_eq!(charsets.decoder("#a"), None);
        assert_eq!(charsets.encoder("#a"), Some(encoding_rs::UTF_8));
        assert!(charsets.set("#b", Some("GBK"), None).is_err());
        assert!(charsets.set("#b", Some("UTF-8"), None).is_ok());
        assert!(charsets.utf8_only());
    }
}
