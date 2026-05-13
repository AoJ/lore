//! Thin UI-side data helpers. Pure DB queries live in `lore_core::db`;
//! this file only owns:
//!   1. connection bootstrap (db_path + open_db wrapper)
//!   2. UI-only formatting helpers (size, ext, mime)
//!   3. OS integrations (open_in_browser)
//!   4. View-model adapters that turn raw core records into display-ready
//!      strings + base64 (e.g. PageDetailView).

use anyhow::Result;
use std::path::PathBuf;

/// Resolve database path from environment or system default.
pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("LORE_DB") {
        return PathBuf::from(p);
    }
    if let Some(dir) = dirs::data_local_dir() {
        let p = dir.join("lore");
        std::fs::create_dir_all(&p).ok();
        return p.join("lore.db");
    }
    PathBuf::from("lore.db")
}

/// Open a connection for runtime queries. Skips migration runner / seed —
/// `main::app()` calls `lore_core::db::open` once at startup to bootstrap.
pub fn open_db() -> Result<rusqlite::Connection> {
    lore_core::db::open_existing(&db_path())
}

pub fn get_revision() -> i64 {
    open_db()
        .ok()
        .and_then(|conn| lore_core::db::get_revision(&conn).ok())
        .unwrap_or(0)
}

/// Read PRAGMA user_version directly (raw connection, no migrations, no
/// refuse-on-newer). Used by the polling loop to detect a schema upgrade
/// happening underneath us — going through `open_db()` would fail outright
/// if another process has bumped the DB past `EXPECTED_VERSION`.
pub fn db_schema_version() -> u32 {
    rusqlite::Connection::open(db_path())
        .ok()
        .and_then(|c| {
            c.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .ok()
        })
        .unwrap_or(0)
}

// ---- Page detail view-model ----

/// Display-ready web page record. Wraps `lore_core::db::WebPageDetail`
/// with UI concerns: NULL-title fallback, byte-size formatting,
/// base64-encoded screenshot for inline rendering.
#[derive(Clone, Debug)]
pub struct PageDetailView {
    pub url: String,
    pub title: String,
    pub domain: String,
    pub category: String,
    pub status: String,
    pub created_at: String,
    pub last_error: Option<String>,
    pub has_snapshot: bool,
    pub content_size: Option<String>,
    pub plain_text_preview: Option<String>,
    pub screenshot_base64: Option<String>,
}

pub fn get_page_view(id: i64) -> Result<PageDetailView> {
    let conn = open_db()?;
    let p = lore_core::db::get_page(&conn, id)?;
    let (content_size, plain_text_preview, screenshot_base64, has_snapshot) = match p.snapshot {
        Some(s) => {
            let b64 = s.screenshot.as_ref().map(|bytes| {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(bytes)
            });
            (
                Some(format_size_short(s.size_bytes)),
                s.plain_text_preview,
                b64,
                true,
            )
        }
        None => (None, None, None, false),
    };
    Ok(PageDetailView {
        url: p.url,
        title: p
            .title
            .unwrap_or_else(|| crate::texts::NO_TITLE.to_string()),
        domain: p.domain,
        category: p.category,
        status: p.status,
        created_at: p.created_at,
        last_error: p.last_error,
        has_snapshot,
        content_size,
        plain_text_preview,
        screenshot_base64,
    })
}

// ---- File / size / mime helpers ----

/// Formats sizes like "1.2 MB" — used for file lists.
pub fn format_file_size(bytes: i64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Compact size formatter for page snapshot metadata (KB/MB/B, one decimal).
fn format_size_short(bytes: i64) -> String {
    if bytes > 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes > 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Extract the uppercase file extension, e.g. "PDF", "PNG". Returns "FILE" if none.
pub fn file_extension(name: &str) -> String {
    std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| "FILE".into())
        .to_string()
}

/// Best-effort MIME type from file extension.
pub fn mime_from_extension(name: &str) -> String {
    let ext = std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "doc" => "application/msword",
        "xls" => "application/vnd.ms-excel",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "json" => "application/json",
        "xml" => "application/xml",
        "csv" => "text/csv",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
    .to_string()
}

pub fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_file_size_bytes_under_1k() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(999), "999 B");
    }

    #[test]
    fn format_file_size_kilobytes() {
        assert_eq!(format_file_size(1_000), "1.0 KB");
        assert_eq!(format_file_size(1_500), "1.5 KB");
        assert_eq!(format_file_size(999_999), "1000.0 KB");
    }

    #[test]
    fn format_file_size_megabytes() {
        assert_eq!(format_file_size(1_000_000), "1.0 MB");
        assert_eq!(format_file_size(12_300_000), "12.3 MB");
    }

    #[test]
    fn format_file_size_gigabytes() {
        assert_eq!(format_file_size(1_000_000_000), "1.0 GB");
        assert_eq!(format_file_size(2_500_000_000), "2.5 GB");
    }

    #[test]
    fn format_size_short_thresholds_match_get_page_view() {
        // The page-detail formatter uses strict `>` (not `>=`) so 1000 stays as B.
        assert_eq!(format_size_short(500), "500 B");
        assert_eq!(format_size_short(1_500), "1.5 KB");
        assert_eq!(format_size_short(1_500_000), "1.5 MB");
    }

    #[test]
    fn file_extension_uppercases_and_strips_dot() {
        assert_eq!(file_extension("doc.pdf"), "PDF");
        assert_eq!(file_extension("photo.JPG"), "JPG");
        assert_eq!(file_extension("a/b/c.tar.gz"), "GZ");
    }

    #[test]
    fn file_extension_no_extension_returns_fallback() {
        assert_eq!(file_extension("README"), "FILE");
        assert_eq!(file_extension(""), "FILE");
    }

    #[test]
    fn mime_from_extension_known_types() {
        assert_eq!(mime_from_extension("doc.pdf"), "application/pdf");
        assert_eq!(mime_from_extension("img.PNG"), "image/png");
        assert_eq!(mime_from_extension("img.jpeg"), "image/jpeg");
        assert_eq!(mime_from_extension("page.html"), "text/html");
        assert_eq!(mime_from_extension("data.json"), "application/json");
    }

    #[test]
    fn mime_from_extension_unknown_falls_back_to_octet_stream() {
        assert_eq!(mime_from_extension("noext"), "application/octet-stream");
        assert_eq!(mime_from_extension("foo.xyzzy"), "application/octet-stream");
    }
}
