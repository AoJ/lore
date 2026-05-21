use serde::{Deserialize, Serialize};

#[cfg(feature = "sqlite")]
use anyhow::Result;
#[cfg(feature = "sqlite")]
use rusqlite::Connection;
#[cfg(feature = "sqlite")]
use std::collections::HashMap;

#[cfg(feature = "sqlite")]
use crate::rules;

/// Web page summary returned by `list_pages` and search.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebPageRow {
    pub id: i64,
    pub title: Option<String>,
    pub domain: String,
    pub category: String,
    pub status: String,
    /// ISO date truncated to YYYY-MM-DD (matches `NoteRow.updated_at` convention).
    pub created_at: String,
}

/// Full web_page record + latest snapshot, used by detail views.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebPageDetail {
    pub url: String,
    pub title: Option<String>,
    pub domain: String,
    pub category: String,
    pub status: String,
    pub created_at: String,
    pub last_error: Option<String>,
    pub snapshot: Option<WebPageSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebPageSnapshot {
    pub size_bytes: i64,
    pub plain_text_preview: Option<String>,
    /// PNG bytes of the page screenshot. Base64-encoded when serialized to
    /// JSON (HTTP API); native serde formats see raw bytes via the helper.
    #[serde(with = "crate::serde_b64::opt_vec")]
    pub screenshot: Option<Vec<u8>>,
}

/// Per-version metadata for the "Versions" list in page detail.
/// Returned by `list_page_versions` — does **not** include HTML/text/screenshot
/// bodies (those go through `get_page_version` on demand).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: i64,
    pub version: i64,
    pub fetched_at: String,
    /// Title at fetch time. NULL on pre-versioning rows (caller falls back to
    /// `web_page.title`).
    pub title: Option<String>,
    pub size_bytes: i64,
    pub has_screenshot: bool,
    /// SHA256 of plain_text. NULL only on legacy rows where backfill couldn't
    /// run (shouldn't happen in practice — migration 0008 fills these).
    pub content_hash: Option<String>,
    /// JSON `{title_changed, size_delta_pct, content_same}` vs previous version.
    /// NULL on version 1 (no previous to diff).
    pub change_summary: Option<String>,
}

/// Full body of a specific snapshot version — what the detail view actually
/// renders (screenshot, plain_text). HTML stays in DB, fetched separately if
/// the user opens "View raw" (avoids shipping multi-MB pages when only the
/// preview is needed).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotContent {
    pub id: i64,
    pub version: i64,
    pub fetched_at: String,
    pub title: Option<String>,
    pub size_bytes: i64,
    pub plain_text_preview: Option<String>,
    #[serde(with = "crate::serde_b64::opt_vec")]
    pub screenshot: Option<Vec<u8>>,
    pub content_hash: Option<String>,
    pub change_summary: Option<String>,
}

/// Outcome of `archive_url`: returns the row id plus the classifier category
/// (e.g. "archive", "discard") so callers can show appropriate feedback.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchiveOutcome {
    pub id: i64,
    pub category: String,
}

/// Classification rule from DB, ordered by priority descending.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClassificationRule {
    pub pattern: String,
    pub match_type: String,
    pub category: String,
    pub note: String,
}

/// Insert-parameter for `insert_web_page`. Lifetime-bound to caller's
/// strings — only used in the SQLite path; the WASM client never assembles
/// one, it just calls `archive_url`.
#[cfg(feature = "sqlite")]
pub struct NewWebPage<'a> {
    pub url: &'a str,
    pub url_normalized: &'a str,
    pub title: Option<&'a str>,
    pub domain: &'a str,
    pub category: &'a str,
    pub status: &'a str,
    pub source: Option<&'a str>,
    pub space_id: Option<i64>,
}

