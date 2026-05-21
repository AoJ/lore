//! Phase A — web page versioning, re-archive, per-version delete.
//!
//! Tests run against the same DB the server uses (`TestApp.db_path`),
//! seeded directly via `lore-core::db::*` helpers (mirrors what the worker
//! would do, without needing headless Chrome). HTTP API is asserted from
//! the page via `api_post`/`fetch_raw`; UI assertions go through DOM.

use std::time::Duration;

use lore_e2e::TestApp;
use serde_json::json;

/// End-to-end re-archive workflow: queue a real page, run the worker
/// binary, confirm the page status walks queued → fetching → archived and
/// a snapshot lands. This exercises the actual pipeline (renderer +
/// snapshot insert + status updates), not just an API status flip.
///
/// Falls back to HTTP if Chrome is unavailable, which is fine for asserting
/// on workflow shape — we don't need visual fidelity here.
#[tokio::test]
async fn worker_runs_full_archive_workflow() {
    let app = TestApp::spawn().await.expect("spawn app");

    // Queue a real URL via the API (status = queued).
    let outcome = app
        .api_post(
            "archive_url",
            json!({
                "raw_url": "https://example.com/",
                "space_id": 1,
                "title": null,
                "source": null,
            }),
        )
        .await
        .expect("archive_url");
    let page_id = outcome["id"].as_i64().expect("page id");

    let pre = app
        .api_post("get_page", json!({ "page_id": page_id }))
        .await
        .expect("get_page pre-worker");
    assert_eq!(pre["status"], "queued");

    // Run the worker binary.
    let status = app.run_worker().expect("run worker");
    // 0 = ok, 2 = degraded (HTTP fallback after Chrome failure) — both
    // accepted because Chrome availability varies. 1 = real failure → fail.
    let code = status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 2,
        "worker should exit 0 or 2, got {}",
        code
    );

    let post = app
        .api_post("get_page", json!({ "page_id": page_id }))
        .await
        .expect("get_page post-worker");
    assert_eq!(post["status"], "archived", "page should be archived");
    let total: i64 = post["total_size_bytes"].as_i64().unwrap();
    assert!(total > 0, "snapshot should have non-zero size, got {}", total);

    // List versions — exactly one snapshot from the worker run.
    let versions = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list versions");
    let arr = versions.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["version"], 1);
}

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

/// Retry button must only show for `failed` pages — `queued` (worker
/// hasn't run yet) and `fetching` (worker in progress) used to render it
/// too, which made users think a normal pending state was a failure.
#[tokio::test]
async fn retry_button_does_not_show_for_queued_pages() {
    let app = TestApp::spawn().await.expect("spawn app");
    // archive_url leaves status = queued without running the worker.
    let outcome = app
        .api_post(
            "archive_url",
            json!({ "raw_url": "https://example.com/", "space_id": 1, "title": null, "source": null }),
        )
        .await
        .expect("archive_url");
    assert!(outcome["id"].as_i64().is_some());

    app.click_text(".sidebar-item", "Webs").await.expect("Webs");
    let _ = app
        .wait_for(".list-item", Duration::from_secs(5))
        .await
        .expect("page row");
    app.click(".list-item").await.expect("open detail");

    // Status chip should be the warning-colored "queued" badge, not Retry.
    app.wait_for(".status-chip.status-queued", Duration::from_secs(5))
        .await
        .expect("queued status chip visible");

    let action_texts: String = app
        .page
        .evaluate(
            "Array.from(document.querySelectorAll('.page-actions .btn')).map(b => b.textContent.trim()).join('|')",
        )
        .await
        .expect("eval action buttons")
        .into_value()
        .unwrap_or_default();
    assert!(
        !action_texts.contains("Retry"),
        "Retry button must not appear for a queued page, got buttons: '{}'",
        action_texts
    );
}

/// UI: opening a page with multiple versions shows the inline version
/// selector in the header. Clicking it opens a popover with one row per
/// snapshot and the latest marked `current`.
#[tokio::test]
async fn version_selector_opens_picker_with_all_versions() {
    let app = TestApp::spawn().await.expect("spawn app");
    let _page_id = app
        .seed_page_with_snapshots(
            "https://example.test/ui",
            "UI test page",
            &["body one", "body two"],
        )
        .expect("seed");

    app.click_text(".sidebar-item", "Webs")
        .await
        .expect("click Webs");
    let _ = app
        .wait_for(".list-item", Duration::from_secs(5))
        .await
        .expect("page row appears");
    app.click(".list-item").await.expect("open page detail");

    // Header shows "v2 · date" — chevron present (multi-version mode).
    let selector_text = app
        .text(".version-selector")
        .await
        .expect("version selector visible");
    assert!(
        selector_text.starts_with("v2"),
        "header should start with v2 (latest), got: {}",
        selector_text
    );

    // Click selector → popover opens with 2 rows.
    app.click(".version-selector").await.expect("open picker");
    app.wait_for(".version-picker-popover", Duration::from_secs(2))
        .await
        .expect("popover opens");
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

    let current_text = app
        .text(".version-badge.badge-current")
        .await
        .expect("current badge");
    assert!(!current_text.is_empty(), "current badge has label");
}

