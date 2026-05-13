use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

use crate::rules;

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

pub fn update_status(conn: &Connection, page_id: i64, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE web_page SET status = ?1, last_error = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        rusqlite::params![status, page_id],
    )?;
    Ok(())
}

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

pub fn insert_snapshot(
    conn: &Connection,
    web_page_id: i64,
    html_content: &str,
    plain_text: &str,
    screenshot: Option<&[u8]>,
) -> Result<i64> {
    let version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM web_page_snapshot WHERE web_page_id = ?1",
            [web_page_id],
            |row| row.get(0),
        )
        .unwrap_or(1);

    conn.execute(
        "INSERT INTO web_page_snapshot (web_page_id, version, html_content, plain_text, screenshot)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![web_page_id, version, html_content, plain_text, screenshot],
    )?;
    let snapshot_id = conn.last_insert_rowid();

    // Index in FTS
    let (title, url): (Option<String>, String) = conn.query_row(
        "SELECT title, url FROM web_page WHERE id = ?1",
        [web_page_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    conn.execute(
        "INSERT INTO web_page_fts(rowid, title, plain_text, url) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![snapshot_id, title.unwrap_or_default(), plain_text, url],
    )?;

    Ok(snapshot_id)
}

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

pub fn find_page_by_url(conn: &Connection, url: &str) -> Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM web_page WHERE url = ?1")?;
    let result = stmt.query_row([url], |row| row.get::<_, i64>(0)).ok();
    Ok(result)
}

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

/// Classification rule from DB, ordered by priority descending.
#[derive(Clone, Debug)]
pub struct ClassificationRule {
    pub pattern: String,
    pub match_type: String,
    pub category: String,
    pub note: String,
}

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

pub fn trash_page(conn: &Connection, page_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE web_page SET trashed_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        [page_id],
    )?;
    Ok(())
}

pub fn restore_page(conn: &Connection, page_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE web_page SET trashed_at = NULL WHERE id = ?1",
        [page_id],
    )?;
    Ok(())
}

/// Web page summary returned by `list_pages` and search.
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub struct WebPageSnapshot {
    pub size_bytes: i64,
    pub plain_text_preview: Option<String>,
    pub screenshot: Option<Vec<u8>>,
}

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

/// Outcome of `archive_url`: returns the row id plus the classifier category
/// (e.g. "archive", "discard") so callers can show appropriate feedback.
#[derive(Clone, Debug)]
pub struct ArchiveOutcome {
    pub id: i64,
    pub category: String,
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
