//! Tests for backend-unavailability behavior.
//!
//! Scenario: the lore-server process becomes unreachable mid-session. The
//! frontend must:
//!   1. show a prominent `.offline-banner` (not an alert)
//!   2. keep the last-known data visible — lists must not be cleared
//!   3. lock sidebar + list panel so the user can't navigate away from the
//!      current note (`.app-layout` gets class `offline`)
//!   4. queue keystrokes typed while offline and flush them on reconnect
//!   5. clear the banner automatically once the server recovers

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
            Ok(if app.page.find_element(".offline-banner").await.is_err() {
                Some(())
            } else {
                None
            })
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

/// While offline the `.app-layout` must carry the `offline` CSS class, which
/// locks sidebar and list panel via `pointer-events: none`. The class must be
/// absent when the server is up.
#[tokio::test]
async fn offline_class_toggles_with_connectivity() {
    let mut app = TestApp::spawn().await.expect("spawn");
    app.wait_for_default(".app-layout")
        .await
        .expect("app-layout mounted");

    // Helper: evaluate whether .app-layout has the 'offline' class.
    let has_offline_class = |page: chromiumoxide::Page| async move {
        let js = "document.querySelector('.app-layout')?.classList.contains('offline') ?? false";
        page.evaluate(js)
            .await
            .ok()
            .and_then(|v| v.into_value::<bool>().ok())
            .unwrap_or(false)
    };

    // Initially online — class must be absent.
    assert!(
        !has_offline_class(app.page.clone()).await,
        "app-layout must not have 'offline' class when backend is reachable"
    );

    app.stop_server();

    // Wait up to 12 s for the class to appear (poll interval is 2 s).
    app.wait_until(
        || async {
            Ok(if has_offline_class(app.page.clone()).await {
                Some(())
            } else {
                None
            })
        },
        Duration::from_secs(12),
    )
    .await
    .expect("'offline' class must appear on app-layout after server dies");

    app.restart_server().await.expect("restart server");

    // Class must clear within the next poll cycles.
    app.wait_until(
        || async {
            Ok(if !has_offline_class(app.page.clone()).await {
                Some(())
            } else {
                None
            })
        },
        Duration::from_secs(12),
    )
    .await
    .expect("'offline' class must be removed after server recovers");
}

/// Keystrokes typed while offline are queued and flushed to the backend on
/// reconnect. This test drives the note editor via the Milkdown bridge
/// textarea (the same path real keystrokes use).
#[tokio::test]
async fn offline_keystrokes_flushed_on_reconnect() {
    let mut app = TestApp::spawn().await.expect("spawn");

    // Seed a note and open it.
    let note_id = app
        .api_post(
            "create_note",
            json!({
                "title": "Flush test",
                "body":  "Initial body",
                "folder_id": null,
                "space_id": 1,
            }),
        )
        .await
        .expect("create_note")
        .as_i64()
        .expect("note id");

    // Wait for the note to appear in the list, then open it.
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
    .expect("note appears in list");

    app.click(".list-item").await.expect("open note");
    // Wait for the bridge textarea to mount (it's inside the note editor).
    app.wait_for("#milkdown-bridge", Duration::from_secs(5))
        .await
        .expect("milkdown bridge mounted");

    // Helper: trigger the milkdown bridge with new content, simulating typing.
    let trigger_bridge = |page: &chromiumoxide::Page, content: &str| {
        let js = format!(
            r#"(function() {{
                var ta = document.getElementById('milkdown-bridge');
                if (!ta) return false;
                ta.value = {content};
                ta.dispatchEvent(new InputEvent('input', {{ bubbles: true }}));
                return true;
            }})()"#,
            content = serde_json::to_string(content).unwrap(),
        );
        let page = page.clone();
        async move {
            page.evaluate(js.as_str())
                .await
                .ok()
                .and_then(|v| v.into_value::<bool>().ok())
                .unwrap_or(false)
        }
    };

    // Trigger a save while online to confirm the bridge works.
    let ok = trigger_bridge(&app.page, "Flush test\nOnline save").await;
    assert!(ok, "bridge trigger returned false");
    // Give the async save task a moment to complete.
    tokio::time::sleep(Duration::from_millis(300)).await;

    app.stop_server();

    // Wait for the offline class to appear.
    app.wait_until(
        || async {
            let js =
                "document.querySelector('.app-layout')?.classList.contains('offline') ?? false";
            let offline: bool = app
                .page
                .evaluate(js)
                .await
                .ok()
                .and_then(|v| v.into_value().ok())
                .unwrap_or(false);
            Ok(if offline { Some(()) } else { None })
        },
        Duration::from_secs(12),
    )
    .await
    .expect("offline class appears");

    // Type new content while offline — this queues a pending save.
    let ok = trigger_bridge(&app.page, "Flush test\nQueued while offline").await;
    assert!(ok, "bridge trigger returned false while offline");
    tokio::time::sleep(Duration::from_millis(200)).await;

    app.restart_server().await.expect("restart server");

    // Wait for reconnect (banner disappears = backend_online flipped back).
    app.wait_until(
        || async {
            Ok(if app.page.find_element(".offline-banner").await.is_err() {
                Some(())
            } else {
                None
            })
        },
        Duration::from_secs(12),
    )
    .await
    .expect("reconnect detected");

    // Give the flush + server-side write a moment to land.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify via the API that the pending content was actually persisted.
    let note = app
        .api_post("get_note", json!({ "note_id": note_id }))
        .await
        .expect("get_note after reconnect");

    let body = note["body"].as_str().unwrap_or("");
    assert!(
        body.contains("Queued while offline"),
        "pending save must have been flushed to the server; body was: {:?}",
        body
    );
}
