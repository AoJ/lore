use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderRow {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub sort_order: i64,
    pub space_id: Option<i64>,
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

pub fn insert_folder(
    conn: &Connection,
    name: &str,
    parent_id: Option<i64>,
    space_id: i64,
) -> Result<i64> {
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

/// Count notes per folder (direct children only, excludes deleted)
pub fn folder_note_counts(conn: &Connection, space_id: i64) -> Result<HashMap<i64, i64>> {
    let mut stmt = conn.prepare(
        "SELECT folder_id, COUNT(*) FROM note WHERE space_id = ?1 AND deleted_at IS NULL AND folder_id IS NOT NULL GROUP BY folder_id",
    )?;
    let mut map = HashMap::new();
    let rows = stmt.query_map([space_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    for (fid, count) in rows.flatten() {
        map.insert(fid, count);
    }
    Ok(map)
}
