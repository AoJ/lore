/// Snapshot tests for error envelope and export formats.
/// Run with: cargo test -p lore-core --test snapshots -- --nocapture
/// Review and accept with: cargo insta review
use insta::{assert_json_snapshot, assert_snapshot};
use lore_core::error::{BackendError, ErrorCode};

// --- error.rs envelope snapshots ---

#[test]
fn snapshot_error_route_not_found() {
    let err = BackendError::route_not_found("GET /api/nonexistent");
    assert_json_snapshot!(err);
}

#[test]
fn snapshot_error_not_found() {
    let err = BackendError::not_found("page id=999 not found");
    assert_json_snapshot!(err);
}

#[test]
fn snapshot_error_invalid_input() {
    let err = BackendError::invalid_input("invalid query: missing required field 'space_id'");
    assert_json_snapshot!(err);
}

#[test]
fn snapshot_error_internal() {
    let err = BackendError::internal("database connection lost");
    assert_json_snapshot!(err);
}

// --- Format::mime() snapshots ---

#[test]
fn snapshot_format_mime_markdown() {
    let mime = lore_core::export::Format::Markdown.mime();
    assert_snapshot!(mime, @"text/markdown; charset=utf-8");
}

#[test]
fn snapshot_format_mime_json() {
    let mime = lore_core::export::Format::Json.mime();
    assert_snapshot!(mime, @"application/json; charset=utf-8");
}

#[test]
fn snapshot_format_mime_html() {
    let mime = lore_core::export::Format::Html.mime();
    assert_snapshot!(mime, @"text/html; charset=utf-8");
}

// TODO: Add full export snapshot when DB fixture proves stable.
// The fixture would:
// 1. `db::open` a tempfile
// 2. seed one space + one note + one page deterministically
// 3. call `export_snapshot` and snapshot the `Exported` bytes
// Defer if fiddly; ship error + mime snapshots first.
