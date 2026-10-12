// SPDX-License-Identifier: GPL-3.0-only
//! HTML numeric character references (`&#x1F352;`, `&#127826;`) in received
//! text, found for display only. Named references (`&amp;`) and tags are not
//! HTML-rendered; the text itself is never rewritten.

use std::ops::Range;

/// Most digits accepted in one reference; longer ones stay as written.
const MAX_DIGITS: usize = 8;

/// Most references drawn per message, so a hostile line stays cheap.
const MAX_RUNS: usize = 64;

/// The character a reference names, unless showing it would be unsafe or
/// misleading: controls, line and paragraph separators, bidirectional
/// controls (they would reorder the line) and noncharacters.
fn character(value: u32) -> Option<char> {
    let ch = char::from_u32(value)?;
    let unsafe_char = ch.is_control()
        || matches!(ch, '\u{2028}' | '\u{2029}' | '\u{FEFF}')
        || cayenchat_model::display::is_bidi_control(ch)
        || matches!(value, 0xFDD0..=0xFDEF)
        || value & 0xFFFE == 0xFFFE;
    (!unsafe_char).then_some(ch)
}

/// The reference at the start of `text`: its byte length and character.
fn parse(text: &str) -> Option<(usize, char)> {
    let rest = text.strip_prefix("&#")?;
    let (digits, radix) = match rest.strip_prefix(['x', 'X']) {
        Some(hex) => (hex, 16),
        None => (rest, 10),
    };
    let count = digits
        .bytes()
        .take(MAX_DIGITS + 1)
        .take_while(|byte| char::from(*byte).is_digit(radix))
        .count();
    if count == 0 || count > MAX_DIGITS || digits.as_bytes().get(count) != Some(&b';') {
        return None;
    }
    let ch = character(u32::from_str_radix(&digits[..count], radix).ok()?)?;
    Some((text.len() - digits.len() + count + 1, ch))
}

/// The references of `text` as runs of adjacent ones: the original byte range
/// and the characters it names. References inside `links` (original ranges)
/// are left alone so a URL is never altered.
pub(crate) fn find(text: &str, links: &[(Range<usize>, String)]) -> Vec<(Range<usize>, String)> {
    let mut runs: Vec<(Range<usize>, String)> = Vec::new();
    let mut from = 0;
    while let Some(offset) = text[from..].find("&#") {
        let start = from + offset;
        let Some((len, ch)) = parse(&text[start..]) else {
            from = start + 1;
            continue;
        };
        let end = start + len;
        from = end;
        if links
            .iter()
            .any(|(link, _)| link.start < end && start < link.end)
        {
            continue;
        }
        if let Some((range, shown)) = runs.last_mut().filter(|(range, _)| range.end == start) {
            range.end = end;
            shown.push(ch);
        } else if runs.len() >= MAX_RUNS {
            break;
        } else {
            runs.push((start..end, ch.to_string()));
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(Range<usize>, String)> {
        let links = cayenchat_app::links::find(text);
        find(text, &links)
    }

    #[test]
    fn hex_and_decimal_references_are_decoded() {
        assert_eq!(found("&#x1F352;"), [(0..9, "🍒".into())]);
        assert_eq!(found("&#X1f352;"), [(0..9, "🍒".into())]);
        assert_eq!(found("a &#127826; b"), [(2..11, "🍒".into())]);
    }

    #[test]
    fn adjacent_references_form_one_run() {
        let text = "&#x1F352;&#x1F352;&#127826; 大当たり";
        assert_eq!(found(text), [(0..27, "🍒🍒🍒".into())]);
        assert_eq!(
            found("&#65; &#66;"),
            [(0..5, "A".into()), (6..11, "B".into())]
        );
    }

    #[test]
    fn named_references_and_tags_are_left_alone() {
        assert!(found("&amp; &lt;b&gt; <b>x</b> &#;").is_empty());
    }

    #[test]
    fn malformed_references_stay_as_written() {
        for text in [
            "&#x;", "&#xZZ;", "&#12", "&#12 ;", "&#x1F352", "&#-5;", "&# 65;", "&#x 41;",
        ] {
            assert!(found(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn unsafe_values_stay_as_written() {
        for text in [
            "&#0;",
            "&#x7;",
            "&#10;",
            "&#13;",
            "&#x7F;",
            "&#x85;",
            "&#xD800;",
            "&#xDFFF;",
            "&#x110000;",
            "&#x202E;",
            "&#x2066;",
            "&#x2028;",
            "&#xFEFF;",
            "&#xFFFE;",
            "&#xFDD0;",
            "&#x1FFFF;",
        ] {
            assert!(found(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn overlong_references_stay_as_written() {
        assert!(found("&#0000000065;").is_empty());
        assert!(found(&format!("&#{}1;", "9".repeat(5000))).is_empty());
        assert_eq!(found("&#00000065;"), [(0..11, "A".into())]);
    }

    #[test]
    fn a_bad_reference_does_not_hide_the_next() {
        assert_eq!(found("&#&#65;"), [(2..7, "A".into())]);
        assert_eq!(found("&#xD800;&#65;"), [(8..13, "A".into())]);
    }

    #[test]
    fn references_in_urls_are_not_touched() {
        assert!(found("see https://example.com/a?x=&#65;&y=1 ok").is_empty());
        let text = "&#65; https://example.com/&#66; &#67;";
        let runs = found(text);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].1, "A");
        assert_eq!(runs[1].1, "C");
    }

    #[test]
    fn the_number_of_runs_is_bounded() {
        let text = "&#65; ".repeat(1000);
        assert_eq!(found(&text).len(), MAX_RUNS);
    }
}
