//! Sidebar navigation: clicking section items in the left panel switches
//! which list component renders in the middle panel. The frontend uses
//! `Section` enum + `match` in `AppLayout` to pick a component, so this
//! pins the wiring end-to-end (click handler → state mutation → match
//! arm → component swap).

use std::time::Duration;

use lore_e2e::TestApp;

#[tokio::test]
async fn clicking_sidebar_sections_swaps_the_list_panel() {
    let app = TestApp::spawn().await.expect("spawn");

    // Default section on boot is `AllNotes` — list-title reads "Notes".
    app.wait_until(
        || async {
            let t = app.text(".list-title").await.unwrap_or_default();
            Ok(if t.contains("Notes") { Some(()) } else { None })
        },
        Duration::from_secs(3),
    )
    .await
    .expect("initial section is Notes");

    // Each click should swap the list-title. The titles differ from
    // the sidebar labels in two cases — sidebar says "Webs" but the
    // Pages list-title says "Pages" — which is exactly why the assert
    // checks the list-title content, not just that something changed.
    let cases = [
        ("Webs", "Pages"),
        ("Files", "Files"),
        ("Trash", "Trash"),
        ("Timeline", "Timeline"),
        ("Notes", "Notes"),
    ];

    for (label, expected_title) in cases {
        app.click_text(".sidebar-item", label)
            .await
            .unwrap_or_else(|_| panic!("click sidebar `{}`", label));

        app.wait_until(
            || async {
                let t = app.text(".list-title").await.unwrap_or_default();
                Ok(if t.contains(expected_title) {
                    Some(())
                } else {
                    None
                })
            },
            Duration::from_secs(3),
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "after clicking `{}`, list-title should contain `{}`",
                label, expected_title
            )
        });
    }
}
