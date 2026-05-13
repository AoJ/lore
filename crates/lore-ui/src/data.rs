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
        .and_then(|c| c.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0)).ok())
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
        title: p.title.unwrap_or_else(|| crate::texts::NO_TITLE.to_string()),
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
        "pdf"  => "application/pdf",
        "png"  => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif"  => "image/gif",
        "webp" => "image/webp",
        "svg"  => "image/svg+xml",
        "avif" => "image/avif",
        "txt"  => "text/plain",
        "md"   => "text/markdown",
        "html" | "htm" => "text/html",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "doc"  => "application/msword",
        "xls"  => "application/vnd.ms-excel",
        "zip"  => "application/zip",
        "gz"   => "application/gzip",
        "json" => "application/json",
        "xml"  => "application/xml",
        "csv"  => "text/csv",
        "mp4"  => "video/mp4",
        "mp3"  => "audio/mpeg",
        _      => "application/octet-stream",
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
