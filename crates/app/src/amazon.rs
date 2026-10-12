// SPDX-License-Identifier: MPL-2.0
//! Shortening of long Amazon product links in an outgoing message (#315).
//!
//! Only a link whose meaning can be kept is rewritten, to
//! `https://<same host>/dp/<ASIN>`; everything else, including the text around
//! a link, is left exactly as written. No network access is involved.

use crate::links;

/// Links shorter than this many characters are left alone.
pub const MIN_LENGTH: usize = 100;

/// Marketplace hosts, without a leading `www.`. A host is matched whole, so
/// `amazon.co.jp.example` and `notamazon.com` never qualify.
const DOMAINS: &[&str] = &[
    "amazon.co.jp",
    "amazon.com",
    "amazon.co.uk",
    "amazon.de",
    "amazon.fr",
    "amazon.it",
    "amazon.es",
    "amazon.ca",
    "amazon.com.au",
    "amazon.com.br",
    "amazon.com.mx",
    "amazon.in",
    "amazon.nl",
    "amazon.se",
    "amazon.pl",
    "amazon.sg",
    "amazon.ae",
    "amazon.sa",
    "amazon.com.tr",
    "amazon.eg",
    "amazon.com.be",
];

/// Query parameters that only track where the link came from or what was
/// searched for. Any other parameter (`th`, `psc`, `smid`, `tag`, ...) may
/// select a variation, a seller or an affiliate, so its link is kept.
const TRACKING: &[&str] = &[
    "keywords",
    "qid",
    "sr",
    "crid",
    "sprefix",
    "dib",
    "dib_tag",
    "ref",
    "ref_",
    "content-id",
    "pd_rd_i",
    "pd_rd_r",
    "pd_rd_w",
    "pd_rd_wg",
    "pf_rd_i",
    "pf_rd_m",
    "pf_rd_p",
    "pf_rd_r",
    "pf_rd_s",
    "pf_rd_t",
    "linkcode",
    "linkid",
    "psc_unused",
];

/// `text` with each long Amazon product link shortened. A message holding a
/// CTCP delimiter (U+0001) is protocol text and is returned as written.
pub fn shorten(text: &str) -> String {
    if text.contains('\u{1}') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for (range, _) in links::find(text) {
        if !starts_cleanly(&text[..range.start]) || !ends_cleanly(&text[range.end..]) {
            continue;
        }
        if let Some(short) = shorten_link(&text[range.clone()]) {
            out.push_str(&text[cursor..range.start]);
            out.push_str(&short);
            cursor = range.end;
        }
    }
    out.push_str(&text[cursor..]);
    out
}

/// Whether the link found just before `after` really ends there. The link
/// finder also stops at quotes, brackets and CJK punctuation, which may sit
/// inside the URL, so the rest of the whitespace-delimited word must be only
/// closing punctuation. Anything else could be a parameter that was never
/// checked, and the whole link is left as written.
fn ends_cleanly(after: &str) -> bool {
    after
        .split(char::is_whitespace)
        .next()
        .is_some_and(|rest| rest.chars().all(|ch| ".,;:!?)}」』。、".contains(ch)))
}

/// Whether the link found just after `before` really starts its
/// whitespace-delimited word, optionally behind one opening bracket, and is
/// not inside another URL or a value such as `?q=https://...`.
fn starts_cleanly(before: &str) -> bool {
    let word = before.rsplit(char::is_whitespace).next().unwrap_or("");
    matches!(word, "" | "(" | "「" | "『")
}

fn shorten_link(link: &str) -> Option<String> {
    if link.chars().count() < MIN_LENGTH {
        return None;
    }
    let url = url::Url::parse(link).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let host = url.host_str()?;
    let bare = host.strip_prefix("www.").unwrap_or(host);
    if !DOMAINS.contains(&bare) {
        return None;
    }
    let tracking_only = url
        .query_pairs()
        .all(|(key, _)| TRACKING.contains(&key.to_ascii_lowercase().as_str()));
    if !tracking_only {
        return None;
    }
    let segments: Vec<&str> = url.path_segments()?.collect();
    let (slug_ok, asin, rest) = match segments.as_slice() {
        ["dp", asin, rest @ ..] => (true, asin, rest),
        [slug, "dp", asin, rest @ ..] => (is_slug(slug), asin, rest),
        ["gp", "product", asin, rest @ ..] => (true, asin, rest),
        _ => return None,
    };
    let rest_ok = match rest {
        [] | [""] => true,
        [tail] => tail.starts_with("ref="),
        _ => false,
    };
    (slug_ok && rest_ok && is_asin(asin)).then(|| format!("https://{host}/dp/{asin}"))
}

/// A product-name segment before `/dp/`: anything but another path keyword.
fn is_slug(segment: &str) -> bool {
    !segment.is_empty() && !matches!(segment, "dp" | "gp" | "product")
}

