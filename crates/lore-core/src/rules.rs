use url::Url;

use crate::db::ClassificationRule;

/// Classify a URL against loaded rules. Returns category string.
/// Default (no rule match) = "archive".
pub fn classify(url: &Url, rules: &[ClassificationRule]) -> String {
    let scheme = url.scheme();

    // Hard rules that don't belong in DB
    if scheme == "file" {
        return "local".to_string();
    }
    if scheme == "chrome" || scheme == "chrome-extension" || scheme == "about" {
        return "discard".to_string();
    }

    let host = url.host_str().unwrap_or("");
    let full_url = url.as_str();

    // Check for private IP ranges (not practical to store in DB)
    if is_private_network(host) {
        return "local".to_string();
    }

    // Evaluate DB rules by priority (already sorted)
    for rule in rules {
        let matched = match rule.match_type.as_str() {
            "domain" => host == rule.pattern || host == format!("www.{}", rule.pattern),
            "domain_suffix" => {
                host == rule.pattern
                    || host.ends_with(&format!(".{}", rule.pattern))
                    || host == format!("www.{}", rule.pattern)
            }
            "url_prefix" => {
                // Match against host+path (without scheme)
                let host_path = format!("{}{}", host, url.path());
                host_path.starts_with(&rule.pattern)
            }
            "url_contains" => full_url.contains(&rule.pattern),
            _ => false,
        };
        if matched {
            return rule.category.clone();
        }
    }

    "archive".to_string()
}

fn is_private_network(host: &str) -> bool {
    if host == "localhost" || host == "127.0.0.1" || host == "0.0.0.0" {
        return true;
    }
    if let Some(first) = host.split('.').next()
        && first == "10"
    {
        return true;
    }
    if host.starts_with("192.168.") {
        return true;
    }
    if host.starts_with("172.")
        && let Some(second) = host.split('.').nth(1)
        && let Ok(n) = second.parse::<u8>()
        && (16..=31).contains(&n)
    {
        return true;
    }
    false
}

/// Normalize a URL for deduplication.
pub fn normalize_url(url: &Url) -> String {
    let scheme = url.scheme();
    let host = url.host_str().unwrap_or("").to_lowercase();
    let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
    let path = url.path();

    let path = if path.len() > 1 && path.ends_with('/') {
        &path[..path.len() - 1]
    } else {
        path
    };

    let filtered_query = if let Some(query) = url.query() {
        let mut params: Vec<(&str, &str)> = query
            .split('&')
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next()?;
                let value = parts.next().unwrap_or("");
                Some((key, value))
            })
            .filter(|(key, _)| !is_tracking_param(key))
            .collect();
        params.sort_by_key(|(k, _)| *k);
        if params.is_empty() {
            String::new()
        } else {
            let qs: Vec<String> = params
                .iter()
                .map(|(k, v)| {
                    if v.is_empty() {
                        k.to_string()
                    } else {
                        format!("{}={}", k, v)
                    }
                })
                .collect();
            format!("?{}", qs.join("&"))
        }
    } else {
        String::new()
    };

    format!("{}://{}{}{}{}", scheme, host, port, path, filtered_query)
}

