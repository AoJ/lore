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

    // Migration: add deleted_at to space if missing
    let has_space_deleted: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('space') WHERE name='deleted_at'")
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false);
    if !has_space_deleted {
        conn.execute_batch("ALTER TABLE space ADD COLUMN deleted_at TEXT;").ok();
    }

    // Ensure revision counter is seeded (existing DBs may not have it)
    conn.execute("INSERT OR IGNORE INTO db_revision (id, revision) VALUES (1, 0)", []).ok();

    // Seed default space if none exists
    let space_count: i64 = conn.query_row("SELECT COUNT(*) FROM space", [], |row| row.get(0))?;
    if space_count == 0 {
        conn.execute(
            "INSERT INTO space (name, last_used) VALUES ('Personal', strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            [],
        )?;
        // Assign all existing content to default space
        let default_id: i64 = conn.last_insert_rowid();
        conn.execute("UPDATE web_page SET space_id = ?1 WHERE space_id IS NULL", [default_id])?;
        conn.execute("UPDATE note SET space_id = ?1 WHERE space_id IS NULL", [default_id])?;
        conn.execute("UPDATE note_folder SET space_id = ?1 WHERE space_id IS NULL", [default_id])?;
    }

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

/// Get the current global revision number. Incremented by DB triggers on every change.
pub fn get_revision(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT revision FROM db_revision WHERE id = 1", [], |r| r.get(0))
        .map_err(Into::into)
}

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

// ---- Soft-delete (trash) for web pages ----

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

pub fn trash_count(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM web_page WHERE trashed_at IS NOT NULL",
        [],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

pub fn cleanup_old_trash(conn: &Connection, days: i64) -> Result<usize> {
    let cutoff = format!("-{} days", days);
    let mut cleaned = 0usize;

    // Clean old trashed pages
    let page_ids: Vec<i64> = conn
        .prepare(
            "SELECT id FROM web_page WHERE trashed_at IS NOT NULL AND trashed_at < strftime('%Y-%m-%dT%H:%M:%fZ','now', ?1)",
        )?
        .query_map([&cutoff], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in &page_ids {
        delete_page(conn, *id)?;
        cleaned += 1;
    }

    // Clean old trashed spaces (permanently delete with all content)
    let space_ids: Vec<i64> = conn
        .prepare(
            "SELECT id FROM space WHERE deleted_at IS NOT NULL AND deleted_at < strftime('%Y-%m-%dT%H:%M:%fZ','now', ?1)",
        )?
        .query_map([&cutoff], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in &space_ids {
        delete_space_permanent(conn, *id)?;
        cleaned += 1;
    }

    Ok(cleaned)
}

// ---- Notes ----

pub fn insert_note(conn: &Connection, title: &str, body: &str, folder_id: Option<i64>, space_id: i64) -> Result<i64> {
    conn.execute(
        "INSERT INTO note (title, body, folder_id, space_id) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![title, body, folder_id, space_id],
    )?;
    let note_id = conn.last_insert_rowid();
    // Index in FTS
    conn.execute(
        "INSERT INTO note_fts(rowid, title, body) VALUES (?1, ?2, ?3)",
        rusqlite::params![note_id, title, body],
    )?;
    Ok(note_id)
}

pub fn update_note(conn: &Connection, note_id: i64, title: &str, body: &str) -> Result<()> {
    // Update FTS (delete old, insert new)
    conn.execute(
        "INSERT INTO note_fts(note_fts, rowid, title, body) VALUES('delete', ?1, '', '')",
        [note_id],
    )
    .ok();
    conn.execute(
        "INSERT INTO note_fts(rowid, title, body) VALUES (?1, ?2, ?3)",
        rusqlite::params![note_id, title, body],
    )?;
    conn.execute(
        "UPDATE note SET title = ?1, body = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?3",
        rusqlite::params![title, body, note_id],
    )?;
    Ok(())
}

pub fn move_note_to_folder(conn: &Connection, note_id: i64, folder_id: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE note SET folder_id = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
        rusqlite::params![folder_id, note_id],
    )?;
    Ok(())
}

pub fn trash_note(conn: &Connection, note_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE note SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        [note_id],
    )?;
    Ok(())
}

pub fn restore_note(conn: &Connection, note_id: i64) -> Result<()> {
    conn.execute("UPDATE note SET deleted_at = NULL WHERE id = ?1", [note_id])?;
    Ok(())
}

pub fn delete_note_permanent(conn: &Connection, note_id: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO note_fts(note_fts, rowid, title, body) VALUES('delete', ?1, '', '')",
        [note_id],
    )
    .ok();
    conn.execute("DELETE FROM note WHERE id = ?1", [note_id])?;
    Ok(())
}

pub struct NoteRow {
    pub id: i64,
    pub title: String,
    pub body_preview: String,
    pub folder_id: Option<i64>,
    pub updated_at: String,
}

