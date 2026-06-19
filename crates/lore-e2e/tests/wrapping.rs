//! Long content must not overflow horizontally. Imported chat exports have code
//! blocks with very long lines (and long unbreakable tokens in prose); without
//! the wrapping CSS they push the note panel past the window edge. The web UI
//! mounts the same component tree + CSS as desktop, so we reproduce it here.

use std::time::Duration;

use lore_e2e::TestApp;

// Measure horizontal overflow of the note scroll-panel and the whole page.
const MEASURE_JS: &str = r#"
(() => {
  const ed = document.querySelector('.milkdown .editor') || document.querySelector('.ProseMirror');
  const cp = document.querySelector('.content-panel-container');
  const de = document.documentElement;
  return {
    editor: ed ? (ed.scrollWidth - ed.clientWidth) : -1,
    panel: cp ? (cp.scrollWidth - cp.clientWidth) : -1,
    page: de.scrollWidth - de.clientWidth,
  };
})()
"#;

#[tokio::test]
async fn long_code_block_and_token_do_not_overflow_horizontally() {
    let app = TestApp::spawn().await.expect("spawn app");

    // A long prose paragraph (wraps at spaces) + a 600-char unbreakable run in a
    // code block and inline — the shapes that pushed the column past the edge.
    let long = "x".repeat(600);
    let prose = "slovo ".repeat(140);
    let body = format!("# Wrap test\n\n{prose}\n\n```\n{long}\n```\n\ninline token: `{long}`\n");
    app.api_post(
        "create_note",
        serde_json::json!({
            "title": "Wrap test",
            "body": body,
            "folder_id": null,
            "space_id": 1,
        }),
    )
    .await
    .expect("create_note");

    // Open the note and wait for the code block to render.
    app.wait_for(".list-item", Duration::from_secs(5))
        .await
        .expect("note in list");
    app.click(".list-item").await.expect("open note");
    app.wait_for(".milkdown pre", Duration::from_secs(5))
        .await
        .expect("code block renders");

    // Measure once the editor is laid out.
    let metrics: serde_json::Value = app
        .wait_until(
            || async {
                if app.page.find_element(".milkdown pre").await.is_err() {
                    return Ok(None);
                }
                let v: serde_json::Value = app.page.evaluate(MEASURE_JS).await?.into_value()?;
                Ok(Some(v))
            },
            Duration::from_secs(5),
        )
        .await
        .expect("measure layout");

    let editor = metrics["editor"].as_f64().unwrap_or(-1.0);
    let panel = metrics["panel"].as_f64().unwrap_or(-1.0);
    let page = metrics["page"].as_f64().unwrap_or(-1.0);

    // Allow ~1px rounding. Prose wraps to the column; the code block scrolls
    // inside itself — nothing should push the editor/panel/page horizontally.
    assert!(
        editor <= 1.0,
        "editor content overflows horizontally by {editor}px (text/code not wrapped or contained)"
    );
    assert!(
        panel <= 1.0,
        "note panel overflows horizontally by {panel}px (long content not contained)"
    );
    assert!(
        page <= 1.0,
        "page overflows horizontally by {page}px (content runs past the window edge)"
    );
}