fn is_tracking_param(key: &str) -> bool {
    let key_lower = key.to_lowercase();
    if key_lower.starts_with("utm_") {
        return true;
    }
    const TRACKING: &[&str] = &[
        "sca_esv",
        "sxsrf",
        "ei",
        "ved",
        "uact",
        "oq",
        "gs_lp",
        "sclient",
        "biw",
        "bih",
        "iflsig",
        "zx",
        "dsh",
        "flowname",
        "followup",
        "ifkv",
        "rart",
        "service",
        "es_id",
        "nis",
        "session_redirect",
    ];
    TRACKING.contains(&key_lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, match_type: &str, category: &str) -> ClassificationRule {
        ClassificationRule {
            pattern: pattern.to_string(),
            match_type: match_type.to_string(),
            category: category.to_string(),
            note: String::new(),
        }
    }

    fn make_rules() -> Vec<ClassificationRule> {
        // Simulate what seed.sql would produce
        vec![
            rule("www.google.com/search", "url_prefix", "discard"),
            rule("accounts.google.com", "domain", "discard"),
            rule("www.linkedin.com/feed", "url_prefix", "discard"),
            rule("deepl.com", "domain_suffix", "discard"),
            rule("portal.azure.com", "domain", "discard"),
            rule("localhost", "domain", "local"),
        ]
    }

    fn classify_url(s: &str) -> String {
        let url = Url::parse(s).unwrap();
        let rules = make_rules();
        classify(&url, &rules)
    }

    #[test]
    fn test_google_search_discard() {
        assert_eq!(
            classify_url("https://www.google.com/search?q=test"),
            "discard"
        );
    }

    #[test]
    fn test_linkedin_feed_discard() {
        assert_eq!(classify_url("https://www.linkedin.com/feed/"), "discard");
    }

    #[test]
    fn test_deepl_discard() {
        assert_eq!(
            classify_url("https://www.deepl.com/en/translator"),
            "discard"
        );
    }

    #[test]
    fn test_github_archive() {
        assert_eq!(
            classify_url("https://github.com/boilingdata/boilstream"),
            "archive"
        );
    }

    #[test]
    fn test_file_local() {
        assert_eq!(classify_url("file:///Users/aoj/Downloads/foo.pdf"), "local");
    }

    #[test]
    fn test_private_ip_local() {
        assert_eq!(
            classify_url("http://10.17.13.1:23000/merge-pdf.html"),
            "local"
        );
    }

    #[test]
    fn test_azure_discard() {
        assert_eq!(
            classify_url("https://portal.azure.com/#view/something"),
            "discard"
        );
    }

    #[test]
    fn test_default_archive() {
        assert_eq!(
            classify_url("https://www.example.com/some-article"),
            "archive"
        );
    }

    #[test]
    fn test_normalize_strips_tracking() {
        let url = Url::parse("https://www.google.com/search?q=test&sca_esv=123&ei=xyz").unwrap();
        assert_eq!(normalize_url(&url), "https://www.google.com/search?q=test");
    }

    #[test]
    fn test_normalize_sorts_params() {
        let url = Url::parse("https://example.com/path?z=1&a=2").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/path?a=2&z=1");
    }

    // --- Scheme alternatives (line 14 OR chain) ---

    #[test]
    fn chrome_internal_url_is_discard() {
        assert_eq!(classify_url("chrome://settings/"), "discard");
    }

    #[test]
    fn chrome_extension_url_is_discard() {
        assert_eq!(
            classify_url("chrome-extension://abc/options.html"),
            "discard"
        );
    }

    #[test]
    fn about_url_is_discard() {
        assert_eq!(classify_url("about:blank"), "discard");
    }

    // --- domain_suffix branches (lines 31..34) ---

    #[test]
    fn domain_suffix_matches_exact_root() {
        // host == rule.pattern (no www, no subdomain)
        assert_eq!(classify_url("https://deepl.com/translator"), "discard");
    }

    #[test]
    fn domain_suffix_matches_subdomain() {
        // host.ends_with(.pattern)
        assert_eq!(classify_url("https://api.deepl.com/v1"), "discard");
    }

    #[test]
    fn domain_suffix_matches_www_form() {
        // host == "www.{pattern}" branch
        assert_eq!(
            classify_url("https://www.deepl.com/en/translator"),
            "discard"
        );
    }

    #[test]
    fn domain_suffix_rejects_unrelated_host_containing_pattern() {
        // "deepl.com.evil.example" must not match — guards against naive substring logic
        assert_eq!(classify_url("https://deepl.com.evil.example/x"), "archive");
    }

    // --- url_contains rule type (line 40 match arm) ---

    #[test]
    fn url_contains_rule_matches() {
        let rules = vec![rule("/share/", "url_contains", "discard")];
        let url = Url::parse("https://example.com/api/share/thing").unwrap();
        assert_eq!(classify(&url, &rules), "discard");
    }

    #[test]
    fn url_contains_rule_does_not_match_when_absent() {
        let rules = vec![rule("/share/", "url_contains", "discard")];
        let url = Url::parse("https://example.com/api/thing").unwrap();
        assert_eq!(classify(&url, &rules), "archive");
    }

    // --- is_private_network branches (line 52 OR chain) ---

    #[test]
    fn private_network_localhost_alone() {
        assert_eq!(classify_url("http://localhost:8080/"), "local");
    }

    #[test]
    fn private_network_loopback_ip() {
        assert_eq!(classify_url("http://127.0.0.1:8080/"), "local");
    }

    #[test]
    fn private_network_wildcard_ip() {
        assert_eq!(classify_url("http://0.0.0.0:8080/"), "local");
    }

    #[test]
    fn private_network_192_168_range() {
        assert_eq!(classify_url("http://192.168.1.10/"), "local");
    }

    #[test]
    fn private_network_172_16_through_31() {
        // Boundary checks on the 172.16..=172.31 range
        assert_eq!(classify_url("http://172.16.0.1/"), "local");
        assert_eq!(classify_url("http://172.20.4.4/"), "local");
        assert_eq!(classify_url("http://172.31.255.255/"), "local");
    }

    #[test]
    fn private_network_172_15_is_public() {
        // Just outside the private range
        assert_eq!(classify_url("http://172.15.0.1/"), "archive");
    }

    #[test]
    fn private_network_172_32_is_public() {
        // Just outside the private range
        assert_eq!(classify_url("http://172.32.0.1/"), "archive");
    }

    // --- normalize_url path handling (lines 80..81) ---

    #[test]
    fn normalize_strips_trailing_slash_from_nonroot_path() {
        let url = Url::parse("https://example.com/foo/bar/").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/foo/bar");
    }

    #[test]
    fn normalize_keeps_root_slash() {
        // path.len() > 1 guard: "/" must stay as "/"
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/");
    }

    #[test]
    fn normalize_leaves_path_without_trailing_slash_alone() {
        let url = Url::parse("https://example.com/foo/bar").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/foo/bar");
    }

    #[test]
    fn normalize_strips_two_char_path_with_trailing_slash() {
        // Specifically exercises path.len() > 1 (== 2 here): "/a/" → "/a"
        let url = Url::parse("https://example.com/a/").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/a");
    }
}
