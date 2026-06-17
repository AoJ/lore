//! Note CRUD through the UI + sidebar polling refresh.

use std::time::Duration;

use lore_e2e::TestApp;

#[tokio::test]
async fn clicking_plus_creates_a_note_and_list_refreshes() {
    let app = TestApp::spawn().await.expect("spawn app");

    // Make sure the empty state is showing before the click.
    app.wait_for_default(".empty-state").await.unwrap();

    // Click the "+" in the notes list header.
    app.click(".list-add-btn").await.expect("click + button");

    // A list-item should appear within the polling interval (2 s).
    app.wait_for(".list-item", Duration::from_secs(5))
        .await
        .expect("note appears in list");

    // And the sidebar's empty-state should be gone. The list-item and the
    // empty-state removal land on separate renders, so poll rather than
    // asserting on the first frame after the row appears.
    app.wait_until(
        || async { Ok(app.page.find_element(".empty-state").await.err().map(|_| ())) },
        Duration::from_secs(3),
    )
    .await
    .expect("empty-state should be gone after note creation");
}

#[tokio::test]
async fn api_seeded_notes_render_in_sidebar_after_poll_tick() {
    let app = TestApp::spawn().await.expect("spawn app");

    // Seed three notes via the API.
    for title in ["First", "Second", "Third"] {
        app.api_post(
            "create_note",
            serde_json::json!({
                "title": title,
                "body": "",
                "folder_id": null,
                "space_id": 1,
            }),
        )
        .await
        .expect("create_note");
    }

    // Poll the DOM until exactly 3 list-items show up. The web client
    // refreshes on every 2 s revision check, so 5 s is comfortable.
    let count = app
        .wait_until(
            || async {
                let els = app
                    .page
                    .find_elements(".list-item")
                    .await
                    .unwrap_or_default();
                if els.len() == 3 {
                    Ok(Some(els.len()))
                } else {
                    Ok(None)
                }
            },
            Duration::from_secs(5),
        )
        .await
        .expect("3 list items visible");
    assert_eq!(count, 3);
}
