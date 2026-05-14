//! Extract http(s) URLs from text. Pure parser, no DB, no network.

/// Find every http/https URL embedded in `text`. Recognizes:
/// - Markdown links: `[label](https://example.com)` — only the URL part is returned
/// - Bare URLs separated by whitespace
///
/// URLs are de-duplicated while preserving first-seen order. Trailing
/// punctuation (`,;.<>()"'`) is trimmed from bare URLs.
pub fn extract_urls(text: &str) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();

    // Pattern 1: [label](url)
    let mut rest = text;
    while let Some(pos) = rest.find("](") {
        let start = pos + 2;
        if let Some(end) = rest[start..].find(')') {
            let url = rest[start..start + end].trim();
            if is_http_url(url) && !urls.iter().any(|u| u == url) {
                urls.push(url.to_string());
            }
            rest = &rest[start + end..];
        } else {
            break;
        }
    }

    // Pattern 2: bare URLs
    for word in text.split_whitespace() {
        let trimmed = word.trim_matches(|c: char| {
            matches!(c, '(' | ')' | '<' | '>' | '"' | '\'' | ',' | ';' | '.')
        });
        if is_http_url(trimmed) && !urls.iter().any(|u| u == trimmed) {
            urls.push(trimmed.to_string());
        }
    }

    urls
}

fn is_http_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_returns_empty() {
        assert!(extract_urls("").is_empty());
    }

    #[test]
    fn no_urls_returns_empty() {
        assert!(extract_urls("plain text without links").is_empty());
    }

    #[test]
    fn finds_bare_url() {
        let urls = extract_urls("see https://example.com for details");
        assert_eq!(urls, vec!["https://example.com"]);
    }

    #[test]
    fn finds_markdown_link() {
        let urls = extract_urls("read [the docs](https://docs.example.com/x) now");
        assert_eq!(urls, vec!["https://docs.example.com/x"]);
    }

    #[test]
    fn finds_both_markdown_and_bare() {
        let urls = extract_urls("a [link](https://a.com) and bare https://b.com");
        assert_eq!(urls, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn deduplicates_preserving_first_seen_order() {
        let urls = extract_urls("https://a.com [x](https://a.com) https://b.com");
        assert_eq!(urls, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn trims_trailing_punctuation() {
        let urls = extract_urls("visit https://example.com, then leave.");
        assert_eq!(urls, vec!["https://example.com"]);
    }

    #[test]
    fn ignores_non_http_schemes() {
        let urls = extract_urls("ftp://x.com mailto:a@b.com [doc](file:///tmp/x)");
        assert!(urls.is_empty());
    }

    #[test]
    fn accepts_plain_http() {
        let urls = extract_urls("see http://example.com");
        assert_eq!(urls, vec!["http://example.com"]);
    }

    #[test]
    fn handles_unclosed_markdown_link_gracefully() {
        // No panic, just skips the malformed bit
        let urls = extract_urls("broken [link](https://a.com missing-paren");
        assert!(urls.is_empty() || urls == vec!["https://a.com"]);
    }

    #[test]
    fn dedups_repeated_bare_url_in_same_text() {
        // Drives the `u == url` check at the bare-pass dedup: three identical
        // bare URLs must collapse to one. With the comparator flipped to !=,
        // every occurrence would be (wrongly) added.
        let urls = extract_urls("https://a.com https://a.com https://a.com");
        assert_eq!(urls, vec!["https://a.com"]);
    }

    #[test]
    fn dedups_repeated_markdown_link_in_same_text() {
        // Bare-pass dedup is exercised separately. This test pins the
        // markdown-pass dedup at the `]( ... )` branch — same URL appearing
        // in two markdown links must yield exactly one entry.
        let urls = extract_urls("[a](https://x.com) and [b](https://x.com)");
        assert_eq!(urls, vec!["https://x.com"]);
    }
}

/// Formal-verification harnesses. Compile-gated; reached only via `cargo kani`.
#[cfg(kani)]
mod proofs {
    use super::*;

    /// Empty input emits no URLs. Pins the empty-string early-exit through
    /// both the markdown-pass `find` loop and the bare-pass `split_whitespace`.
    #[kani::proof]
    fn empty_input_emits_no_urls() {
        assert!(extract_urls("").is_empty());
    }

    /// A bare HTTPS URL on its own is returned intact. Drives the bare-pass
    /// `is_http_url` check and the `Vec::push` path.
    #[kani::proof]
    fn bare_https_url_is_extracted() {
        let urls = extract_urls("https://a.io");
        assert_eq!(urls.len(), 1);
    }

    /// A markdown link is unwrapped to just the URL. Drives the
    /// `find("](")` + `find(')')` path through the parser.
    #[kani::proof]
    fn markdown_link_unwraps_to_url() {
        let urls = extract_urls("[x](https://a.io)");
        assert_eq!(urls.len(), 1);
    }
}
