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
    /// First-archive timestamp (`web_page.created_at`). UI usually shows
    /// `last_fetched_at` instead, but this is kept for ordering / "first
    /// seen" displays.
    pub created_at: String,
    /// Timestamp of the most recent snapshot (`MAX(fetched_at)`). `None` if
    /// the page has never been archived (status `queued` / `failed` and no
    /// snapshots yet). Drives the header date in the detail view.
    pub last_fetched_at: Option<String>,
    /// Sum across every snapshot version of `html_content + plain_text +
    /// screenshot + title` byte lengths. Reflects the real DB cost of
    /// keeping a page archived, not the latest version's text size alone.
    pub total_size_bytes: i64,
    pub last_error: Option<String>,
    pub snapshot: Option<WebPageSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebPageSnapshot {
    pub size_bytes: i64,
    pub plain_text_preview: Option<String>,
    /// PNG bytes of the down-scaled thumbnail. This is what the detail panel
    /// shows by default — much cheaper to ship than the full screenshot.
    /// `None` on legacy snapshots that pre-date migration 0010 (in which
    /// case the UI falls back to the full screenshot via the lazy endpoint).
    #[serde(with = "crate::serde_b64::opt_vec")]
    pub screenshot_thumb: Option<Vec<u8>>,
    /// True iff a full-size screenshot exists for this snapshot. Lets the
    /// UI decide whether to render the "click to enlarge" affordance
    /// without shipping the bytes up-front.
    pub has_full_screenshot: bool,
    /// Cleaned `<article>` HTML from readability extraction (m0011+). UI
    /// renders this as the Article view; `None` on legacy snapshots or
    /// when extraction failed (login wall, JS-only app, …).
    pub readability_html: Option<String>,
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
    /// Thumbnail (down-scaled PNG) shown by default. `None` for legacy
    /// snapshots — see `WebPageSnapshot::screenshot_thumb`.
    #[serde(with = "crate::serde_b64::opt_vec")]
    pub screenshot_thumb: Option<Vec<u8>>,
    /// Full-size screenshot is loaded lazily on click via
    /// `get_snapshot_full_screenshot` — this flag tells the UI whether to
    /// render the click-to-enlarge button at all.
    pub has_full_screenshot: bool,
    pub content_hash: Option<String>,
    pub change_summary: Option<String>,
    /// Cleaned article HTML (m0011+). UI renders this as the default
    /// Article view; falls back to `plain_text_preview` when None.
    pub readability_html: Option<String>,
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
pub fn insert_web_page(conn: &Connection, page: &NewWebPage<'_>) -> Result<i64> {
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

/// Compute the `change_summary` JSON for a snapshot vs its predecessor.
/// Shared by `insert_snapshot` (new snapshot) and `delete_page_version`
/// (recompute on the snapshot that newly inherits a different predecessor).
#[cfg(feature = "sqlite")]
fn compute_change_summary(
    prev_title: &Option<String>,
    prev_text_size: i64,
    prev_hash: Option<&str>,
    current_title: &Option<String>,
    current_text_size: i64,
    current_hash: &str,
) -> String {
    let title_changed = prev_title != current_title;
    let size_delta_pct: i32 = if prev_text_size == 0 {
        if current_text_size == 0 { 0 } else { 100 }
    } else {
        (((current_text_size - prev_text_size) as f64 / prev_text_size as f64) * 100.0).round()
            as i32
    };
    let content_same = prev_hash == Some(current_hash);
    format!(
        "{{\"title_changed\":{},\"size_delta_pct\":{},\"content_same\":{}}}",
        title_changed, size_delta_pct, content_same
    )
}

/// Bundle of readability-extracted fields. Passed to `insert_snapshot` as
/// a single optional struct so adding more fields later doesn't keep
/// growing the function signature.
///
/// Only `html` and `text` are stored:
/// - `html` is what the Article tab renders
/// - `text` is what FTS indexes (cleaner signal than raw plain_text)
#[cfg(feature = "sqlite")]
#[derive(Debug, Default, Clone)]
pub struct ReadabilityBundle<'a> {
    pub html: Option<&'a str>,
    pub text: Option<&'a str>,
}

#[cfg(feature = "sqlite")]
pub fn insert_snapshot(
    conn: &Connection,
    web_page_id: i64,
    html_content: &str,
    plain_text: &str,
    screenshot: Option<&[u8]>,
    screenshot_thumb: Option<&[u8]>,
    readability: ReadabilityBundle<'_>,
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

    let change_summary: Option<String> =
        previous
            .as_ref()
            .map(|(_, prev_title, prev_size, prev_hash)| {
                compute_change_summary(
                    prev_title,
                    *prev_size,
                    prev_hash.as_deref(),
                    &current_title,
                    plain_text.len() as i64,
                    &content_hash,
                )
            });

    conn.execute(
        "INSERT INTO web_page_snapshot \
            (web_page_id, version, html_content, plain_text, screenshot, \
             screenshot_thumb, title, content_hash, change_summary, \
             readability_html, readability_text) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            web_page_id,
            version,
            html_content,
            plain_text,
            screenshot,
            screenshot_thumb,
            current_title,
            content_hash,
            change_summary,
            readability.html,
            readability.text,
        ],
    )?;
    let snapshot_id = conn.last_insert_rowid();

    // FTS: prefer the cleaned readability text when available — much higher
    // signal-to-noise than raw page text (no nav/footer/ad copy). Falls
    // back to raw plain_text on legacy/extraction-failed snapshots.
    let fts_text = readability.text.unwrap_or(plain_text);
    conn.execute(
        "INSERT INTO web_page_fts(rowid, title, plain_text, url) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            snapshot_id,
            current_title.unwrap_or_default(),
            fts_text,
            url
        ],
    )?;

    Ok(snapshot_id)
}