/// `delete_page_version` recomputes the next snapshot's `change_summary`
/// because its diff base just changed. v1+v2+v3 → delete v2 → v3's summary
/// now reflects v3 vs v1 instead of v3 vs v2.
#[tokio::test]
async fn delete_middle_version_recomputes_next_summary() {
    let app = TestApp::spawn().await.expect("spawn app");
    let page_id = app
        .seed_page_with_snapshots(
            "https://example.test/recompute",
            "R",
            // v1 small, v2 huge, v3 same as v1
            &["aaa", "bbbbbbbbbbbbbbbbbbbb", "aaa"],
        )
        .expect("seed");

    let arr = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list");
    let arr = arr.as_array().unwrap();
    // Before delete: v3 diffs against v2 (size went down a lot, contents differ).
    let v3_before = arr.iter().find(|v| v["version"] == 3).unwrap();
    let v2 = arr.iter().find(|v| v["version"] == 2).unwrap();
    let v2_id = v2["id"].as_i64().unwrap();
    let summary_before = v3_before["change_summary"].as_str().unwrap();
    assert!(
        summary_before.contains("\"content_same\":false"),
        "before delete v3 differs from v2: {}",
        summary_before
    );

    app.api_post("delete_page_version", json!({ "snapshot_id": v2_id }))
        .await
        .expect("delete v2");

    let arr_after = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list after");
    let arr_after = arr_after.as_array().unwrap();
    let v3_after = arr_after.iter().find(|v| v["version"] == 3).unwrap();
    let summary_after = v3_after["change_summary"].as_str().unwrap();
    // v1 == "aaa" and v3 == "aaa" → after the gap, v3 should now show
    // content_same=true and size_delta_pct=0.
    assert!(
        summary_after.contains("\"content_same\":true"),
        "after delete v3 should match v1: {}",
        summary_after
    );
    assert!(
        summary_after.contains("\"size_delta_pct\":0"),
        "after delete v3 size matches v1: {}",
        summary_after
    );
}

/// `auto_archive_from_text` ignores internal attachment URLs — they are a
/// rendering protocol for note file blocks, not real pages.
#[tokio::test]
async fn auto_archive_skips_internal_attachment_urls() {
    let app = TestApp::spawn().await.expect("spawn app");
    let text = "See [file](https://attachment.lore.invalid/42) and \
                [article](https://example.test/real)";
    let count = app
        .api_post(
            "auto_archive_from_text",
            json!({ "text": text, "space_id": 1 }),
        )
        .await
        .expect("auto_archive_from_text");
    // Only the real URL should land in DB.
    assert_eq!(count, 1, "exactly one URL should be queued, got {}", count);

    // List pages — no attachment.lore.invalid row should be present.
    let pages = app
        .api_post("list_pages", json!({ "space_id": 1, "limit": 100 }))
        .await
        .expect("list_pages");
    let arr = pages.as_array().unwrap();
    for p in arr {
        assert_ne!(
            p["domain"], "attachment.lore.invalid",
            "internal attachment domain should never appear in Web list"
        );
    }
    assert_eq!(arr.len(), 1, "exactly one real page should be present");
}

/// `archive_url` refuses internal attachment URLs even when called directly
/// (defensive — auto_archive already filters, but a manual paste shouldn't
/// poison the list either).
#[tokio::test]
async fn archive_url_refuses_internal_attachment_urls() {
    let app = TestApp::spawn().await.expect("spawn app");
    let (status, body) = app
        .fetch_raw(
            "POST",
            "/api/archive_url",
            Some(
                &json!({
                    "raw_url": "https://attachment.lore.invalid/42",
                    "space_id": 1,
                    "title": null,
                    "source": null,
                })
                .to_string(),
            ),
        )
        .await
        .expect("fetch");
    assert!(
        status >= 400,
        "internal attachment URL should be refused, got {}: {}",
        status,
        body
    );
}

