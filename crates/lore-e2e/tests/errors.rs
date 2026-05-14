//! Error wire format. Every server failure must emit a JSON
//! `{ "code": ..., "message": ... }` so the frontend can branch on the
//! coarse category (`not_found` / `route_not_found` / `invalid_input` /
//! `internal`). Three of the four paths are exercised here — `internal`
//! is hard to trigger from outside without DB tampering and is left to
//! manual testing.

use lore_e2e::TestApp;
use serde_json::Value;

#[tokio::test]
async fn unknown_api_endpoint_returns_route_not_found() {
    let app = TestApp::spawn().await.expect("spawn");

    let (status, body) = app
        .fetch_raw("POST", "/api/does_not_exist", Some("{}"))
        .await
        .expect("fetch");
    assert_eq!(status, 404);

    let err: Value = serde_json::from_str(&body).expect("BackendError JSON");
    assert_eq!(err["code"], "route_not_found", "body: {}", body);
    assert!(
        err["message"].as_str().is_some_and(|s| !s.is_empty()),
        "message must be non-empty"
    );
}

#[tokio::test]
async fn missing_note_returns_not_found() {
    let app = TestApp::spawn().await.expect("spawn");

    // Empty DB → note id 99999 doesn't exist.
    let (status, body) = app
        .fetch_raw("POST", "/api/get_note", Some(r#"{"note_id": 99999}"#))
        .await
        .expect("fetch");
    assert_eq!(status, 404);

    let err: Value = serde_json::from_str(&body).expect("BackendError JSON");
    assert_eq!(err["code"], "not_found", "body: {}", body);
}

#[tokio::test]
async fn malformed_body_returns_invalid_input() {
    let app = TestApp::spawn().await.expect("spawn");

    // `space_id` declared as `i64` server-side; sending a string fails
    // the `JsonReq` extractor, which our custom rejection converts to
    // `invalid_input` (HTTP 400).
    let (status, body) = app
        .fetch_raw(
            "POST",
            "/api/list_folders",
            Some(r#"{"space_id":"not-an-int"}"#),
        )
        .await
        .expect("fetch");
    assert_eq!(status, 400);

    let err: Value = serde_json::from_str(&body).expect("BackendError JSON");
    assert_eq!(err["code"], "invalid_input", "body: {}", body);
}