pub fn list_notes(conn: &Connection, folder_id: Option<i64>, space_id: i64) -> Result<Vec<NoteRow>> {
    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(fid) = folder_id {
        (
            "SELECT id, title, SUBSTR(body, 1, 100), folder_id, updated_at FROM note WHERE deleted_at IS NULL AND folder_id = ?1 AND space_id = ?2 ORDER BY updated_at DESC".to_string(),
            vec![Box::new(fid) as Box<dyn rusqlite::types::ToSql>, Box::new(space_id)],
        )
    } else {
        (
            "SELECT id, title, SUBSTR(body, 1, 100), folder_id, updated_at FROM note WHERE deleted_at IS NULL AND folder_id IS NULL AND space_id = ?1 ORDER BY updated_at DESC".to_string(),
            vec![Box::new(space_id) as Box<dyn rusqlite::types::ToSql>],
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(refs.as_slice(), |row| {
            Ok(NoteRow {
                id: row.get(0)?,
                title: row.get(1)?,
                body_preview: row.get(2)?,
                folder_id: row.get(3)?,
                updated_at: row.get::<_, String>(4)?.chars().take(10).collect(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub struct NoteData {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub folder_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

pub fn get_note(conn: &Connection, note_id: i64) -> Result<NoteData> {
    conn.query_row(
        "SELECT id, title, body, folder_id, created_at, updated_at, deleted_at FROM note WHERE id = ?1",
        [note_id],
        |row| {
            Ok(NoteData {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                folder_id: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                deleted_at: row.get(6)?,
            })
        },
    )
    .map_err(Into::into)
}

// ---- Folders ----

pub struct FolderRow {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub sort_order: i64,
    pub space_id: Option<i64>,
}

impl Clone for FolderRow {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            parent_id: self.parent_id,
            sort_order: self.sort_order,
            space_id: self.space_id,
        }
    }
}

pub fn list_folders(conn: &Connection, space_id: i64) -> Result<Vec<FolderRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, parent_id, sort_order, space_id FROM note_folder WHERE space_id = ?1 ORDER BY sort_order, name",
    )?;
    let rows = stmt
        .query_map([space_id], |row| {
            Ok(FolderRow {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                sort_order: row.get(3)?,
                space_id: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn insert_folder(conn: &Connection, name: &str, parent_id: Option<i64>, space_id: i64) -> Result<i64> {
    conn.execute(
        "INSERT INTO note_folder (name, parent_id, space_id) VALUES (?1, ?2, ?3)",
        rusqlite::params![name, parent_id, space_id],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_folder(conn: &Connection, folder_id: i64, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE note_folder SET name = ?1 WHERE id = ?2",
        rusqlite::params![name, folder_id],
    )?;
    Ok(())
}

pub fn delete_folder(conn: &Connection, folder_id: i64) -> Result<()> {
    let parent: Option<i64> = conn
        .query_row(
            "SELECT parent_id FROM note_folder WHERE id = ?1",
            [folder_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    // Move all notes (active and trashed) to parent folder — FK constraint
    // requires this since the folder row is being deleted
    conn.execute(
        "UPDATE note SET folder_id = ?1 WHERE folder_id = ?2",
        rusqlite::params![parent, folder_id],
    )?;
    // Move subfolders to parent
    conn.execute(
        "UPDATE note_folder SET parent_id = ?1 WHERE parent_id = ?2",
        rusqlite::params![parent, folder_id],
    )?;
    conn.execute("DELETE FROM note_folder WHERE id = ?1", [folder_id])?;
    Ok(())
}

/// Restore a trashed note. If its folder no longer exists, move to root.
pub fn restore_note_safe(conn: &Connection, note_id: i64) -> Result<()> {
    conn.execute("UPDATE note SET deleted_at = NULL WHERE id = ?1", [note_id])?;
    // Check if folder still exists
    let folder_id: Option<i64> = conn.query_row(
        "SELECT folder_id FROM note WHERE id = ?1", [note_id], |r| r.get(0),
    )?;
    if let Some(fid) = folder_id {
        let exists: bool = conn
            .prepare("SELECT 1 FROM note_folder WHERE id = ?1")?
            .exists([fid])?;
        if !exists {
            conn.execute("UPDATE note SET folder_id = NULL WHERE id = ?1", [note_id])?;
        }
    }
    Ok(())
}

// ---- Spaces ----

pub struct SpaceRow {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub last_used: Option<String>,
    pub deleted_at: Option<String>,
}

/// List active (non-deleted) spaces
pub fn list_spaces(conn: &Connection) -> Result<Vec<SpaceRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, color, last_used, deleted_at FROM space WHERE deleted_at IS NULL ORDER BY last_used DESC, created_at DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SpaceRow {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                last_used: row.get(3)?,
                deleted_at: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// List ALL spaces including soft-deleted (for Settings view)
pub fn list_all_spaces(conn: &Connection) -> Result<Vec<SpaceRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, color, last_used, deleted_at FROM space ORDER BY deleted_at IS NOT NULL, last_used DESC, created_at DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SpaceRow {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                last_used: row.get(3)?,
                deleted_at: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn get_active_space(conn: &Connection) -> Result<SpaceRow> {
    conn.query_row(
        "SELECT id, name, color, last_used, deleted_at FROM space WHERE deleted_at IS NULL ORDER BY last_used DESC, created_at DESC LIMIT 1",
        [],
        |row| {
            Ok(SpaceRow {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                last_used: row.get(3)?,
                deleted_at: row.get(4)?,
            })
        },
    )
    .map_err(Into::into)
}

pub fn touch_space(conn: &Connection, space_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE space SET last_used = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        [space_id],
    )?;
    Ok(())
}

pub fn insert_space(conn: &Connection, name: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO space (name, last_used) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        [name],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_space(conn: &Connection, space_id: i64, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE space SET name = ?1 WHERE id = ?2",
        rusqlite::params![name, space_id],
    )?;
    Ok(())
}

/// Soft-delete a space — content stays but is inaccessible until restored
pub fn trash_space(conn: &Connection, space_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE space SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        [space_id],
    )?;
    Ok(())
}

pub fn restore_space(conn: &Connection, space_id: i64) -> Result<()> {
    conn.execute("UPDATE space SET deleted_at = NULL WHERE id = ?1", [space_id])?;
    Ok(())
}

/// Permanently delete a space and ALL its content
pub fn delete_space_permanent(conn: &Connection, space_id: i64) -> Result<()> {
    // Delete all content in this space
    // First get snapshot IDs for FTS cleanup
    let page_ids: Vec<i64> = conn
        .prepare("SELECT id FROM web_page WHERE space_id = ?1")?
        .query_map([space_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for pid in page_ids {
        delete_page(conn, pid)?;
    }
    let note_ids: Vec<i64> = conn
        .prepare("SELECT id FROM note WHERE space_id = ?1")?
        .query_map([space_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for nid in note_ids {
        delete_note_permanent(conn, nid)?;
    }
    conn.execute("DELETE FROM note_folder WHERE space_id = ?1", [space_id])?;
    conn.execute("DELETE FROM space WHERE id = ?1", [space_id])?;
    Ok(())
}

pub struct SpaceStats {
    pub page_count: i64,
    pub note_count: i64,
    pub file_count: i64,
    pub pages_size_bytes: i64,
}

pub fn space_stats(conn: &Connection, space_id: i64) -> Result<SpaceStats> {
    let page_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM web_page WHERE space_id = ?1 AND trashed_at IS NULL",
        [space_id], |r| r.get(0),
    )?;
    let note_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM note WHERE space_id = ?1 AND deleted_at IS NULL",
        [space_id], |r| r.get(0),
    )?;
    let pages_size_bytes: i64 = conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(wps.html_content) + LENGTH(COALESCE(wps.plain_text,'')) + LENGTH(COALESCE(wps.screenshot,''))), 0)
         FROM web_page_snapshot wps
         JOIN web_page wp ON wp.id = wps.web_page_id
         WHERE wp.space_id = ?1 AND wp.trashed_at IS NULL",
        [space_id], |r| r.get(0),
    )?;
    Ok(SpaceStats {
        page_count,
        note_count,
        file_count: 0, // files not implemented yet
        pages_size_bytes,
    })
}

/// Find notes that reference a given URL in their body text
pub fn find_notes_referencing_url(conn: &Connection, url: &str, space_id: i64) -> Result<Vec<(i64, String)>> {
    let pattern = format!("%{}%", url);
    let mut stmt = conn.prepare(
        "SELECT id, title FROM note WHERE body LIKE ?1 AND space_id = ?2 AND deleted_at IS NULL ORDER BY updated_at DESC",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![pattern, space_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Check archive status for multiple URLs at once
pub fn check_urls_status(conn: &Connection, urls: &[String]) -> Result<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();
    for url in urls {
        if let Ok(Some(status)) = conn.query_row(
            "SELECT status FROM web_page WHERE url = ?1 OR url_normalized = ?1",
            [url],
            |row| row.get::<_, String>(0),
        ).map(Some).or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        }) {
            map.insert(url.clone(), status);
        }
    }
    Ok(map)
}

/// Count notes per folder (direct children only, excludes deleted)
pub fn folder_note_counts(conn: &Connection, space_id: i64) -> Result<std::collections::HashMap<i64, i64>> {
    let mut stmt = conn.prepare(
        "SELECT folder_id, COUNT(*) FROM note WHERE space_id = ?1 AND deleted_at IS NULL AND folder_id IS NOT NULL GROUP BY folder_id",
    )?;
    let mut map = std::collections::HashMap::new();
    let rows = stmt.query_map([space_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        if let Ok((fid, count)) = row {
            map.insert(fid, count);
        }
    }
    Ok(map)
}