/// `WebPageDetail.total_size_bytes` aggregates all snapshots, not just the
/// latest. Verified by seeding 3 snapshots and checking total > 3× the size
/// of any single one (since title + html columns add overhead too).
#[tokio::test]
async fn total_size_aggregates_across_snapshots() {
    let app = TestApp::spawn().await.expect("spawn app");
    let one_body = "x".repeat(1000); // 1 KB plain_text per snapshot
    let bodies = [one_body.as_str(), one_body.as_str(), one_body.as_str()];
    let page_id = app
        .seed_page_with_snapshots("https://example.test/big", "BigPage", &bodies)
        .expect("seed");

    let detail = app
        .api_post("get_page", json!({ "page_id": page_id }))
        .await
        .expect("get_page");

    let total = detail["total_size_bytes"].as_i64().expect("total_size_bytes field");
    // Each snapshot: 1000 (text) + 13 (<html></html>) + 0 (screenshot) + 7 (title)
    // ≈ 1020 bytes. Three snapshots → ~3060. Be lenient: assert > 3000.
    assert!(
        total >= 3000,
        "total_size should sum across all 3 snapshots, got {}",
        total
    );

    // Sanity: last_fetched_at should be populated.
    assert!(
        detail["last_fetched_at"].is_string(),
        "last_fetched_at populated when snapshots exist, got {:?}",
        detail["last_fetched_at"]
    );
}

/// UI re-archive button must actually toggle the DB status to `queued` —
/// reproduces user-reported "click Re-archive, worker says no work" issue.
#[tokio::test]
async fn clicking_reachive_button_flips_status_to_queued() {
    let app = TestApp::spawn().await.expect("spawn app");
    // Seed an archived page (status = archived, 1 snapshot).
    let page_id = app
        .seed_page_with_snapshots("https://example.test/btn", "Btn", &["body"])
        .expect("seed");
    // Page rows come from `list_pages` which only sees archived pages by
    // default, so manually flip to archived (seed creates `archived` already).
    let conn = app.conn().unwrap();
    conn.execute(
        "UPDATE web_page SET status = 'archived' WHERE id = ?1",
        rusqlite::params![page_id],
    )
    .unwrap();
    drop(conn);

    app.click_text(".sidebar-item", "Webs").await.expect("Webs");
    let _ = app
        .wait_for(".list-item", Duration::from_secs(5))
        .await
        .expect("page row");
    app.click(".list-item").await.expect("open detail");

    // Click the Re-archive button by text (button label).
    app.wait_for_default(".page-actions").await.expect("actions");
    app.click_text(".page-actions .btn", "Re-archive")
        .await
        .expect("Re-archive button");

    // Status in DB must flip to queued within a poll tick.
    let pid = page_id;
    let _ = app
        .wait_until(
            || async {
                let v = app
                    .api_post("get_page", json!({ "page_id": pid }))
                    .await
                    .ok();
                match v.as_ref().map(|x| x["status"].as_str()) {
                    Some(Some("queued")) => Ok(Some(())),
                    _ => Ok(None),
                }
            },
            Duration::from_secs(5),
        )
        .await
        .expect("Re-archive button must set status to queued");
}

/// Full re-archive cycle: archive → worker → re-archive → worker → 2 versions.
/// Validates the user-visible loop: clicking "Re-archive" actually produces a
/// new snapshot when the worker runs next, instead of silently doing nothing.
#[tokio::test]
async fn rearchive_cycle_produces_second_snapshot() {
    let app = TestApp::spawn().await.expect("spawn app");
    let outcome = app
        .api_post(
            "archive_url",
            json!({ "raw_url": "https://example.com/", "space_id": 1, "title": null, "source": null }),
        )
        .await
        .expect("archive_url");
    let page_id = outcome["id"].as_i64().unwrap();

    // First worker run → v1.
    let status1 = app.run_worker().expect("worker 1");
    assert!(matches!(status1.code(), Some(0) | Some(2)));
    let v1 = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list");
    assert_eq!(v1.as_array().unwrap().len(), 1);

    // Trigger re-archive: status should flip to queued.
    app.api_post("request_reachive", json!({ "page_id": page_id }))
        .await
        .expect("request_reachive");
    let queued = app
        .api_post("get_page", json!({ "page_id": page_id }))
        .await
        .expect("get_page after reachive");
    assert_eq!(queued["status"], "queued");

    // Second worker run → v2.
    let status2 = app.run_worker().expect("worker 2");
    assert!(matches!(status2.code(), Some(0) | Some(2)));
    let v2 = app
        .api_post("list_page_versions", json!({ "page_id": page_id }))
        .await
        .expect("list 2");
    assert_eq!(
        v2.as_array().unwrap().len(),
        2,
        "after re-archive + worker run, page should have 2 snapshots"
    );

    // Final status back to archived.
    let final_state = app
        .api_post("get_page", json!({ "page_id": page_id }))
        .await
        .expect("get_page final");
    assert_eq!(final_state["status"], "archived");
}