/// All snapshots for a page, newest first. Returns metadata only — body is
/// fetched on demand via `get_page_version`.
///
/// `size_bytes` is the **total** per-snapshot storage cost (HTML + plain
/// text + screenshot + title), so the version list reflects DB footprint
/// rather than just text length.
#[cfg(feature = "sqlite")]
pub fn list_page_versions(conn: &Connection, web_page_id: i64) -> Result<Vec<SnapshotMeta>> {
    let mut stmt = conn.prepare(
        "SELECT id, version, fetched_at, title, \
                COALESCE(LENGTH(html_content),0) + COALESCE(LENGTH(plain_text),0) + \
                  COALESCE(LENGTH(screenshot),0) + COALESCE(LENGTH(title),0), \
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
                SUBSTR(plain_text, 1, 2000), screenshot_thumb, \
                screenshot IS NOT NULL, content_hash, change_summary, \
                readability_html \
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
                screenshot_thumb: row.get(6)?,
                has_full_screenshot: row.get(7)?,
                content_hash: row.get(8)?,
                change_summary: row.get(9)?,
                readability_html: row.get(10)?,
            })
        },
    )
    .map_err(Into::into)
}

/// Full-size screenshot PNG bytes for a snapshot. Returns `None` if the
/// snapshot has no full screenshot (`screenshot` column is NULL), which
/// the UI uses to gracefully no-op the click-to-enlarge action. Errors
/// only on DB issues.
#[cfg(feature = "sqlite")]
pub fn get_snapshot_full_screenshot(
    conn: &Connection,
    snapshot_id: i64,
) -> Result<Option<Vec<u8>>> {
    let bytes: Option<Vec<u8>> = conn.query_row(
        "SELECT screenshot FROM web_page_snapshot WHERE id = ?1",
        [snapshot_id],
        |row| row.get(0),
    )?;
    Ok(bytes)
}