fn is_asin(value: &str) -> bool {
    value.len() == 10
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASIN: &str = "B012345678";

    fn long(host: &str, path: &str) -> String {
        format!(
            "https://{host}{path}?keywords=example&qid=1234567890123&sr=8-1&crid=ABCDEFGHIJKLM&sprefix=abc"
        )
    }

    #[test]
    fn shortens_long_product_links_keeping_the_host() {
        for (host, path) in [
            ("www.amazon.co.jp", "/SomeProduct/dp/B012345678/ref=sr_1_1"),
            ("www.amazon.com", "/dp/B012345678"),
            ("amazon.de", "/gp/product/B012345678/"),
            ("www.amazon.co.uk", "/%E5%95%86%E5%93%81/dp/B012345678"),
        ] {
            let url = long(host, path);
            assert!(url.chars().count() >= MIN_LENGTH, "{url}");
            assert_eq!(shorten(&url), format!("https://{host}/dp/{ASIN}"), "{url}");
        }
    }

    #[test]
    fn keeps_surrounding_text_and_other_links() {
        let url = long("www.amazon.co.jp", "/x/dp/B012345678");
        let text = format!("これ {url}。 https://example.com/a も ({url}) end");
        let short = "https://www.amazon.co.jp/dp/B012345678";
        assert_eq!(
            shorten(&text),
            format!("これ {short}。 https://example.com/a も ({short}) end")
        );
        assert_eq!(shorten("no links 日本語"), "no links 日本語");
    }

    #[test]
    fn threshold_is_counted_in_characters() {
        let base = "https://www.amazon.co.jp/x/dp/B012345678?keywords=";
        let fill = |total: usize| format!("{base}{}", "a".repeat(total - base.len()));
        assert_eq!(
            shorten(&fill(MIN_LENGTH)),
            "https://www.amazon.co.jp/dp/B012345678"
        );
        let below = fill(MIN_LENGTH - 1);
        assert_eq!(shorten(&below), below);
    }

    #[test]
    fn leaves_everything_else_unchanged() {
        let padding = "&qid=1234567890123&sr=8-1&crid=ABCDEFGHIJKLMNOP&sprefix=abcdefgh";
        let cases = [
            // short product link
            "https://www.amazon.co.jp/dp/B012345678".to_owned(),
            // official short links, however long
            format!("https://amzn.to/3abcdef?x={}", "a".repeat(120)),
            format!("https://a.co/d/abc?x={}", "a".repeat(120)),
            // spoofed and foreign hosts
            long("www.amazon.co.jp.evil.example", "/dp/B012345678"),
            long("notamazon.com", "/dp/B012345678"),
            long("evil.example", "/dp/B012345678"),
            long("user@www.amazon.com", "/dp/B012345678"),
            long("www.amazon.com:8443", "/dp/B012345678"),
            // meaning-bearing parameters, fragments
            format!("https://www.amazon.com/dp/B012345678?th=1&psc=1&keywords=a{padding}"),
            format!("https://www.amazon.com/dp/B012345678?smid=A1B2C3D4E5{padding}"),
            format!("https://www.amazon.com/dp/B012345678?tag=me-22{padding}"),
            format!("https://www.amazon.com/dp/B012345678?keywords=a{padding}#reviews"),
            // bad ASIN, unknown or non-product paths
            long("www.amazon.com", "/dp/b012345678"),
            long("www.amazon.com", "/dp/B01234567"),
            long("www.amazon.com", "/dp/B0123456789"),
            long("www.amazon.com", "/s"),
            long("www.amazon.com", "/gp/help/B012345678"),
            long("www.amazon.com", "/x/dp/B012345678/extra/segments"),
            long("www.amazon.com", "/a/b/dp/B012345678"),
        ];
        for text in cases {
            assert_eq!(shorten(&text), text);
        }
    }

    #[test]
    fn leaves_links_cut_at_a_quote_and_ctcp_text_alone() {
        let padding = "a".repeat(60);
        let quoted =
            format!("https://www.amazon.com/dp/B012345678?keywords={padding}'&smid=SELLER");
        assert_eq!(shorten(&quoted), quoted);
        let bracketed = format!("<{}>", long("www.amazon.com", "/dp/B012345678"));
        assert_eq!(shorten(&bracketed), bracketed);
        let ctcp = format!(
            "\u{1}PING {}\u{1}",
            long("www.amazon.co.jp", "/dp/B012345678")
        );
        assert_eq!(shorten(&ctcp), ctcp);
        let query = "?keywords=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        for stop in ['。', '、'] {
            let text = format!("https://www.amazon.com/dp/B012345678{query}{stop}&smid=SELLER");
            assert_eq!(shorten(&text), text);
        }
        let outer =
            format!("https://example.com/search?q='https://www.amazon.com/dp/B012345678{query}");
        assert_eq!(shorten(&outer), outer);
        for tail in ["。商品&smid=SELLER", "、商品&smid=SELLER", "。と"] {
            let text = format!("https://www.amazon.com/dp/B012345678{query}{tail}");
            assert_eq!(shorten(&text), text);
        }
        let nested =
            format!("https://example.com/search?q='(https://www.amazon.com/dp/B012345678{query}");
        assert_eq!(shorten(&nested), nested);
        let sentence = format!("see {}.", long("www.amazon.com", "/dp/B012345678"));
        assert_eq!(
            shorten(&sentence),
            "see https://www.amazon.com/dp/B012345678."
        );
    }
}
