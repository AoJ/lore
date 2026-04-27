use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = include_str!("schema.sql");
const SEED: &str = include_str!("seed.sql");

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    let conn =
        Connection::open(path).with_context(|| format!("opening database {}", path.display()))?;
    conn.execute_batch(SCHEMA)
        .context("initializing database schema")?;

    // Seed classification rules if table is empty
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM classification_rule", [], |row| {
        row.get(0)
    })?;
    if count == 0 {
        conn.execute_batch(SEED)
            .context("seeding classification rules")?;
    }

    Ok(conn)
}

pub struct NewWebPage<'a> {
    pub url: &'a str,
    pub url_normalized: &'a str,
    pub title: Option<&'a str>,
    pub domain: &'a str,
    pub category: &'a str,
    pub status: &'a str,
    pub source: Option<&'a str>,
}

pub fn insert_web_page(conn: &Connection, page: &NewWebPage) -> Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO web_page (url, url_normalized, title, domain, category, status, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            page.url,
            page.url_normalized,
            page.title,
            page.domain,
            page.category,
            page.status,
            page.source,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_status(conn: &Connection, page_id: i64, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE web_page SET status = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        rusqlite::params![status, page_id],
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
        },
    )
}

/// Load classification rules from DB, ordered by priority descending.
pub struct ClassificationRule {
    pub pattern: String,
    pub match_type: String,
    pub category: String,
}

pub fn load_rules(conn: &Connection) -> Result<Vec<ClassificationRule>> {
    let mut stmt = conn.prepare(
        "SELECT pattern, match_type, category FROM classification_rule ORDER BY priority DESC",
    )?;
    let rules = stmt
        .query_map([], |row| {
            Ok(ClassificationRule {
                pattern: row.get(0)?,
                match_type: row.get(1)?,
                category: row.get(2)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rules)
}