/// A fresh snapshot inserted while the detail is open shows up in the
/// version picker after the polling refresh, **without** kicking the user
/// off the version they had selected. The header version label stays put
/// (the active selection is preserved); the picker count grows.
#[tokio::test]
async fn new_snapshot_appears_in_picker_without_changing_selection() {
    let app = TestApp::spawn().await.expect("spawn app");
    let page_id = app
        .seed_page_with_snapshots("https://example.test/live", "Live", &["v1 only"])
        .expect("seed v1");

    app.click_text(".sidebar-item", "Webs")
        .await
        .expect("click Webs");
    let _ = app
        .wait_for(".list-item", Duration::from_secs(5))
        .await
        .expect("row appears");
    app.click(".list-item").await.expect("open detail");

    // Confirm we're on v1 (single-version mode → no chevron).
    let header = app
        .wait_until(
            || async {
                let txt = app.text(".version-selector").await.ok();
                match txt {
                    Some(t) if t.starts_with("v1") => Ok(Some(t)),
                    _ => Ok(None),
                }
            },
            Duration::from_secs(3),
        )
        .await
        .expect("v1 visible");
    assert!(header.starts_with("v1"));
    assert!(
        !header.contains('▾'),
        "single-version header has no chevron, got: {}",
        header
    );

    // Worker appends v2 to DB.
    app.add_snapshot(page_id, "v2 content").expect("add v2");

    // Polling tick (2 s) → effect re-runs → chevron appears (multi-version),
    // but the currently-selected version stays v1.
    let new_header = app
        .wait_until(
            || async {
                let txt = app.text(".version-selector").await.ok();
                match txt {
                    Some(t) if t.contains('▾') => Ok(Some(t)),
                    _ => Ok(None),
                }
            },
            Duration::from_secs(15),
        )
        .await
        .expect("chevron appears after second snapshot");
    assert!(
        new_header.starts_with("v1"),
        "user's selection (v1) is preserved when a new version lands, got: {}",
        new_header
    );

    // Open picker — both versions are listed now.
    app.click(".version-selector").await.expect("open picker");
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
        .expect("2 version rows visible in picker");
    assert_eq!(rows, 2);
}

/// Selecting a different version in the picker updates the header label
/// and the rendered preview text.
#[tokio::test]
async fn selecting_older_version_repaints_header_and_preview() {
    let app = TestApp::spawn().await.expect("spawn app");
    let _page_id = app
        .seed_page_with_snapshots(
            "https://example.test/repaint",
            "Repaint",
            &["FIRSTVERSIONTEXT", "SECONDVERSIONTEXT"],
        )
        .expect("seed");

    app.click_text(".sidebar-item", "Webs")
        .await
        .expect("click Webs");
    let _ = app
        .wait_for(".list-item", Duration::from_secs(5))
        .await
        .expect("page row");
    app.click(".list-item").await.expect("open page");

    // Default = v2 in header.
    let header = app.text(".version-selector").await.expect("header label");
    assert!(header.starts_with("v2"), "default to newest, got: {}", header);

    // Open picker, click the v1 row (last in DESC order).
    app.click(".version-selector").await.expect("open picker");
    app.wait_for(".version-row", Duration::from_secs(2))
        .await
        .expect("rows render");

    // The version-row whose `.version-num` text is "v1" — use JS click_text
    // on the row container by version-num span text.
    let clicked: bool = app
        .page
        .evaluate(
            r#"
            (() => {
                const rows = Array.from(document.querySelectorAll('.version-row'));
                const target = rows.find(r => {
                    const num = r.querySelector('.version-num');
                    return num && num.textContent.trim() === 'v1';
                });
                if (target) { target.click(); return true; }
                return false;
            })()
            "#,
        )
        .await
        .expect("eval click")
        .into_value()
        .expect("bool");
    assert!(clicked, "v1 row found and clicked");

    // Header should now lead with v1.
    let new_header = app
        .wait_until(
            || async {
                let txt = app.text(".version-selector").await.ok();
                match txt {
                    Some(t) if t.starts_with("v1") => Ok(Some(t)),
                    _ => Ok(None),
                }
            },
            Duration::from_secs(3),
        )
        .await
        .expect("header switches to v1");
    assert!(new_header.starts_with("v1"));
}
