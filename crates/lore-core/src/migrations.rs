//! DB schema versioning and migration runner.
//!
//! The DB stores its schema version in SQLite's built-in `PRAGMA user_version`
//! (a 32-bit integer in the file header). The application embeds the
//! [`EXPECTED_VERSION`] it knows how to operate on. On every connection open:
//!
//! 1. If `db_version > EXPECTED_VERSION` → refuse to start (the DB was touched
//!    by a newer build of the app — running with an older one would corrupt data).
//! 2. If `db_version < EXPECTED_VERSION` → apply migrations
//!    `(db_version + 1)..=EXPECTED_VERSION`, each in its own transaction so a
//!    failure leaves the DB at a clean intermediate version.
//! 3. If `db_version == 0` and tables already exist → "legacy unversioned DB"
//!    bridge: stamp the version to [`EXPECTED_VERSION`] (this branch only
//!    triggers once, when the pre-versioning DB is first opened by a versioned
//!    build).
//!
//! Migrations live in `crates/lore-core/migrations/NNNN_description.sql` and
//! are embedded into the binary via `include_str!`. Migrations needing Rust
//! code (e.g. SHA256 backfill, regex rewrites) are registered as
//! [`Step::Code`] entries instead.

use anyhow::{Context, Result, anyhow};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

/// The schema version this build of lore knows how to use. Bump by 1 with
/// every new migration added to [`MIGRATIONS`]. The value lives at the
/// crate root so the WASM client (no `sqlite` feature → no `migrations`
/// module) can still compare against it; this is just a re-export.
pub const EXPECTED_VERSION: u32 = crate::EXPECTED_DB_SCHEMA_VERSION;

enum Step {
    Sql(&'static str),
    Code(fn(&Connection) -> Result<()>),
}

/// Index N (zero-based) is the migration that takes the DB from version `N`
/// to version `N+1`. Order is **load-bearing** — never reorder, never delete
/// past entries, only append.
const MIGRATIONS: &[Step] = &[
    Step::Sql(include_str!("../migrations/0001_initial.sql")),
    Step::Sql(include_str!("../migrations/0002_space_deleted_at.sql")),
    Step::Sql(include_str!("../migrations/0003_file_table.sql")),
    Step::Code(m0004_attachment_size_hash),
    Step::Sql(include_str!(
        "../migrations/0005_rewrite_attachment_urls.sql"
    )),
    Step::Code(m0006_unescape_attachment_links),
    Step::Sql(include_str!(
        "../migrations/0007_revision_triggers_completion.sql"
    )),
];

/// Read the current schema version of the DB.
pub fn current_version(conn: &Connection) -> Result<u32> {
    let v: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    Ok(v)
}

fn set_user_version(conn: &Connection, v: u32) -> Result<()> {
    // PRAGMA user_version doesn't accept SQL parameter binding; format inline.
    // Safe because v is a u32 we control.
    conn.execute_batch(&format!("PRAGMA user_version = {};", v))?;
    Ok(())
}

/// True if this DB has tables but no version stamp — i.e. it was created by
/// a pre-versioning build of lore.
fn is_legacy_unversioned(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='note'",
        [],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

/// Apply pending migrations to bring the DB up to [`EXPECTED_VERSION`].
/// Returns the version the DB ended at (always equals `EXPECTED_VERSION` on
/// success).
pub fn apply(conn: &mut Connection) -> Result<u32> {
    let current = current_version(conn)?;

    if current > EXPECTED_VERSION {
        return Err(anyhow!(
            "DB schema is v{} but this build of lore only knows v{}. \
             Run a newer version of the app, or restore an older DB backup.",
            current,
            EXPECTED_VERSION
        ));
    }

    if current == 0 && is_legacy_unversioned(conn) {
        eprintln!(
            "[lore] legacy unversioned DB detected, stamping to v{}",
            EXPECTED_VERSION
        );
        set_user_version(conn, EXPECTED_VERSION)?;
        return Ok(EXPECTED_VERSION);
    }

    for v in (current + 1)..=EXPECTED_VERSION {
        let tx = conn.transaction()?;
        let step = &MIGRATIONS[(v - 1) as usize];
        match step {
            Step::Sql(sql) => {
                tx.execute_batch(sql)
                    .with_context(|| format!("applying SQL migration v{}", v))?;
            }
            Step::Code(f) => {
                f(&tx).with_context(|| format!("applying code migration v{}", v))?;
            }
        }
        tx.execute_batch(&format!("PRAGMA user_version = {};", v))?;
        tx.commit()
            .with_context(|| format!("committing migration v{}", v))?;
    }

    Ok(EXPECTED_VERSION)
}

// ---------------------------------------------------------------------------
// Code migrations
// ---------------------------------------------------------------------------

/// v3 → v4: add `size`, `hash`, `deleted_at` columns to `note_attachment`,
/// backfill SHA256 hashes for rows that pre-date the column.
fn m0004_attachment_size_hash(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "ALTER TABLE note_attachment ADD COLUMN size INTEGER NOT NULL DEFAULT 0;\
         ALTER TABLE note_attachment ADD COLUMN hash TEXT NOT NULL DEFAULT '';\
         ALTER TABLE note_attachment ADD COLUMN deleted_at TEXT;\
         CREATE INDEX idx_note_attachment_deleted ON note_attachment(deleted_at);\
         CREATE TRIGGER trg_rev_attachment_u AFTER UPDATE ON note_attachment \
            BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;",
    )?;

    let ids: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM note_attachment WHERE hash = ''")?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    for id in ids {
        let bytes: Vec<u8> = conn.query_row(
            "SELECT data FROM note_attachment WHERE id = ?1",
            [id],
            |r| r.get(0),
        )?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = format!("{:x}", hasher.finalize());
        conn.execute(
            "UPDATE note_attachment SET size = ?1, hash = ?2 WHERE id = ?3",
            rusqlite::params![bytes.len() as i64, hash, id],
        )?;
    }
    Ok(())
}

