/// Property-based tests for pure functions in lore-core.
/// Run with: cargo test -p lore-core --test proptests
use lore_core::{merge, rules, search, serde_b64, url_extract};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};

// --- serde_b64 round-trip tests ---

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct B64TestBytes {
    #[serde(with = "serde_b64::vec")]
    data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct B64TestOptBytes {
    #[serde(with = "serde_b64::opt_vec")]
    data: Option<Vec<u8>>,
}

proptest! {
    #[test]
    fn prop_serde_b64_bytes_roundtrip(bytes in prop::collection::vec(any::<u8>(), 0..1000)) {
        let original = B64TestBytes { data: bytes };
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: B64TestBytes = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(original, restored);
    }

    #[test]
    fn prop_serde_b64_opt_bytes_roundtrip(bytes in prop::option::of(prop::collection::vec(any::<u8>(), 0..1000))) {
        let original = B64TestOptBytes { data: bytes };
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: B64TestOptBytes = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(original, restored);
    }
}

// --- merge::three_way_merge tests ---

proptest! {
    #[test]
    fn prop_merge_no_op_when_agree(text in ".*") {
        let result = merge::three_way_merge(&text, &text, &text);
        prop_assert_eq!(result.text, text);
        prop_assert!(!result.had_conflict);
    }

    #[test]
    fn prop_merge_ours_when_only_theirs_changed(
        base in ".*",
        ours in ".*",
    ) {
        let result = merge::three_way_merge(&base, &ours, &base);
        prop_assert_eq!(result.text, ours);
        prop_assert!(!result.had_conflict);
    }

    #[test]
    fn prop_merge_theirs_when_only_ours_changed(
        base in ".*",
        theirs in ".*",
    ) {
        let result = merge::three_way_merge(&base, &base, &theirs);
        prop_assert_eq!(result.text, theirs);
        prop_assert!(!result.had_conflict);
    }

    #[test]
    fn prop_merge_never_panics(
        base in ".*",
        ours in ".*",
        theirs in ".*",
    ) {
        let _ = merge::three_way_merge(&base, &ours, &theirs);
    }
}

// --- rules::normalize_url tests ---

proptest! {
    #[test]
    fn prop_normalize_url_idempotent(url_str in r"https?://[a-z0-9.-]+\.[a-z]{2,}(/[a-z0-9._-]*)*") {
        if let Ok(url) = url::Url::parse(&url_str) {
            let normalized_once = rules::normalize_url(&url);
            // normalize_url returns String, so parse it back to Url for idempotence check
            if let Ok(url_again) = url::Url::parse(&normalized_once) {
                let normalized_twice = rules::normalize_url(&url_again);
                prop_assert_eq!(normalized_once, normalized_twice);
            }
        }
    }

    #[test]
    fn prop_normalize_url_never_panics(url_str in ".*") {
        if let Ok(url) = url::Url::parse(&url_str) {
            let _ = rules::normalize_url(&url);
        }
    }
}

// --- rules::classify tests ---
// Note: classify needs rules, use empty vec for totality test (just checks never panics + non-empty)

proptest! {
    #[test]
    fn prop_classify_never_empty(url_str in r"https?://[a-z0-9.-]+\.[a-z]{2,}") {
        if let Ok(url) = url::Url::parse(&url_str) {
            let rules_empty = vec![];
            let category = rules::classify(&url, &rules_empty);
            prop_assert!(!category.is_empty(), "classify should return non-empty category");
        }
    }

    #[test]
    fn prop_classify_never_panics(url_str in ".*") {
        if let Ok(url) = url::Url::parse(&url_str) {
            let rules_empty = vec![];
            let _ = rules::classify(&url, &rules_empty);
        }
    }
}

// --- url_extract::extract_urls tests ---

proptest! {
    #[test]
    fn prop_extract_urls_all_valid(text in ".*") {
        let urls = url_extract::extract_urls(&text);
        for url_str in urls {
            let _ = url::Url::parse(&url_str)
                .expect(&format!("extracted URL must parse: {}", url_str));
        }
    }

    #[test]
    fn prop_extract_urls_never_panics(text in ".*") {
        let _ = url_extract::extract_urls(&text);
    }
}

// --- search::prepare_query tests ---

proptest! {
    #[test]
    fn prop_prepare_query_never_panics(query in ".*") {
        let _ = search::prepare_query(&query);
    }

    #[test]
    fn prop_prepare_query_appends_wildcard(word in "[a-z]{1,5}") {
        let result = search::prepare_query(&word);
        if !word.contains(|c: char| !c.is_alphanumeric()) {
            prop_assert!(result.contains('*'), "short word should have wildcard: {}", result);
        }
    }
}
