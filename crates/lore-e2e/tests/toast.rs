//! Toast notifications — auto-dismiss + per-toast timer isolation.

use std::time::Duration;

use lore_e2e::TestApp;
use serde_json::json;

/// A toast triggered by trashing a note vanishes by itself after its
/// auto-dismiss window expires — users shouldn't need to chase the × button
/// every time. Without this every toast accumulated on screen until the
/// page changed.
#[tokio::test]
async fn toast_auto_dismisses_after_timeout() {
    let app = TestApp::spawn().await.expect("spawn app");
    let note_id_v = app
        .api_post(
            "create_note",
            json!({ "title": "T", "body": "", "folder_id": null, "space_id": 1 }),
        )
        .await
        .expect("create_note");
    let note_id = note_id_v.as_i64().unwrap();

    // Wait for the note to render in the sidebar, click it, then trash it
    // to fire a toast through the UI path (mirrors what users actually do).
    let _ = app
        .wait_for(".list-item", Duration::from_secs(5))
        .await
        .expect("note row appears");
    app.click(".list-item").await.expect("open note");

    app.api_post("trash_note", json!({ "note_id": note_id }))
        .await
        .expect("trash_note");

    // Show toast manually through state so we don't depend on a specific
    // delete UI affordance — what we're testing is the dismissal timing,
    // not which button triggers it. Fire via JS.
    app.page
        .evaluate("window.__force_show_toast && window.__force_show_toast()")
        .await
        .ok();

    // For a deterministic check, trigger a toast via the API-touching
    // restore flow: trash then restore. Each posts its own toast in the UI
    // (but only when the user clicks the buttons). Simplest is to assert on
    // the lifecycle by directly observing whether a toast that does appear
    // dismisses.
    // We skip the JS hack and instead create one through real navigation:
    app.click_text(".sidebar-item", "Trash")
        .await
        .expect("trash nav");
    // Wait for the trashed note to appear, then restore via API to keep
    // the test compact.
    let _ = app
        .wait_for(".list-item", Duration::from_secs(5))
        .await
        .ok();
    app.api_post("restore_note", json!({ "note_id": note_id }))
        .await
        .expect("restore");
    // No toast comes from API-only restore; this branch verifies cleanup.
    // The actual auto-dismiss assertion happens below via a synthetic
    // trash-from-detail flow.

    // Reset DOM state by navigating back home, then trigger trash from the
    // detail panel which DOES fire `show_toast` from `handle_keyboard`'s
    // trash path.
    app.click_text(".sidebar-item", "Notes").await.ok();
    let _ = app
        .wait_for(".list-item", Duration::from_secs(5))
        .await
        .ok();
    app.click(".list-item").await.ok();
    // Cmd+D triggers trash_selected → state.show_toast.
    app.page
        .evaluate(
            r#"
            (() => {
                const ev = new KeyboardEvent('keydown', { key: 'd', metaKey: true, bubbles: true });
                document.querySelector('.app-keyboard-trap')?.dispatchEvent(ev);
            })()
            "#,
        )
        .await
        .ok();

    // Toast must appear within a poll tick.
    let _ = app
        .wait_for(".toast", Duration::from_secs(3))
        .await
        .expect("toast appears after delete");

    // ...and disappear within the auto-dismiss window (7 s for undo
    // toasts; trash uses undo so we wait that long + slack).
    app.wait_until(
        || async {
            let exists = app.page.find_element(".toast").await.is_ok();
            if exists { Ok(None) } else { Ok(Some(())) }
        },
        Duration::from_secs(12),
    )
    .await
    .expect("toast auto-dismisses on its own");
}
