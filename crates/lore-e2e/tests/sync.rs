//! Cross-client refresh regression. The frontend polls `get_note` for the
//! currently-open note on every revision tick and, if the server's
//! `updated_at` advanced past what we last loaded, pushes the new content
//! into the editor via `smartReplace`. Tests here pin that path so
//! changes to polling logic or `smartReplace` don't silently break it.

use std::time::Duration;

use lore_e2e::TestApp;
use serde_json::json;

#[tokio::test]
async fn external_edit_refreshes_open_note_via_smart_replace() {
    let app = TestApp::spawn().await.expect("spawn");

    // Seed: a note with recognisable body.
    let note_id = app
        .api_post(
            "create_note",
            json!({
                "title": "Sync Test",
                "body": "Initial body content",
                "folder_id": null,
                "space_id": 1,
            }),
        )
        .await
        .expect("create_note")
        .as_i64()
        .expect("note id");

    // Wait for the polling loop to surface the note in the sidebar.
    app.wait_until(
        || async {
            let items = app
                .page
                .find_elements(".list-item")
                .await
                .unwrap_or_default();
            Ok(if items.is_empty() { None } else { Some(()) })
        },
        Duration::from_secs(5),
    )
    .await
    .expect("note shows up in sidebar");

    // Open it.
    app.click(".list-item").await.expect("click list-item");

    // Editor must load the initial body before we test the refresh.
    app.wait_until(
        || async {
            let t = app.text(".ProseMirror").await.unwrap_or_default();
            Ok(if t.contains("Initial body content") {
                Some(())
            } else {
                None
            })
        },
        Duration::from_secs(5),
    )
    .await
    .expect("editor shows initial body");

    // External edit — pretend a second client wrote to the server.
    app.api_post(
        "update_note",
        json!({
            "note_id": note_id,
            "title": "Sync Test",
            "body": "Edited externally — agent says hi",
        }),
    )
    .await
    .expect("update_note");

    // Within the polling window the editor should pick up the new body.
    // 10 s leaves headroom: poll interval is 2 s, plus the smartReplace
    // dispatch and the next textContent read.
    app.wait_until(
        || async {
            let t = app.text(".ProseMirror").await.unwrap_or_default();
            Ok(if t.contains("Edited externally — agent says hi") {
                Some(())
            } else {
                None
            })
        },
        Duration::from_secs(10),
    )
    .await
    .expect("editor receives smartReplace within poll window");

    // Old text must be gone — guards against an additive bug that
    // appends new content instead of replacing.
    let final_text = app.text(".ProseMirror").await.unwrap();
    assert!(
        !final_text.contains("Initial body content"),
        "old body should be replaced, not appended; got: {:?}",
        final_text
    );
}
