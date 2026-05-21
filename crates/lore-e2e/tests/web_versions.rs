//! Phase A — web page versioning, re-archive, per-version delete.
//!
//! Tests run against the same DB the server uses (`TestApp.db_path`),
//! seeded directly via `lore-core::db::*` helpers (mirrors what the worker
//! would do, without needing headless Chrome). HTTP API is asserted from
//! the page via `api_post`/`fetch_raw`; UI assertions go through DOM.

use std::time::Duration;

use lore_e2e::TestApp;
use serde_json::json;

/// `request_reachive` HTTP method flips the page status back to `queued`.
/// Acts as the trigger the worker picks up — the worker itself is out of
/// scope here (would need Chromium); we just check the queue gets set.
#[tokio::test]
async fn request_reachive_sets_status_to_queued() {
    let app = TestApp::spawn().await.expect("spawn app");
    let page_id = app
        .seed_page_with_snapshots(
            "https://example.test/article",
            "Hello",
            &["initial body"],
        )
        .expect("seed page");

    // Sanity: starts archived.
    let detail = app
        .api_post("get_page", json!({ "page_id": page_id }))
        .await
        .expect("get_page");
    assert_eq!(detail["status"], "archived");

    app.api_post("request_reachive", json!({ "page_id": page_id }))
        .await
        .expect("request_reachive");

    let detail = app
        .api_post("get_page", json!({ "page_id": page_id }))
        .await
        .expect("get_page after reachive");
    assert_eq!(detail["status"], "queued");
}

/// `list_page_versions` returns all snapshots newest-first, with metadata
/// for each (id, version, size, hash, change_summary).
#[tokio::test]
async fn list_page_versions_returns_snapshots_newest_first() {
    let app = TestApp::spawn().await.expect("spawn app");
    let page_id = app
        .seed_page_with_snapshots(
            "https://example.test/multi",
            "Multi",
            &["v1 body", "v2 body changed", "v3 body changed again"],
        )
        .expect("seed page with 3 snapshots");

    let result = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list_page_versions");
    let versions = result.as_array().expect("array response");
    assert_eq!(versions.len(), 3, "should return 3 versions");

    // Newest first: v3, v2, v1.
    assert_eq!(versions[0]["version"], 3);
    assert_eq!(versions[1]["version"], 2);
    assert_eq!(versions[2]["version"], 1);

    // Every snapshot has a content_hash (backfilled at insert).
    for v in versions {
        assert!(
            v["content_hash"].is_string(),
            "content_hash should be populated, got {:?}",
            v["content_hash"]
        );
    }
}

/// `insert_snapshot` computes `change_summary` for v2+ based on diff vs
/// previous: title change, size delta, content_same flag.
#[tokio::test]
async fn change_summary_records_title_change_and_size_delta() {
    let app = TestApp::spawn().await.expect("spawn app");
    let page_id = app
        .seed_page_with_snapshots("https://example.test/diff", "Original", &["short body"])
        .expect("seed v1");

    // Title change happens *before* the next snapshot — the snapshot
    // captures the new title at fetch time.
    app.set_page_title(page_id, "Renamed").expect("rename");
    app.add_snapshot(page_id, "a much longer body than the first one had")
        .expect("seed v2");

    let result = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list versions");
    let v2 = &result.as_array().unwrap()[0]; // Newest first.
    assert_eq!(v2["version"], 2);

    let summary = v2["change_summary"].as_str().expect("v2 has summary");
    assert!(
        summary.contains("\"title_changed\":true"),
        "expected title_changed:true in {}",
        summary
    );
    assert!(
        summary.contains("\"content_same\":false"),
        "expected content_same:false in {}",
        summary
    );
    // Size went from 10 bytes to ~42 — definitely positive delta.
    assert!(
        summary.contains("size_delta_pct"),
        "expected size_delta_pct field in {}",
        summary
    );
}

