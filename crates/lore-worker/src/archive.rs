use anyhow::{Context, Result};
use rusqlite::Connection;
use url::Url;

use crate::render::{self, Renderer};
use lore_core::{db, rules};

/// Outcome of archiving one URL. The worker prints a summary that
/// distinguishes the three states so the user can tell "everything went
/// well", "data is in but Chrome failed and we used HTTP" and "nothing got
/// saved" apart — previously they all rolled up into a misleading
/// `1 ok, 0 failed` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveOutcome {
    /// Archived through the full Chrome pipeline.
    Ok,
    /// Archived through HTTP fallback after Chrome failed. Data was stored,
    /// but the snapshot is missing JS-rendered content and a screenshot.
    Degraded,
    /// Nothing was stored; page status is `failed`.
    Failed,
}

pub fn archive_url(conn: &Connection, url_str: &str) -> Result<ArchiveOutcome> {
    let parsed = Url::parse(url_str).with_context(|| format!("invalid URL: {}", url_str))?;
    let domain = parsed.host_str().unwrap_or("unknown").to_string();
    let normalized = rules::normalize_url(&parsed);

    let page_id = db::ensure_page(conn, url_str, &normalized, None, &domain, "archive")?;

    let renderer = render::create_renderer();
    fetch_and_store(conn, page_id, url_str, renderer.as_ref())
}

pub fn archive_queued(conn: &Connection, limit: usize) -> Result<ArchiveSummary> {
    // Reset stuck fetching pages back to queued (from previous crashed runs)
    conn.execute(
        "UPDATE web_page SET status = 'queued', last_error = 'reset: previous fetch interrupted' WHERE status = 'fetching'",
        [],
    )?;

    let mut stmt = conn.prepare(
        "SELECT id, url FROM web_page WHERE status = 'queued' AND category = 'archive' LIMIT ?1",
    )?;

    let pages: Vec<(i64, String)> = stmt
        .query_map([limit as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if pages.is_empty() {
        eprintln!("No queued pages to archive");
        return Ok(ArchiveSummary::default());
    }

    let total = pages.len();
    eprintln!("Archiving {} pages...", total);

    let renderer = render::create_renderer();
    let mut summary = ArchiveSummary::default();

    for (i, (page_id, url)) in pages.iter().enumerate() {
        eprint!("[{}/{}] ", i + 1, total);
        match fetch_and_store(conn, *page_id, url, renderer.as_ref()) {
            Ok(ArchiveOutcome::Ok) => summary.ok += 1,
            Ok(ArchiveOutcome::Degraded) => summary.degraded += 1,
            Ok(ArchiveOutcome::Failed) | Err(_) => summary.failed += 1,
        }
    }

    if summary.degraded > 0 {
        eprintln!(
            "Done: {} ok, {} degraded (Chrome failed, used HTTP fallback), {} failed",
            summary.ok, summary.degraded, summary.failed
        );
    } else {
        eprintln!("Done: {} ok, {} failed", summary.ok, summary.failed);
    }
    Ok(summary)
}

/// Aggregate counters returned from `archive_queued`. The worker `main`
/// uses these to set a non-zero exit code when something needs attention.
#[derive(Debug, Default, Clone, Copy)]
pub struct ArchiveSummary {
    pub ok: u32,
    pub degraded: u32,
    pub failed: u32,
}

fn fetch_and_store(
    conn: &Connection,
    page_id: i64,
    url: &str,
    renderer: &dyn Renderer,
) -> Result<ArchiveOutcome> {
    eprintln!("Fetching {}...", url);
    db::update_status(conn, page_id, "fetching")?;

    match renderer.render(url) {
        Ok(rendered) => {
            let degraded = rendered.via_fallback;
            if let Some(ref title) = rendered.title {
                conn.execute(
                    "UPDATE web_page SET title = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2 AND (title IS NULL OR title = '')",
                    rusqlite::params![title, page_id],
                )?;
            }

            db::insert_snapshot(
                conn,
                page_id,
                &rendered.html,
                &rendered.plain_text,
                rendered.screenshot.as_deref(),
                rendered.screenshot_thumb.as_deref(),
            )?;

            db::update_status(conn, page_id, "archived")?;
            if degraded {
                eprintln!(
                    "Degraded: {} ({} chars, via HTTP fallback)",
                    url,
                    rendered.plain_text.len()
                );
                Ok(ArchiveOutcome::Degraded)
            } else {
                eprintln!("Archived: {} ({} chars)", url, rendered.plain_text.len());
                Ok(ArchiveOutcome::Ok)
            }
        }
        Err(e) => {
            let error_msg = format!("{:#}", e);
            db::update_status_with_error(conn, page_id, "failed", &error_msg)?;
            eprintln!("Failed: {}: {}", url, e);
            Ok(ArchiveOutcome::Failed)
        }
    }
}
