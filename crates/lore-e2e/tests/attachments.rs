//! Attachment + file download via the raw octet-stream endpoints. These
//! are the GET routes that the web build's `<a href=... download>` and
//! the in-editor attachment-click handler use; desktop's
//! `rfd::AsyncFileDialog` path is not exercised here.

use lore_e2e::TestApp;
use serde_json::{Value, json};

#[tokio::test]
async fn attachment_raw_returns_bytes_with_attachment_disposition() {
    let app = TestApp::spawn().await.expect("spawn");

    // Need a parent note for the attachment (FK constraint).
    let note_id = app
        .api_post(
            "create_note",
            json!({ "title": "host", "body": "", "folder_id": null, "space_id": 1 }),
        )
        .await
        .expect("create_note")
        .as_i64()
        .expect("note id");

    // base64("hello world") = "aGVsbG8gd29ybGQ="
    let resp = app
        .api_post(
            "insert_attachment",
            json!({
                "note_id": note_id,
                "name": "hello.txt",
                "mime_type": "text/plain",
                "data_b64": "aGVsbG8gd29ybGQ=",
            }),
        )
        .await
        .expect("insert_attachment");
    let att_id = resp
        .get(0)
        .and_then(Value::as_i64)
        .expect("(id, outcome) tuple");

    // Body round-trip
    let path = format!("/api/attachments/{}/raw", att_id);
    let (status, body) = app.fetch_raw("GET", &path, None).await.expect("GET raw");
    assert_eq!(status, 200);
    assert_eq!(body, "hello world");

    // Content-Type + Content-Disposition headers (browser uses these to
    // decide "download vs render"). `fetch_raw` doesn't surface headers,
    // so dip into `evaluate` directly for this assertion.
    let url = format!("{}{}", app.base_url, path);
    let js = format!(
        r#"
        (async () => {{
            const r = await fetch({url});
            return {{
                ct: r.headers.get('content-type'),
                cd: r.headers.get('content-disposition'),
            }};
        }})()
        "#,
        url = serde_json::to_string(&url).unwrap()
    );
    let headers: Value = app
        .page
        .evaluate(js.as_str())
        .await
        .expect("evaluate headers")
        .into_value()
        .expect("parse headers");

    assert_eq!(headers["ct"], "text/plain");
    let cd = headers["cd"].as_str().expect("content-disposition");
    assert!(
        cd.contains("attachment") && cd.contains("hello.txt"),
        "expected attachment disposition with filename, got: {:?}",
        cd
    );
}

#[tokio::test]
async fn file_raw_returns_bytes_with_attachment_disposition() {
    let app = TestApp::spawn().await.expect("spawn");

    // Top-level file (not a note attachment).
    let resp = app
        .api_post(
            "insert_file",
            json!({
                "name": "note.md",
                "mime_type": "text/markdown",
                "data_b64": "IyBoaQo=",  // "# hi\n"
                "space_id": 1,
            }),
        )
        .await
        .expect("insert_file");
    let file_id = resp
        .get(0)
        .and_then(Value::as_i64)
        .expect("(id, outcome) tuple");

    let path = format!("/api/files/{}/raw", file_id);
    let (status, body) = app.fetch_raw("GET", &path, None).await.expect("GET raw");
    assert_eq!(status, 200);
    assert_eq!(body, "# hi\n");

    // Status + the filename header are enough for the regression — the
    // attachment-disposition flag is what makes browsers download
    // instead of navigate. (Asserted above for attachments; same
    // handler shape for files.)
    let url = format!("{}{}", app.base_url, path);
    let js = format!(
        r#"
        (async () => {{
            const r = await fetch({url});
            return r.headers.get('content-disposition');
        }})()
        "#,
        url = serde_json::to_string(&url).unwrap()
    );
    let cd: String = app
        .page
        .evaluate(js.as_str())
        .await
        .expect("evaluate cd")
        .into_value()
        .expect("parse cd");
    assert!(
        cd.contains("attachment") && cd.contains("note.md"),
        "got: {:?}",
        cd
    );
}
