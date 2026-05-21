//! Thin UI-side data helpers. All DB access now goes through `crate::backend`;
//! this file owns:
//!   1. DB path resolution (`db_path`, used by the boot path)
//!   2. UI-only formatting helpers (size, ext, mime)
//!   3. OS integrations (open_in_browser)
//!   4. View-model adapters that fetch via the backend and format for render
//!      (e.g. `PageDetailView`).

use anyhow::Result;

/// Resolve database path from environment or system default. Desktop-only —
/// the web build talks to a remote `lore-server` and never opens a DB itself.
#[cfg(feature = "desktop")]
pub fn db_path() -> std::path::PathBuf {
    use std::path::PathBuf;

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

// ---- Page detail view-model ----

/// Display-ready web page record. Wraps `lore_core::db::WebPageDetail`
/// with UI concerns: NULL-title fallback, byte-size formatting,
/// base64-encoded screenshot for inline rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct PageDetailView {
    pub url: String,
    /// Page-level title (latest known). Snapshot views may override with
    /// the title captured at that snapshot's fetch time.
    pub title: String,
    pub domain: String,
    pub category: String,
    pub status: String,
    /// First-archive date — kept for reference but UI shows
    /// `last_fetched_at_display` in the header instead.
    pub created_at: String,
    /// Date/time of the most recent snapshot, formatted for display
    /// (`YYYY-MM-DD HH:MM`). `None` if the page has never been archived.
    pub last_fetched_at_display: Option<String>,
    /// Aggregate size across all snapshots (text + html + screenshot + title),
    /// formatted. Reflects actual DB cost, not just the latest version.
    pub total_size_display: Option<String>,
    pub last_error: Option<String>,
    pub has_snapshot: bool,
    pub plain_text_preview: Option<String>,
    /// Down-scaled thumbnail (PNG, base64). Default view in the detail
    /// panel. `None` for snapshots without screenshots (HTTP fallback) and
    /// for legacy snapshots that pre-date migration 0010 (in which case
    /// the UI fetches the full screenshot lazily as a fallback).
    pub screenshot_thumb_base64: Option<String>,
    /// True if the snapshot has a full-size screenshot the UI can fetch
    /// lazily on click. Drives "click to enlarge" affordance.
    pub has_full_screenshot: bool,
    /// Cleaned `<article>` HTML from m0011 readability extraction. When
    /// present, the detail view renders it via an `<iframe srcdoc>` as
    /// the default "Article" tab. `None` for legacy snapshots or pages
    /// that have no extractable article (dashboards, login walls).
    pub readability_html: Option<String>,
}

pub async fn get_page_view(id: i64) -> Result<PageDetailView> {
    let p = crate::backend::current().get_page(id).await?;
    let (plain_text_preview, screenshot_thumb_base64, has_full_screenshot, readability_html, has_snapshot) =
        match p.snapshot {
            Some(s) => {
                let b64 = s.screenshot_thumb.as_ref().map(|bytes| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                });
                (
                    s.plain_text_preview,
                    b64,
                    s.has_full_screenshot,
                    s.readability_html,
                    true,
                )
            }
            None => (None, None, false, None, false),
        };
    let total_size_display = if p.total_size_bytes > 0 {
        Some(format_size_short(p.total_size_bytes))
    } else {
        None
    };
    let last_fetched_at_display = p.last_fetched_at.as_deref().map(format_iso_to_display);
    Ok(PageDetailView {
        url: p.url,
        title: p
            .title
            .unwrap_or_else(|| crate::texts::NO_TITLE.to_string()),
        domain: p.domain,
        category: p.category,
        status: p.status,
        created_at: p.created_at,
        last_fetched_at_display,
        total_size_display,
        last_error: p.last_error,
        has_snapshot,
        plain_text_preview,
        screenshot_thumb_base64,
        has_full_screenshot,
        readability_html,
    })
}

/// Trim an ISO timestamp like `2026-05-21T14:30:00.123Z` down to a
/// presentation-friendly `YYYY-MM-DD HH:MM`. Used by the page-detail
/// header and the version-picker rows so they share the same format.
pub fn format_iso_to_display(iso: &str) -> String {
    iso.chars().take(16).collect::<String>().replace('T', " ")
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

// ---- Snapshot version view-model ----

/// Lightweight parse of `change_summary` JSON. We control the producer
/// (`insert_snapshot` in `lore-core`), so the format is fixed:
/// `{"title_changed":bool,"size_delta_pct":i32,"content_same":bool}`.
/// Avoids pulling `serde_json` into the UI for three fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeSummary {
    pub title_changed: bool,
    pub size_delta_pct: i32,
    pub content_same: bool,
}

pub fn parse_change_summary(json: &str) -> Option<ChangeSummary> {
    let extract_bool = |key: &str| -> Option<bool> {
        let needle = format!("\"{}\":", key);
        let start = json.find(&needle)? + needle.len();
        let rest = json[start..].trim_start();
        if rest.starts_with("true") {
            Some(true)
        } else if rest.starts_with("false") {
            Some(false)
        } else {
            None
        }
    };
    let extract_i32 = |key: &str| -> Option<i32> {
        let needle = format!("\"{}\":", key);
        let start = json.find(&needle)? + needle.len();
        let rest = &json[start..];
        let end = rest.find(|c: char| c != '-' && !c.is_ascii_digit())?;
        rest[..end].parse().ok()
    };
    Some(ChangeSummary {
        title_changed: extract_bool("title_changed")?,
        size_delta_pct: extract_i32("size_delta_pct")?,
        content_same: extract_bool("content_same")?,
    })
}

/// Display-ready snapshot version row for the "Versions" panel.
#[derive(Clone, Debug, PartialEq)]
pub struct VersionView {
    pub id: i64,
    pub version: i64,
    /// "2026-05-21 14:30" — trimmed from ISO timestamp for display.
    pub fetched_at_display: String,
    /// Original ISO string, used for export filename and ordering.
    pub fetched_at_iso: String,
    pub title_display: String,
    pub size_display: String,
    pub has_screenshot: bool,
    /// Empty on version 1 (no diff base).
    pub summary: Option<ChangeSummary>,
}

pub fn snapshot_meta_to_view(
    meta: &lore_core::db::SnapshotMeta,
    page_title_fallback: &str,
) -> VersionView {
    VersionView {
        id: meta.id,
        version: meta.version,
        fetched_at_display: format_iso_to_display(&meta.fetched_at),
        fetched_at_iso: meta.fetched_at.clone(),
        title_display: meta
            .title
            .clone()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| page_title_fallback.to_string()),
        size_display: format_size_short(meta.size_bytes),
        has_screenshot: meta.has_screenshot,
        summary: meta
            .change_summary
            .as_deref()
            .and_then(parse_change_summary),
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

/// Open `url` in the host OS's default browser. Desktop-only — on web
/// builds we use a normal `<a href target="_blank">` anchor instead (the
/// browser already owns navigation), so this function isn't compiled.
#[cfg(feature = "desktop")]
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
