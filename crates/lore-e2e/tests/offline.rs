//! Tests for backend-unavailability behavior.
//!
//! Scenario: the lore-server process becomes unreachable mid-session. The
//! frontend must:
//!   1. show a prominent `.offline-banner` (not an alert)
//!   2. keep the last-known data visible — lists must not be cleared
//!   3. clear the banner automatically once the server recovers

use std::time::Duration;

use lore_e2e::TestApp;
use serde_json::json;

/// Seed a note, kill the server, verify the banner appears and the note is
/// still visible in the list (data not wiped).
#[tokio::test]
async fn offline_banner_appears_and_data_preserved() {
    let mut app = TestApp::spawn().await.expect("spawn");

    app.api_post(
        "create_note",
        json!({
            "title": "Offline test note",
            "body":  "Should survive the outage",
            "folder_id": null,
            "space_id": 1,
        }),
    )
    .await
    .expect("create_note");

    // Wait for the note to appear in the list (first poll tick).
    app.wait_until(
        || async {
            let items = app
                .page
                .find_elements(".list-item")
                .await
                .unwrap_or_default();
            Ok(if items.is_empty() { None } else { Some(()) })
        },
        Duration::from_secs(8),
    )
    .await
    .expect("note in list before outage");

    // No banner should be visible yet.
    assert!(
        app.page.find_element(".offline-banner").await.is_err(),
        "offline-banner must not be present before server is killed"
    );

    app.stop_server();

    // The polling loop runs every 2 s; allow up to 3 cycles + headroom.
    app.wait_for(".offline-banner", Duration::from_secs(12))
        .await
        .expect("offline-banner must appear after server is killed");

    // Data must not have been cleared.
    let items = app
        .page
        .find_elements(".list-item")
        .await
        .unwrap_or_default();
    assert!(
        !items.is_empty(),
        "list must still show items during outage (data must not be wiped)"
    );

    let first_text = items[0]
        .inner_text()
        .await
        .unwrap_or_default()
        .unwrap_or_default();
    assert!(
        first_text.contains("Offline test note"),
        "note title must be preserved during outage; got: {:?}",
        first_text
    );
}

/// Verify that the banner disappears automatically when the server comes back
/// and that data is still consistent after reconnect.
#[tokio::test]
async fn banner_clears_and_data_intact_after_reconnect() {
    let mut app = TestApp::spawn().await.expect("spawn");

    app.api_post(
        "create_note",
        json!({
            "title": "Recovery note",
            "body":  "Reconnect test",
            "folder_id": null,
            "space_id": 1,
        }),
    )
    .await
    .expect("create_note");

    app.wait_until(
        || async {
            let items = app
                .page
                .find_elements(".list-item")
                .await
                .unwrap_or_default();
            Ok(if items.is_empty() { None } else { Some(()) })
        },
        Duration::from_secs(8),
    )
    .await
    .expect("note in list before outage");

    app.stop_server();
    app.wait_for(".offline-banner", Duration::from_secs(12))
        .await
        .expect("offline-banner appears after server killed");

    app.restart_server().await.expect("restart server");

    // Banner must disappear within the next few poll cycles.
    app.wait_until(
        || async {
            Ok(
                if app.page.find_element(".offline-banner").await.is_err() {
                    Some(())
                } else {
                    None
                },
            )
        },
        Duration::from_secs(12),
    )
    .await
    .expect("offline-banner must disappear after server recovers");

    // Note should still be visible.
    let items = app
        .page
        .find_elements(".list-item")
        .await
        .unwrap_or_default();
    assert!(
        !items.is_empty(),
        "note must still be visible after reconnect"
    );
}
