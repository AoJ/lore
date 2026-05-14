use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    conn.execute(
        "UPDATE space SET deleted_at = NULL WHERE id = ?1",
        [space_id],
    )?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpaceStats {
    pub page_count: i64,
    pub note_count: i64,
    pub file_count: i64,
    pub file_size_bytes: i64,
    pub pages_size_bytes: i64,
}

pub fn space_stats(conn: &Connection, space_id: i64) -> Result<SpaceStats> {
    let page_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM web_page WHERE space_id = ?1 AND trashed_at IS NULL",
        [space_id],
        |r| r.get(0),
    )?;
    let note_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM note WHERE space_id = ?1 AND deleted_at IS NULL",
        [space_id],
        |r| r.get(0),
    )?;
    let file_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM file WHERE space_id = ?1 AND deleted_at IS NULL",
        [space_id],
        |r| r.get(0),
    )?;
    let file_size_bytes: i64 = conn.query_row(
        "SELECT COALESCE(SUM(size), 0) FROM file WHERE space_id = ?1 AND deleted_at IS NULL",
        [space_id],
        |r| r.get(0),
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
        file_count,
        file_size_bytes,
        pages_size_bytes,
    })
}
