// SPDX-License-Identifier: MPL-2.0
//! Shortening of a pasted long Amazon product link (#315).
//!
//! Only pasted text that is exactly one product link is rewritten, to
//! `https://<same host>/dp/<ASIN>`. The query is not inspected: the product
//! path alone decides, so search and other non-product pages are never
//! shortened. No network access is involved.

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
    fn shortens_the_reported_916_byte_search_result_link() {
        let url = "https://www.amazon.co.jp/%E3%80%90%E7%B4%94%E6%AD%A3%E5%93%81%E3%80%91DualSense-%E3%83%AF%E3%82%A4%E3%83%A4%E3%83%AC%E3%82%B9%E3%82%B3%E3%83%B3%E3%83%88%E3%83%AD%E3%83%BC%E3%83%A9%E3%83%BC-%E3%82%B3%E3%82%BA%E3%83%9F%E3%83%83%E3%82%AF-%E3%83%AC%E3%83%83%E3%83%89-CFI-ZCT1J02/dp/B094VGQPPG/ref=sr_1_2_sspa?__mk_ja_JP=%E3%82%AB%E3%82%BF%E3%82%AB%E3%83%8A&crid=13B3V06V290CW&dib=eyJ2IjoiMSJ9.B1dqHCjOyTciiR0jSnXJTpEtG7ljJETPazEU3SFIpBqzWNyNPMwrE-4H4ggBYTiHetw5Yo1JfFbvdhD18yuySlRkMeHyt6S6nJJEJmvo48VJI7nay4Btcg22uCZUENUKlpZQ5-Nv5Q64DYzCX722PEjU_Z8_AIs1K_XrrlHXpK-sMnKSwLIp_O3Q7OsTKa186NjxEF9NQHHsrJe4Sj8LUbfwOlgzEA27YEItWNRSnE9i7bmg5P6_lGeDAjFwDFe-_Z55__MlLPsz6B5IssDv1ffu7ebb3CR9meZHwoWLbps.u6VcP0zZJYtU_H_H32gnp5u19cOWTxeNIwTfJ3fvgx0&dib_tag=se&keywords=dualsense&qid=1791885945&sprefix=dualsens%2Caps%2C187&sr=8-2-spons&ufe=app_do%3Aamzn1.fos.bf59e452-7c16-4eb0-8b6f-4f4edb3d8c50&sp_csd=d2lkZ2V0TmFtZT1zcF9hdGY&psc=1";
        assert_eq!(shorten(url), "https://www.amazon.co.jp/dp/B094VGQPPG");
    }

    #[test]
    fn ignores_the_query_of_a_product_link() {
        let padding = "&qid=1234567890123&sr=8-1&crid=ABCDEFGHIJKLMNOP&sprefix=abcdefgh";
        for query in [
            "th=1&psc=1",
            "smid=A1B2C3D4E5",
            "tag=me-22",
            "m=A1&ascsubtag=x",
        ] {
            let url = format!("https://www.amazon.com/dp/B012345678?{query}{padding}");
            assert_eq!(
                shorten(&url),
                "https://www.amazon.com/dp/B012345678",
                "{url}"
            );
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
            // fragments
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
