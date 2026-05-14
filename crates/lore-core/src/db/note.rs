use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub fn insert_note(
    conn: &Connection,
    title: &str,
    body: &str,
    folder_id: Option<i64>,
    space_id: i64,
) -> Result<i64> {
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
    // Hard-delete attachments. Note: `attachment::delete_attachments_for_note`
    // does the same single DELETE but inlining keeps note.rs free of
    // sibling-submodule references.
    conn.execute("DELETE FROM note_attachment WHERE note_id = ?1", [note_id])?;
    conn.execute("DELETE FROM note WHERE id = ?1", [note_id])?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoteRow {
    pub id: i64,
    pub title: String,
    pub body_preview: String,
    pub folder_id: Option<i64>,
    pub updated_at: String,
}

/// IDs only, ordered the same way `list_notes` returns rows. Used by the
/// keyboard-nav path to pick prev/next neighbours without paying for the
/// full row payload.
pub fn list_note_ids_ordered(
    conn: &Connection,
    folder_id: Option<i64>,
    space_id: i64,
) -> Result<Vec<i64>> {
    if let Some(fid) = folder_id {
        let mut stmt = conn.prepare(
            "SELECT id FROM note WHERE deleted_at IS NULL AND folder_id = ?1 \
             ORDER BY updated_at DESC",
        )?;
        let ids = stmt
            .query_map([fid], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    } else {
        let mut stmt = conn.prepare(
            "SELECT id FROM note WHERE deleted_at IS NULL AND space_id = ?1 \
             ORDER BY updated_at DESC",
        )?;
        let ids = stmt
            .query_map([space_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }
}

pub fn list_notes(
    conn: &Connection,
    folder_id: Option<i64>,
    space_id: i64,
) -> Result<Vec<NoteRow>> {
    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(fid) = folder_id
    {
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

#[derive(Clone, Debug, Serialize, Deserialize)]
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

/// Restore a trashed note. If its folder no longer exists, move to root.
pub fn restore_note_safe(conn: &Connection, note_id: i64) -> Result<()> {
    conn.execute("UPDATE note SET deleted_at = NULL WHERE id = ?1", [note_id])?;
    // Check if folder still exists
    let folder_id: Option<i64> =
        conn.query_row("SELECT folder_id FROM note WHERE id = ?1", [note_id], |r| {
            r.get(0)
        })?;
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

/// Find notes that reference a given URL in their body text
pub fn find_notes_referencing_url(
    conn: &Connection,
    url: &str,
    space_id: i64,
) -> Result<Vec<(i64, String)>> {
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