/// Re-archiving the exact same body should mark v2 as `content_same: true`.
/// (Title unchanged, size unchanged.)
#[tokio::test]
async fn change_summary_flags_identical_bodies() {
    let app = TestApp::spawn().await.expect("spawn app");
    let page_id = app
        .seed_page_with_snapshots(
            "https://example.test/same",
            "Same",
            &["identical content", "identical content"],
        )
        .expect("seed two identical snapshots");

    let result = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list");
    let v2 = &result.as_array().unwrap()[0];
    let summary = v2["change_summary"].as_str().unwrap();
    assert!(
        summary.contains("\"content_same\":true"),
        "expected content_same:true in {}",
        summary
    );
    assert!(
        summary.contains("\"title_changed\":false"),
        "expected title_changed:false in {}",
        summary
    );
}

/// `delete_page_version` refuses to delete the only snapshot — callers
/// should trash the whole page instead.
#[tokio::test]
async fn delete_page_version_refuses_only_snapshot() {
    let app = TestApp::spawn().await.expect("spawn app");
    let page_id = app
        .seed_page_with_snapshots("https://example.test/single", "Single", &["only body"])
        .expect("seed");

    let versions = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list");
    let only_id = versions.as_array().unwrap()[0]["id"].as_i64().unwrap();

    // Direct fetch — `api_post` would unwrap to error; we want the body.
    let (status, body) = app
        .fetch_raw(
            "POST",
            "/api/delete_page_version",
            Some(&json!({ "snapshot_id": only_id }).to_string()),
        )
        .await
        .expect("fetch_raw");
    assert!(
        status >= 400,
        "expected 4xx/5xx for sole-snapshot delete, got {}: {}",
        status,
        body
    );
}

/// Deleting one snapshot among many drops it from the list but leaves the
/// others intact.
#[tokio::test]
async fn delete_page_version_drops_one_of_many() {
    let app = TestApp::spawn().await.expect("spawn app");
    let page_id = app
        .seed_page_with_snapshots(
            "https://example.test/three",
            "Three",
            &["v1", "v2", "v3"],
        )
        .expect("seed");

    let versions_before = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list");
    let arr = versions_before.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    // Find v2 by version number, not list index — robust against reordering.
    let v2_id = arr
        .iter()
        .find(|v| v["version"] == 2)
        .expect("v2 in list")["id"]
        .as_i64()
        .unwrap();

    app.api_post(
        "delete_page_version",
        json!({ "snapshot_id": v2_id }),
    )
    .await
    .expect("delete v2");

    let versions_after = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list again");
    let arr = versions_after.as_array().unwrap();
    assert_eq!(arr.len(), 2, "should have 2 versions left");
    let remaining: Vec<i64> = arr.iter().map(|v| v["version"].as_i64().unwrap()).collect();
    assert_eq!(remaining, vec![3, 1], "v2 should be gone, v3+v1 remain");
}

/// UI: opening a page with multiple versions renders the Versions panel
/// with one row per snapshot and the latest marked `current`.
#[tokio::test]
async fn versions_panel_renders_in_ui() {
    let app = TestApp::spawn().await.expect("spawn app");
    let _page_id = app
        .seed_page_with_snapshots(
            "https://example.test/ui",
            "UI test page",
            &["body one", "body two"],
        )
        .expect("seed");

    // Navigate to Webs section so the list shows the seeded page.
    app.click_text(".sidebar-item", "Webs")
        .await
        .expect("click Webs");

    // Wait for the page row, click it.
    let _ = app
        .wait_for(".list-item", Duration::from_secs(5))
        .await
        .expect("page appears in list");
    app.click(".list-item").await.expect("open page detail");

    // Versions panel + 2 rows.
    app.wait_for(".page-versions", Duration::from_secs(5))
        .await
        .expect("versions panel present");
    let rows = app
        .wait_until(
            || async {
                let els = app
                    .page
                    .find_elements(".version-row")
                    .await
                    .unwrap_or_default();
                if els.len() == 2 { Ok(Some(els.len())) } else { Ok(None) }
            },
            Duration::from_secs(5),
        )
        .await
        .expect("2 version rows visible");
    assert_eq!(rows, 2);

    // Latest gets the `current` badge.
    let current_text = app
        .text(".version-badge.badge-current")
        .await
        .expect("current badge");
    assert!(!current_text.is_empty(), "current badge has label");
}
