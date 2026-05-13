use anyhow::Result;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub struct FileRow {
    pub id: i64,
    pub name: String,
    pub mime_type: Option<String>,
    pub size: i64,
    pub hash: String,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

/// Outcome of `insert_file` so the caller can show appropriate feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertFileOutcome {
    Inserted,
    DedupedActive,
    RevivedFromTrash,
}

/// Insert a file, or dedupe against an existing file in this space matching
/// `name + hash`. Active duplicate → return its ID. Trashed duplicate →
/// clear `deleted_at` and return its ID (silent revive from trash).
pub fn insert_file(
    conn: &Connection,
    name: &str,
    mime_type: Option<&str>,
    data: &[u8],
    space_id: i64,
) -> Result<(i64, InsertFileOutcome)> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = format!("{:x}", hasher.finalize());
    let size = data.len() as i64;

    if let Ok(id) = conn.query_row(
        "SELECT id FROM file WHERE name = ?1 AND hash = ?2 AND space_id = ?3 AND deleted_at IS NULL",
        rusqlite::params![name, hash, space_id],
        |row| row.get::<_, i64>(0),
    ) {
        return Ok((id, InsertFileOutcome::DedupedActive));
    }

    if let Ok(id) = conn.query_row(
        "SELECT id FROM file WHERE name = ?1 AND hash = ?2 AND space_id = ?3 AND deleted_at IS NOT NULL",
        rusqlite::params![name, hash, space_id],
        |row| row.get::<_, i64>(0),
    ) {
        conn.execute("UPDATE file SET deleted_at = NULL WHERE id = ?1", [id])?;
        return Ok((id, InsertFileOutcome::RevivedFromTrash));
    }

    conn.execute(
        "INSERT INTO file (name, mime_type, size, hash, data, space_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![name, mime_type, size, hash, data, space_id],
    )?;
    Ok((conn.last_insert_rowid(), InsertFileOutcome::Inserted))
}

pub fn list_files(conn: &Connection, space_id: i64) -> Result<Vec<FileRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, mime_type, size, hash, created_at \
         FROM file WHERE space_id = ?1 AND deleted_at IS NULL \
         ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map([space_id], |row| {
            Ok(FileRow {
                id: row.get(0)?,
                name: row.get(1)?,
                mime_type: row.get(2)?,
                size: row.get(3)?,
                hash: row.get(4)?,
                created_at: row.get::<_, String>(5)?.chars().take(10).collect(),
                deleted_at: None,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn get_file(conn: &Connection, id: i64) -> Result<FileRow> {
    conn.query_row(
        "SELECT id, name, mime_type, size, hash, created_at, deleted_at FROM file WHERE id = ?1",
        [id],
        |row| {
            Ok(FileRow {
                id: row.get(0)?,
                name: row.get(1)?,
                mime_type: row.get(2)?,
                size: row.get(3)?,
                hash: row.get(4)?,
                created_at: row.get::<_, String>(5)?.chars().take(10).collect(),
                deleted_at: row.get(6)?,
            })
        },
    )
    .map_err(Into::into)
}

pub fn get_file_data(conn: &Connection, id: i64) -> Result<(Option<String>, Vec<u8>)> {
    conn.query_row(
        "SELECT mime_type, data FROM file WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(Into::into)
}

pub fn trash_file(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE file SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

pub fn restore_file(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("UPDATE file SET deleted_at = NULL WHERE id = ?1", [id])?;
    Ok(())
}

pub fn delete_file_permanent(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM file WHERE id = ?1", [id])?;
    Ok(())
}

pub fn list_trashed_files(conn: &Connection, space_id: i64) -> Result<Vec<FileRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, mime_type, size, hash, created_at, deleted_at \
         FROM file WHERE space_id = ?1 AND deleted_at IS NOT NULL \
         ORDER BY deleted_at DESC",
    )?;
    let rows = stmt
        .query_map([space_id], |row| {
            Ok(FileRow {
                id: row.get(0)?,
                name: row.get(1)?,
                mime_type: row.get(2)?,
                size: row.get(3)?,
                hash: row.get(4)?,
                created_at: row.get::<_, String>(5)?.chars().take(10).collect(),
                deleted_at: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}
