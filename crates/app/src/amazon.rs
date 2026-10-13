// SPDX-License-Identifier: MPL-2.0
//! Shortening of a pasted long Amazon product link (#315).
//!
//! Only pasted text that is exactly one link whose meaning can be kept is
//! rewritten, to `https://<same host>/dp/<ASIN>`. No network access is involved.

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
    "ie",
    "dchild",
    "sp_csd",
    "spla",
    "ufe",
    "refrid",
    "sbo",
];

/// Tracking parameter families: `__mk_ja_JP` (marketplace charset marker that
/// Japanese search results always add) and the other `pf_rd_*`/`pd_rd_*`.
const TRACKING_PREFIXES: &[&str] = &["__mk_", "pf_rd_", "pd_rd_"];

fn is_tracking(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    TRACKING.contains(&key.as_str()) || TRACKING_PREFIXES.iter().any(|p| key.starts_with(p))
}

/// The short form of pasted text that is, apart from surrounding whitespace
/// (full-width spaces included), exactly one long Amazon product link.
/// `None` for anything else, including text that mixes a link with words.
pub fn shorten_pasted(text: &str) -> Option<String> {
    let link = text.trim();
    if link.contains(char::is_whitespace) {
        return None;
    }
    shorten_link(link)
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
    if !url.query_pairs().all(|(key, _)| is_tracking(&key)) {
        return None;
    }
    let segments: Vec<&str> = url.path_segments()?.collect();
    // `/-/en/...` only picks the display language of the page.
    let segments = match segments.as_slice() {
        ["-", lang, rest @ ..] if lang.len() == 2 || lang.len() == 5 => rest,
        all => all,
    };
    let (slug_ok, asin, rest) = match segments {
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

    /// What pasting `text` inserts: the short form, or the text as it was.
    fn shorten(text: &str) -> String {
        shorten_pasted(text).unwrap_or_else(|| text.to_owned())
    }

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
    fn shortens_links_copied_from_search_results() {
        let short = "https://www.amazon.co.jp/dp/B012345678";
        for url in [
            "https://www.amazon.co.jp/%E5%95%86%E5%93%81/dp/B012345678/ref=sr_1_3?__mk_ja_JP=%E3%82%AB%E3%82%BF%E3%82%AB%E3%83%8A&crid=ABCDEFGHIJKLM&dib=eyJ2IjoiMSJ9&dib_tag=se&keywords=example&qid=1234567890123&sprefix=abc%2Caps%2C200&sr=8-3",
            "https://www.amazon.co.jp/-/en/Product/dp/B012345678/?pd_rd_w=abcde&pf_rd_p=abcdefgh-1234&ie=UTF8&dchild=1&sp_csd=d2lkZ2V0TmFtZT1zcF9hdGY&spla=1&pd_rd_wg=xyz",
        ] {
            assert!(url.chars().count() >= MIN_LENGTH, "{url}");
            assert_eq!(shorten(url), short, "{url}");
        }
    }

    #[test]
    fn leaves_plain_text_alone() {
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
    fn leaves_pasted_text_that_is_not_just_one_link_alone() {
        let url = long("www.amazon.com", "/dp/B012345678");
        for text in [
            format!("see {url}"),
            format!("{url} please"),
            format!("{url} {url}"),
            format!("{url}\n{url}"),
            format!("<{url}>"),
            format!("\u{1}PING {url}\u{1}"),
            format!("https://example.com/search?q='{url}"),
            String::new(),
            "   ".to_owned(),
        ] {
            assert_eq!(shorten(&text), text);
        }
    }

    #[test]
    fn ignores_whitespace_around_the_pasted_link() {
        let url = long("www.amazon.co.jp", "/x/dp/B012345678");
        let short = "https://www.amazon.co.jp/dp/B012345678";
        for text in [
            format!(" {url}"),
            format!("{url}\n"),
            format!("\t{url} "),
            format!("\u{3000}{url}\u{3000}"),
        ] {
            assert_eq!(shorten(&text), short);
        }
    }
}
