use anyhow::{Context, Result};
use rusqlite::Connection;
use url::Url;

use crate::render::{self, Renderer};
use lore_core::{db, rules};

pub fn archive_url(conn: &Connection, url_str: &str) -> Result<()> {
    let parsed = Url::parse(url_str).with_context(|| format!("invalid URL: {}", url_str))?;
    let domain = parsed.host_str().unwrap_or("unknown").to_string();
    let normalized = rules::normalize_url(&parsed);

    let page_id = db::ensure_page(conn, url_str, &normalized, None, &domain, "archive")?;

    let renderer = render::create_renderer();
    fetch_and_store(conn, page_id, url_str, renderer.as_ref())
}

pub fn archive_queued(conn: &Connection, limit: usize) -> Result<()> {
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
        return Ok(());
    }

    let total = pages.len();
    eprintln!("Archiving {} pages...", total);

    let renderer = render::create_renderer();
    let mut success = 0u32;
    let mut failed = 0u32;

    for (i, (page_id, url)) in pages.iter().enumerate() {
        eprint!("[{}/{}] ", i + 1, total);
        match fetch_and_store(conn, *page_id, url, renderer.as_ref()) {
            Ok(()) => success += 1,
            Err(_) => failed += 1,
        }
    }

    eprintln!("Done: {} ok, {} failed", success, failed);
    Ok(())
}

fn fetch_and_store(
    conn: &Connection,
    page_id: i64,
    url: &str,
    renderer: &dyn Renderer,
) -> Result<()> {
    eprintln!("Fetching {}...", url);
    db::update_status(conn, page_id, "fetching")?;

    match renderer.render(url) {
        Ok(rendered) => {
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
            )?;

            db::update_status(conn, page_id, "archived")?;
            eprintln!("Archived: {} ({} chars)", url, rendered.plain_text.len());
            Ok(())
        }
        Err(e) => {
            db::update_status(conn, page_id, "failed")?;
            eprintln!("Failed: {}: {}", url, e);
            Err(e)
        }
    }
}