/// v5 → v6: un-escape brackets/parens around attachment links that Milkdown
/// serialized as plain text back when it didn't recognize `lore://` as a URL
/// scheme.
fn m0006_unescape_attachment_links(conn: &Connection) -> Result<()> {
    let re =
        regex::Regex::new(r"\\\[([^\]\\]*)\\?\]\\\((https://attachment\.lore\.invalid/\d+)\\?\)")
            .expect("static regex");

    let rows: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, title, body FROM note \
             WHERE body LIKE '%attachment.lore.invalid/%' \
                OR title LIKE '%attachment.lore.invalid/%'",
        )?;
        let mapped = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        mapped.filter_map(|r| r.ok()).collect()
    };

    for (id, title, body) in rows {
        let fixed_title = re.replace_all(&title, "[$1]($2)").into_owned();
        let fixed_body = re.replace_all(&body, "[$1]($2)").into_owned();
        if fixed_title != title || fixed_body != body {
            conn.execute(
                "UPDATE note SET title = ?1, body = ?2 WHERE id = ?3",
                rusqlite::params![fixed_title, fixed_body, id],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_in_memory() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn migration_count_matches_expected_version() {
        assert_eq!(MIGRATIONS.len() as u32, EXPECTED_VERSION);
    }

    #[test]
    fn fresh_db_migrates_to_expected_version() {
        let mut conn = open_in_memory();
        assert_eq!(current_version(&conn).unwrap(), 0);
        apply(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), EXPECTED_VERSION);
    }

    #[test]
    fn idempotent_open() {
        let mut conn = open_in_memory();
        apply(&mut conn).unwrap();
        apply(&mut conn).unwrap();
        apply(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), EXPECTED_VERSION);
    }

    #[test]
    fn refuses_newer_db() {
        let mut conn = open_in_memory();
        set_user_version(&conn, EXPECTED_VERSION + 1).unwrap();
        let err = apply(&mut conn).unwrap_err();
        assert!(err.to_string().contains("newer version"));
    }

    #[test]
    fn legacy_db_is_stamped() {
        // Simulate a pre-versioning DB: tables exist, user_version = 0.
        let mut conn = open_in_memory();
        conn.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        // user_version is still 0 — this is what an old DB looks like.
        assert_eq!(current_version(&conn).unwrap(), 0);
        apply(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), EXPECTED_VERSION);
    }

    #[test]
    fn partial_migration_resumes() {
        let mut conn = open_in_memory();
        // Apply 0001 manually, stamp to v1.
        conn.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        set_user_version(&conn, 1).unwrap();
        // Runner should fill in 2..=EXPECTED.
        apply(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), EXPECTED_VERSION);
        // Verify v3 actually ran (file table exists).
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='file'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(exists);
    }

    // ---- m0006: unescape \[label\]\(url\) → [label](url) ----

    fn open_at_full_schema() -> Connection {
        // Schema needs `note` (created by 0001), 0005 rewrote attachment URLs
        // to `https://attachment.lore.invalid/<id>`. Easiest: just open via apply().
        let mut conn = open_in_memory();
        apply(&mut conn).unwrap();
        // Seed a space row so insert into note doesn't trip the FK.
        conn.execute(
            "INSERT INTO space (id, name, last_used) \
             VALUES (1, 'Test', strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            [],
        )
        .unwrap();
        conn
    }

    fn insert_note_raw(conn: &Connection, title: &str, body: &str) -> i64 {
        conn.execute(
            "INSERT INTO note (title, body, space_id, created_at, updated_at) \
             VALUES (?1, ?2, 1, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            rusqlite::params![title, body],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn read_note(conn: &Connection, id: i64) -> (String, String) {
        conn.query_row("SELECT title, body FROM note WHERE id = ?1", [id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .unwrap()
    }

    #[test]
    fn m0006_unescapes_attachment_link_in_body() {
        let conn = open_at_full_schema();
        // Milkdown serialized escapes around `lore://` attachment links back
        // when it didn't recognize the scheme — m0006 reverses it. The
        // label itself is plain text (no backslashes), only the surrounding
        // brackets/parens were escaped.
        let body = r"see \[doc.txt\]\(https://attachment.lore.invalid/42\) for details";
        let id = insert_note_raw(&conn, "title", body);

        m0006_unescape_attachment_links(&conn).unwrap();

        let (_t, b) = read_note(&conn, id);
        assert_eq!(
            b,
            "see [doc.txt](https://attachment.lore.invalid/42) for details"
        );
    }

    #[test]
    fn m0006_unescapes_link_in_title_only() {
        // Kills L185:33 `||→&&` (would skip rows where only title changed)
        // and the `!=→==` mutations: the row must be updated even though
        // body has nothing to fix.
        let conn = open_at_full_schema();
        let title = r"image \[pic\]\(https://attachment.lore.invalid/7\)";
        let body = "plain body, nothing to unescape";
        let id = insert_note_raw(&conn, title, body);

        m0006_unescape_attachment_links(&conn).unwrap();

        let (t, b) = read_note(&conn, id);
        assert_eq!(t, "image [pic](https://attachment.lore.invalid/7)");
        assert_eq!(b, "plain body, nothing to unescape");
    }

    #[test]
    fn m0006_unescapes_link_in_body_only() {
        // Symmetric to the title-only case — guards the other branch of the
        // `fixed_title != title || fixed_body != body` OR.
        let conn = open_at_full_schema();
        let title = "plain title, nothing to fix";
        let body = r"file \[notes\]\(https://attachment.lore.invalid/99\)";
        let id = insert_note_raw(&conn, title, body);

        m0006_unescape_attachment_links(&conn).unwrap();

        let (t, b) = read_note(&conn, id);
        assert_eq!(t, "plain title, nothing to fix");
        assert_eq!(b, "file [notes](https://attachment.lore.invalid/99)");
    }

    #[test]
    fn m0006_leaves_unrelated_notes_untouched() {
        // Note without escaped attachment links must not be UPDATEd. We can't
        // observe "no UPDATE ran" directly, but we can prove the contents
        // are byte-identical after the migration — which kills the
        // "replace fn -> Ok(())" mutant in the negative direction (would not
        // be caught by the unescape tests above, since those would see the
        // same body before and after — they need to differ).
        let conn = open_at_full_schema();
        let title = "no attachments here";
        let body = "completely plain markdown";
        let id = insert_note_raw(&conn, title, body);

        m0006_unescape_attachment_links(&conn).unwrap();

        let (t, b) = read_note(&conn, id);
        assert_eq!(t, title);
        assert_eq!(b, body);
    }
}