#[cfg(feature = "sqlite")]
pub fn insert_web_page(conn: &Connection, page: &NewWebPage) -> Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO web_page (url, url_normalized, title, domain, category, status, source, space_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            page.url,
            page.url_normalized,
            page.title,
            page.domain,
            page.category,
            page.status,
            page.source,
            page.space_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[cfg(feature = "sqlite")]
pub fn update_status(conn: &Connection, page_id: i64, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE web_page SET status = ?1, last_error = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        rusqlite::params![status, page_id],
    )?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub fn update_status_with_error(
    conn: &Connection,
    page_id: i64,
    status: &str,
    error: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE web_page SET status = ?1, last_error = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?3",
        rusqlite::params![status, error, page_id],
    )?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub fn insert_snapshot(
    conn: &Connection,
    web_page_id: i64,
    html_content: &str,
    plain_text: &str,
    screenshot: Option<&[u8]>,
) -> Result<i64> {
    use sha2::{Digest, Sha256};

    // Compute current version + load previous snapshot's meta in one go —
    // needed both for the new row and for `change_summary` diffing.
    let previous: Option<(i64, Option<String>, i64, Option<String>)> = conn
        .query_row(
            "SELECT version, title, LENGTH(plain_text), content_hash \
             FROM web_page_snapshot WHERE web_page_id = ?1 \
             ORDER BY version DESC LIMIT 1",
            [web_page_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .ok();

    let version: i64 = previous.as_ref().map(|p| p.0 + 1).unwrap_or(1);

    // Capture current title from web_page — also reused for FTS row below.
    let (current_title, url): (Option<String>, String) = conn.query_row(
        "SELECT title, url FROM web_page WHERE id = ?1",
        [web_page_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let mut hasher = Sha256::new();
    hasher.update(plain_text.as_bytes());
    let content_hash = format!("{:x}", hasher.finalize());

    // change_summary vs previous snapshot. NULL for v1 (nothing to compare).
    let change_summary: Option<String> = previous.as_ref().map(|(_, prev_title, prev_size, prev_hash)| {
        let title_changed = prev_title != &current_title;
        let new_size = plain_text.len() as i64;
        let size_delta_pct: i32 = if *prev_size == 0 {
            if new_size == 0 { 0 } else { 100 }
        } else {
            (((new_size - prev_size) as f64 / *prev_size as f64) * 100.0).round() as i32
        };
        let content_same = prev_hash.as_deref() == Some(content_hash.as_str());
        format!(
            "{{\"title_changed\":{},\"size_delta_pct\":{},\"content_same\":{}}}",
            title_changed, size_delta_pct, content_same
        )
    });

    conn.execute(
        "INSERT INTO web_page_snapshot \
            (web_page_id, version, html_content, plain_text, screenshot, \
             title, content_hash, change_summary) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            web_page_id,
            version,
            html_content,
            plain_text,
            screenshot,
            current_title,
            content_hash,
            change_summary,
        ],
    )?;
    let snapshot_id = conn.last_insert_rowid();

    // Index in FTS
    conn.execute(
        "INSERT INTO web_page_fts(rowid, title, plain_text, url) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            snapshot_id,
            current_title.unwrap_or_default(),
            plain_text,
            url
        ],
    )?;

    Ok(snapshot_id)
}

/// All snapshots for a page, newest first. Returns metadata only — body is
/// fetched on demand via `get_page_version`.
#[cfg(feature = "sqlite")]
pub fn list_page_versions(conn: &Connection, web_page_id: i64) -> Result<Vec<SnapshotMeta>> {
    let mut stmt = conn.prepare(
        "SELECT id, version, fetched_at, title, LENGTH(plain_text), \
                screenshot IS NOT NULL, content_hash, change_summary \
         FROM web_page_snapshot WHERE web_page_id = ?1 \
         ORDER BY version DESC",
    )?;
    let rows = stmt
        .query_map([web_page_id], |row| {
            Ok(SnapshotMeta {
                id: row.get(0)?,
                version: row.get(1)?,
                fetched_at: row.get(2)?,
                title: row.get(3)?,
                size_bytes: row.get(4)?,
                has_screenshot: row.get(5)?,
                content_hash: row.get(6)?,
                change_summary: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Body of a specific snapshot version — preview + screenshot for the detail
/// pane. HTML is intentionally **not** returned (multi-MB; separate endpoint
/// when "View raw HTML" is implemented).
#[cfg(feature = "sqlite")]
pub fn get_page_version(conn: &Connection, snapshot_id: i64) -> Result<SnapshotContent> {
    conn.query_row(
        "SELECT id, version, fetched_at, title, LENGTH(plain_text), \
                SUBSTR(plain_text, 1, 2000), screenshot, content_hash, change_summary \
         FROM web_page_snapshot WHERE id = ?1",
        [snapshot_id],
        |row| {
            Ok(SnapshotContent {
                id: row.get(0)?,
                version: row.get(1)?,
                fetched_at: row.get(2)?,
                title: row.get(3)?,
                size_bytes: row.get(4)?,
                plain_text_preview: row.get(5)?,
                screenshot: row.get(6)?,
                content_hash: row.get(7)?,
                change_summary: row.get(8)?,
            })
        },
    )
    .map_err(Into::into)
}

/// Delete a single snapshot version. Refuses if it's the only version for its
/// page — caller should trash the page instead. FTS row cleaned up too.
#[cfg(feature = "sqlite")]
pub fn delete_page_version(conn: &Connection, snapshot_id: i64) -> Result<()> {
    let web_page_id: i64 = conn.query_row(
        "SELECT web_page_id FROM web_page_snapshot WHERE id = ?1",
        [snapshot_id],
        |r| r.get(0),
    )?;
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM web_page_snapshot WHERE web_page_id = ?1",
        [web_page_id],
        |r| r.get(0),
    )?;
    if total <= 1 {
        anyhow::bail!("cannot delete the only snapshot for a page; trash the page instead");
    }

    // FTS row first — once the snapshot is gone, we lose the rowid we'd
    // hand to the FTS contentless-table delete command.
    conn.execute(
        "INSERT INTO web_page_fts(web_page_fts, rowid, title, plain_text, url) \
         VALUES('delete', ?1, '', '', '')",
        [snapshot_id],
    )
    .ok();
    conn.execute("DELETE FROM web_page_snapshot WHERE id = ?1", [snapshot_id])?;
    Ok(())
}

/// Mark a page for re-archivace by the worker. Just toggles status back to
/// `queued`; the worker picks it up on its next run (current architecture —
/// no daemon mode yet). Idempotent.
#[cfg(feature = "sqlite")]
pub fn request_reachive(conn: &Connection, page_id: i64) -> Result<()> {
    update_status(conn, page_id, "queued")
}

#[cfg(feature = "sqlite")]
pub fn delete_page(conn: &Connection, page_id: i64) -> Result<()> {
    // Delete FTS entries for this page's snapshots
    let snapshot_ids: Vec<i64> = conn
        .prepare("SELECT id FROM web_page_snapshot WHERE web_page_id = ?1")?
        .query_map([page_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for sid in snapshot_ids {
        conn.execute(
            "INSERT INTO web_page_fts(web_page_fts, rowid, title, plain_text, url) VALUES('delete', ?1, '', '', '')",
            [sid],
        ).ok(); // Ignore errors if entry doesn't exist in FTS
    }
    conn.execute(
        "DELETE FROM web_page_snapshot WHERE web_page_id = ?1",
        [page_id],
    )?;
    conn.execute("DELETE FROM web_page WHERE id = ?1", [page_id])?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub fn find_page_by_url(conn: &Connection, url: &str) -> Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM web_page WHERE url = ?1")?;
    let result = stmt.query_row([url], |row| row.get::<_, i64>(0)).ok();
    Ok(result)
}

#[cfg(feature = "sqlite")]
pub fn ensure_page(
    conn: &Connection,
    url: &str,
    url_normalized: &str,
    title: Option<&str>,
    domain: &str,
    category: &str,
) -> Result<i64> {
    if let Some(id) = find_page_by_url(conn, url)? {
        return Ok(id);
    }
    let status = if category == "archive" {
        "queued"
    } else {
        "skipped"
    };
    insert_web_page(
        conn,
        &NewWebPage {
            url,
            url_normalized,
            title,
            domain,
            category,
            status,
            source: None,
            space_id: None,
        },
    )
}

#[cfg(feature = "sqlite")]
pub fn load_rules(conn: &Connection) -> Result<Vec<ClassificationRule>> {
    let mut stmt = conn.prepare(
        "SELECT pattern, match_type, category, COALESCE(note, '') \
         FROM classification_rule ORDER BY priority DESC",
    )?;
    let rules = stmt
        .query_map([], |row| {
            Ok(ClassificationRule {
                pattern: row.get(0)?,
                match_type: row.get(1)?,
                category: row.get(2)?,
                note: row.get(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rules)
}

#[cfg(feature = "sqlite")]
pub fn trash_page(conn: &Connection, page_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE web_page SET trashed_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        [page_id],
    )?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub fn restore_page(conn: &Connection, page_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE web_page SET trashed_at = NULL WHERE id = ?1",
        [page_id],
    )?;
    Ok(())
}

/// IDs only, ordered the same way `list_pages` returns rows. Used by the
/// keyboard-nav path to pick prev/next neighbours without paying for the
/// full row payload.
#[cfg(feature = "sqlite")]
pub fn list_page_ids_ordered(conn: &Connection, space_id: i64, limit: usize) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM web_page WHERE trashed_at IS NULL AND space_id = ?1 \
         ORDER BY created_at DESC, id DESC LIMIT ?2",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![space_id, limit as i64], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(ids)
}

#[cfg(feature = "sqlite")]
pub fn list_pages(conn: &Connection, space_id: i64, limit: usize) -> Result<Vec<WebPageRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, domain, category, status, created_at
         FROM web_page WHERE trashed_at IS NULL AND space_id = ?1
         ORDER BY created_at DESC, id DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![space_id, limit as i64], |row| {
            Ok(WebPageRow {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?,
                domain: row.get(2)?,
                category: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get::<_, String>(5)?.chars().take(10).collect(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[cfg(feature = "sqlite")]
pub fn get_page(conn: &Connection, id: i64) -> Result<WebPageDetail> {
    let (url, title, domain, category, status, created_at, last_error) = conn.query_row(
        "SELECT url, title, domain, category, status, created_at, last_error \
         FROM web_page WHERE id = ?1",
        [id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        },
    )?;

    let snapshot = conn
        .query_row(
            "SELECT LENGTH(html_content), SUBSTR(plain_text, 1, 2000), screenshot \
             FROM web_page_snapshot WHERE web_page_id = ?1 ORDER BY version DESC LIMIT 1",
            [id],
            |row| {
                Ok(WebPageSnapshot {
                    size_bytes: row.get(0)?,
                    plain_text_preview: row.get(1)?,
                    screenshot: row.get(2)?,
                })
            },
        )
        .ok();

    Ok(WebPageDetail {
        url,
        title,
        domain,
        category,
        status,
        created_at: created_at.chars().take(10).collect(),
        last_error,
        snapshot,
    })
}

/// Parse, normalize and classify a raw URL, then insert into `web_page`.
///
/// - `space_id`: `None` leaves the column NULL (CLI batch import — startup
///   bootstrap assigns these to the default space). `Some(id)` pins it.
/// - `title`: optional human title (e.g. from a `URL<TAB>TITLE` batch line).
/// - `source`: provenance string stored on the row — `"note"` for URLs
///   extracted from note bodies, `None` for explicit user adds.
///
/// Category falls back to `"archive"` if no rule matches.
#[cfg(feature = "sqlite")]
pub fn archive_url(
    conn: &Connection,
    raw_url: &str,
    space_id: Option<i64>,
    title: Option<&str>,
    source: Option<&str>,
) -> Result<ArchiveOutcome> {
    let parsed = url::Url::parse(raw_url)?;
    let rules = load_rules(conn)?;
    let normalized = rules::normalize_url(&parsed);
    let domain = parsed.host_str().unwrap_or("unknown").to_string();
    let category = rules::classify(&parsed, &rules);
    let status = if category == "archive" {
        "queued"
    } else {
        "skipped"
    };

    let id = insert_web_page(
        conn,
        &NewWebPage {
            url: raw_url,
            url_normalized: &normalized,
            title,
            domain: &domain,
            category: &category,
            status,
            source,
            space_id,
        },
    )?;
    Ok(ArchiveOutcome { id, category })
}

/// Auto-archive every URL embedded in `text` that isn't already in the DB.
/// Returns count of new pages queued. Errors during URL parse are skipped
/// silently — best-effort, called from note save paths.
#[cfg(feature = "sqlite")]
pub fn auto_archive_from_text(conn: &Connection, text: &str, space_id: i64) -> Result<usize> {
    let urls = crate::url_extract::extract_urls(text);
    let mut queued = 0usize;
    for url in urls {
        if find_page_by_url(conn, &url)?.is_some() {
            continue;
        }
        if archive_url(conn, &url, Some(space_id), None, Some("note")).is_ok() {
            queued += 1;
        }
    }
    Ok(queued)
}

/// Check archive status for multiple URLs at once
#[cfg(feature = "sqlite")]
pub fn check_urls_status(conn: &Connection, urls: &[String]) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for url in urls {
        if let Ok(Some(status)) = conn
            .query_row(
                "SELECT status FROM web_page WHERE url = ?1 OR url_normalized = ?1",
                [url],
                |row| row.get::<_, String>(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
        {
            map.insert(url.clone(), status);
        }
    }
    Ok(map)
}
