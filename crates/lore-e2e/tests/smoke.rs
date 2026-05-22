//! Smoke tests — does the stack boot and render at all?
//!
//! These run first; if they fail, every other e2e test fails too. Keep
//! them small + flake-free.

use std::time::Duration;

use lore_e2e::TestApp;

#[tokio::test]
async fn server_boots_wasm_mounts_sidebar_renders() {
    let app = TestApp::spawn().await.expect("spawn app");

    // The sidebar should mount within the WASM boot window.
    app.wait_for_default(".sidebar")
        .await
        .expect("sidebar element");

    // The default seeded space ("Personal") shows up once the spaces
    // signal is populated from the backend. The element mounts immediately
    // with a fallback "Space" label, so polling on the element alone races
    // the initial fetch — wait until the label flips to the real name.
    app.wait_until(
        || async {
            let txt = app.text(".space-name").await.ok().unwrap_or_default();
            if txt == "Personal" { Ok(Some(())) } else { Ok(None) }
        },
        Duration::from_secs(5),
    )
    .await
    .expect("default space name resolves to 'Personal' after initial fetch");
}

#[tokio::test]
async fn empty_notes_list_shows_empty_state() {
    let app = TestApp::spawn().await.expect("spawn app");

    let empty = app
        .wait_for(".empty-state", Duration::from_secs(5))
        .await
        .expect("empty-state visible");
    let text = empty.inner_text().await.unwrap().unwrap_or_default();
    assert!(
        text.contains("No notes yet"),
        "empty state copy, got: {:?}",
        text
    );
}

#[tokio::test]
async fn list_spaces_api_round_trip() {
    let app = TestApp::spawn().await.expect("spawn app");

    let spaces = app
        .api_post("list_spaces", serde_json::json!({}))
        .await
        .expect("list_spaces");

    let arr = spaces.as_array().expect("array response");
    assert_eq!(arr.len(), 1, "exactly one seeded space");
    assert_eq!(arr[0]["name"], "Personal");
}