/// Delete a single snapshot version. Refuses if it's the only version for its
/// page — caller should trash the page instead. FTS row cleaned up too.
///
/// After delete, the snapshot that immediately followed the deleted one (if
/// any) now diffs against a different predecessor — its `change_summary` is
/// recomputed so badges stay meaningful. If the deleted version was the
/// oldest (v1), the new oldest has its summary cleared (no predecessor).
#[cfg(feature = "sqlite")]
pub fn delete_page_version(conn: &Connection, snapshot_id: i64) -> Result<()> {
    let (web_page_id, deleted_version): (i64, i64) = conn.query_row(
        "SELECT web_page_id, version FROM web_page_snapshot WHERE id = ?1",
        [snapshot_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
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

    // Recompute change_summary for whatever snapshot is now "next" after the
    // gap (the one whose diff base just changed).
    let next: Option<(i64, Option<String>, i64, Option<String>)> = conn
        .query_row(
            "SELECT id, title, LENGTH(plain_text), content_hash \
             FROM web_page_snapshot \
             WHERE web_page_id = ?1 AND version > ?2 \
             ORDER BY version ASC LIMIT 1",
            rusqlite::params![web_page_id, deleted_version],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();

    if let Some((next_id, next_title, next_size, next_hash)) = next {
        // Find the new predecessor: highest version still below the gap.
        let new_predecessor: Option<(Option<String>, i64, Option<String>)> = conn
            .query_row(
                "SELECT title, LENGTH(plain_text), content_hash \
                 FROM web_page_snapshot \
                 WHERE web_page_id = ?1 AND version < ?2 \
                 ORDER BY version DESC LIMIT 1",
                rusqlite::params![web_page_id, deleted_version],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();

        let new_summary: Option<String> = new_predecessor.map(|(pt, ps, ph)| {
            compute_change_summary(
                &pt,
                ps,
                ph.as_deref(),
                &next_title,
                next_size,
                next_hash.as_deref().unwrap_or(""),
            )
        });

        conn.execute(
            "UPDATE web_page_snapshot SET change_summary = ?1 WHERE id = ?2",
            rusqlite::params![new_summary, next_id],
        )?;
    }

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
            "SELECT LENGTH(html_content), SUBSTR(plain_text, 1, 2000), \
                    screenshot_thumb, screenshot IS NOT NULL, \
                    readability_html \
             FROM web_page_snapshot WHERE web_page_id = ?1 ORDER BY version DESC LIMIT 1",
            [id],
            |row| {
                Ok(WebPageSnapshot {
                    size_bytes: row.get(0)?,
                    plain_text_preview: row.get(1)?,
                    screenshot_thumb: row.get(2)?,
                    has_full_screenshot: row.get(3)?,
                    readability_html: row.get(4)?,
                })
            },
        )
        .ok();

    // Sum across all snapshots: html + text + screenshot + title columns.
    // Returns 0 if there are no snapshots (page queued/failed) — same query,
    // SUM-on-empty produces NULL which `COALESCE` turns into 0.
    let total_size_bytes: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM( \
                COALESCE(LENGTH(html_content),0) + COALESCE(LENGTH(plain_text),0) + \
                COALESCE(LENGTH(screenshot),0) + COALESCE(LENGTH(title),0) \
             ), 0) FROM web_page_snapshot WHERE web_page_id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let last_fetched_at: Option<String> = conn
        .query_row(
            "SELECT MAX(fetched_at) FROM web_page_snapshot WHERE web_page_id = ?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )
        .unwrap_or(None);

    Ok(WebPageDetail {
        url,
        title,
        domain,
        category,
        status,
        created_at: created_at.chars().take(10).collect(),
        last_fetched_at,
        total_size_bytes,
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
    // Refuse internal attachment URLs — they are a UI rendering protocol,
    // not real pages. Without this guard a user pasting a copied attachment
    // link into the URL box would create a fake page entry.
    if parsed.host_str() == Some(INTERNAL_ATTACHMENT_HOST) {
        anyhow::bail!("refusing to archive internal attachment URL: {}", raw_url);
    }
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

/// Host suffix used by the file-attachment block in notes: links like
/// `https://attachment.lore.invalid/42` are an in-DB protocol, not real
/// pages. Archiving them would dirty the Web list with bogus rows.
pub const INTERNAL_ATTACHMENT_HOST: &str = "attachment.lore.invalid";

/// True if `url` points at our internal attachment protocol — used to filter
/// out internal links from auto-archive and (defensively) from manual adds.
fn is_internal_url(url: &str) -> bool {
    url::Url::parse(url)
        .map(|u| u.host_str() == Some(INTERNAL_ATTACHMENT_HOST))
        .unwrap_or(false)
}

/// Auto-archive every URL embedded in `text` that isn't already in the DB.
/// Returns count of new pages queued. Errors during URL parse are skipped
/// silently — best-effort, called from note save paths.
///
/// Internal attachment URLs (`attachment.lore.invalid/...`) are filtered
/// out — they are renderable inside notes but are not real pages.
#[cfg(feature = "sqlite")]
pub fn auto_archive_from_text(conn: &Connection, text: &str, space_id: i64) -> Result<usize> {
    let urls = crate::url_extract::extract_urls(text);
    let mut queued = 0usize;
    for url in urls {
        if is_internal_url(&url) {
            continue;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_change_summary_zero_prev_text_zero_current() {
        // Signature: (prev_title, prev_text_size, prev_hash, current_title, current_text_size, current_hash)
        // prev_text_size == 0, current == 0 → size_delta_pct = 0
        let summary = compute_change_summary(
            &Some("title".to_string()),
            0,
            Some("hash1"),
            &Some("title".to_string()),
            0,
            "hash1",
        );
        assert!(summary.contains("\"size_delta_pct\":0"), "Got: {}", summary);
    }

    #[test]
    fn compute_change_summary_zero_prev_text_nonzero_current() {
        // prev_text_size == 0, current > 0 → size_delta_pct = 100
        let summary = compute_change_summary(
            &Some("title".to_string()),
            0,
            Some("hash1"),
            &Some("title".to_string()),
            100,
            "hash2",
        );
        assert!(summary.contains("\"size_delta_pct\":100"), "Got: {}", summary);
    }

    #[test]
    fn compute_change_summary_title_changed_true() {
        let summary = compute_change_summary(
            &Some("old".to_string()),
            100,
            Some("hash"),
            &Some("new".to_string()),
            100,
            "hash",
        );
        assert!(summary.contains("\"title_changed\":true"), "Got: {}", summary);
    }

    #[test]
    fn compute_change_summary_title_unchanged() {
        let summary = compute_change_summary(
            &Some("same".to_string()),
            100,
            Some("hash"),
            &Some("same".to_string()),
            100,
            "hash",
        );
        assert!(summary.contains("\"title_changed\":false"), "Got: {}", summary);
    }

    #[test]
    fn compute_change_summary_content_hash_match() {
        // prev_hash == current_hash → content_same = true
        let summary = compute_change_summary(
            &Some("title".to_string()),
            100,
            Some("same_hash"),
            &Some("title".to_string()),
            100,
            "same_hash",
        );
        assert!(summary.contains("\"content_same\":true"), "Got: {}", summary);
    }

    #[test]
    fn compute_change_summary_content_hash_mismatch() {
        // prev_hash != current_hash → content_same = false
        let summary = compute_change_summary(
            &Some("title".to_string()),
            100,
            Some("old_hash"),
            &Some("title".to_string()),
            100,
            "different_hash",
        );
        assert!(summary.contains("\"content_same\":false"), "Got: {}", summary);
    }
}
