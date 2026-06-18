//! Markdown-folder import: pour a directory tree of `.md` files into a space as
//! notes, **idempotently**. Identity = the source path relative to the import
//! root, so re-importing the same folder updates changed files and skips
//! unchanged ones instead of duplicating — and refuses to clobber a note you
//! edited inside lore.
//!
//! Three-way conflict model (`base` = content hash recorded at the last import,
//! `lore` = current note body, `file` = current source file):
//!
//! | condition                       | action   |
//! |---------------------------------|----------|
//! | no note for this path           | insert   |
//! | `lore == file`                  | skip     |
//! | `lore != file`, `lore == base`  | update   (only the file changed — safe) |
//! | `lore != file`, `lore != base`  | conflict (note edited in lore) |
//!
//! Any conflict aborts the whole import: the transaction is rolled back and
//! nothing is written, so you can resolve it and re-run.
//!
//! Out of scope (planned follow-up): converting locally-referenced files into
//! note attachments, `--prune` of vanished sources, and parsing per-file
//! created/updated timestamps.

use anyhow::{Context, Result, anyhow};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::db::{
    find_imported_note, get_or_create_folder, insert_imported_note, update_imported_note,
};

/// Summary of an import run. On a real (non-dry-run) import the counts reflect
/// what was committed; on a dry run they reflect what *would* happen.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
    /// Source paths (relative to the import root) where the note was edited in
    /// lore and the file also differs. Left untouched; aborts a real import.
    pub conflicts: Vec<String>,
}

/// Per-file decision. Pure, so it's unit-testable without a DB.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Insert,
    Update,
    Skip,
    Conflict,
}

/// Three-way decision. `existing` is `(lore_hash, base)` for the note already at
/// this source path, where `base` is the hash recorded at the last import.
fn decide(file_hash: &str, existing: Option<(&str, Option<&str>)>) -> Action {
    match existing {
        None => Action::Insert,
        Some((lore_hash, base)) => {
            if lore_hash == file_hash {
                Action::Skip
            } else if base == Some(lore_hash) {
                // lore is still exactly what we last imported → only the file
                // moved on → safe to fast-forward.
                Action::Update
            } else {
                // lore diverged from the recorded base (edited in lore), or we
                // have no base to prove otherwise → refuse to clobber.
                Action::Conflict
            }
        }
    }
}

/// Normalise line endings so CRLF/LF differences don't read as content changes.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn sha256(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

/// Title = the first non-blank line if it's an ATX heading (`# ...`), else
/// `None` (caller falls back to the file name). Only the first non-blank line
/// is considered, so body prose never becomes the title.
fn extract_title(body: &str) -> Option<String> {
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let rest = t.strip_prefix('#')?; // first non-blank line must be a heading
        let title = rest.trim_start_matches('#').trim();
        return if title.is_empty() {
            None
        } else {
            Some(title.to_string())
        };
    }
    None
}

/// File stem (name without `.md`) as a title fallback.
fn file_stem_title(rel: &Path) -> String {
    rel.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled".to_string())
}

/// Posix-style relative path string — the stable cross-platform import identity.
fn rel_to_str(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Collect `.md` files under `root`, returning paths relative to `root`, sorted
/// for deterministic ordering. Hidden entries (dotfiles/dirs) are skipped.
fn collect_markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading dir {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue; // skip dotfiles/dirs
        }
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            walk(root, &path, out)?;
        } else if ty.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
    Ok(())
}

/// Ensure the folder chain `root_folder / <subdirs of rel>` exists, returning
/// the leaf folder id. `cache` memoises sub-paths within one import run.
fn ensure_folder_path(
    conn: &Connection,
    space_id: i64,
    root_folder: &str,
    rel: &Path,
    cache: &mut HashMap<PathBuf, i64>,
) -> Result<i64> {
    let root_key = PathBuf::new();
    let mut current = if let Some(&id) = cache.get(&root_key) {
        id
    } else {
        let id = get_or_create_folder(conn, root_folder, None, space_id)?;
        cache.insert(root_key, id);
        id
    };
    if let Some(parent) = rel.parent() {
        let mut key = PathBuf::new();
        for comp in parent.components() {
            let name = comp.as_os_str().to_string_lossy().into_owned();
            key.push(&name);
            if let Some(&id) = cache.get(&key) {
                current = id;
                continue;
            }
            current = get_or_create_folder(conn, &name, Some(current), space_id)?;
            cache.insert(key.clone(), current);
        }
    }
    Ok(current)
}

/// Import every `.md` under `root` into `space_id`, mirroring the directory tree
/// under a top-level `root_folder`. See the module docs for the conflict model.
/// `dry_run` rolls back at the end (and reports conflicts instead of erroring).
pub fn import_markdown_dir(
    conn: &mut Connection,
    root: &Path,
    space_id: i64,
    root_folder: &str,
    dry_run: bool,
) -> Result<ImportReport> {
    let files = collect_markdown_files(root)?;
    let tx = conn.transaction()?;
    let mut report = ImportReport::default();
    let mut folder_cache: HashMap<PathBuf, i64> = HashMap::new();

    for rel in &files {
        let abs = root.join(rel);
        let raw =
            std::fs::read_to_string(&abs).with_context(|| format!("reading {}", abs.display()))?;
        let body = normalize(&raw);
        let file_hash = sha256(&body);
        let title = extract_title(&body).unwrap_or_else(|| file_stem_title(rel));
        let import_source = rel_to_str(rel);

        let existing = find_imported_note(&tx, space_id, &import_source)?;
        let lore_hash = existing.as_ref().map(|n| sha256(&normalize(&n.body)));
        let action = decide(
            &file_hash,
            existing
                .as_ref()
                .map(|n| (lore_hash.as_deref().unwrap(), n.import_hash.as_deref())),
        );

        match action {
            Action::Insert => {
                let folder_id =
                    ensure_folder_path(&tx, space_id, root_folder, rel, &mut folder_cache)?;
                insert_imported_note(
                    &tx,
                    &title,
                    &body,
                    Some(folder_id),
                    space_id,
                    &import_source,
                    &file_hash,
                )?;
                report.inserted += 1;
            }
            Action::Update => {
                let id = existing.as_ref().unwrap().id;
                update_imported_note(&tx, id, &title, &body, &file_hash)?;
                report.updated += 1;
            }
            Action::Skip => report.skipped += 1,
            Action::Conflict => report.conflicts.push(import_source),
        }
    }

    if dry_run {
        drop(tx); // roll back — this was only a preview
        return Ok(report);
    }
    if !report.conflicts.is_empty() {
        drop(tx); // atomic abort — write nothing
        return Err(anyhow!(
            "import aborted: {} note(s) were edited in lore and differ from the source \
             (resolve in lore or re-export, then re-run):\n  {}",
            report.conflicts.len(),
            report.conflicts.join("\n  ")
        ));
    }
    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn test_db() -> (Connection, i64) {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::apply(&mut conn).unwrap();
        let space_id = crate::db::insert_space(&conn, "Test").unwrap();
        (conn, space_id)
    }

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A unique scratch dir; removed on drop.
    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let p = std::env::temp_dir().join(format!("lore-import-{}-{}", std::process::id(), n));
            std::fs::create_dir_all(&p).unwrap();
            TmpDir(p)
        }
        fn write(&self, rel: &str, contents: &str) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn decide_covers_all_cases() {
        assert_eq!(decide("f", None), Action::Insert);
        assert_eq!(decide("f", Some(("f", Some("f")))), Action::Skip);
        assert_eq!(decide("f", Some(("f", Some("old")))), Action::Skip); // lore==file wins
        assert_eq!(decide("f2", Some(("f1", Some("f1")))), Action::Update); // lore==base, file moved
        assert_eq!(decide("f2", Some(("edited", Some("f1")))), Action::Conflict); // lore edited
        assert_eq!(decide("f2", Some(("edited", None))), Action::Conflict); // no base
    }

    #[test]
    fn extract_title_prefers_heading_then_filename() {
        assert_eq!(extract_title("# Hello\n\nbody"), Some("Hello".into()));
        assert_eq!(
            extract_title("\n\n## Deep title\n"),
            Some("Deep title".into())
        );
        assert_eq!(extract_title("just prose\n# later"), None);
        assert_eq!(extract_title("#\nno text"), None);
    }

    #[test]
    fn insert_then_reimport_is_idempotent() {
        let (mut conn, space) = test_db();
        let dir = TmpDir::new();
        dir.write("a.md", "# Alpha\n\nbody a");
        dir.write("sub/b.md", "# Beta\n\nbody b");

        let r1 = import_markdown_dir(&mut conn, &dir.0, space, "docs", false).unwrap();
        assert_eq!((r1.inserted, r1.updated, r1.skipped), (2, 0, 0));
        assert!(r1.conflicts.is_empty());

        // re-import unchanged → all skipped, no duplicates
        let r2 = import_markdown_dir(&mut conn, &dir.0, space, "docs", false).unwrap();
        assert_eq!((r2.inserted, r2.updated, r2.skipped), (0, 0, 2));

        // exactly two notes exist, with the heading titles and the source paths
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note WHERE space_id = ?1",
                [space],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        let (title, src): (String, String) = conn
            .query_row(
                "SELECT title, import_source FROM note WHERE import_source = 'sub/b.md'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "Beta");
        assert_eq!(src, "sub/b.md");
        // sub-folder mirrored under the root folder "docs"
        let nested: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_folder f JOIN note_folder p ON f.parent_id = p.id \
                 WHERE f.name = 'sub' AND p.name = 'docs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(nested, 1);
    }

    #[test]
    fn changed_file_updates_when_lore_untouched() {
        let (mut conn, space) = test_db();
        let dir = TmpDir::new();
        dir.write("a.md", "# A\n\nv1");
        import_markdown_dir(&mut conn, &dir.0, space, "docs", false).unwrap();

        dir.write("a.md", "# A\n\nv2 updated");
        let r = import_markdown_dir(&mut conn, &dir.0, space, "docs", false).unwrap();
        assert_eq!((r.inserted, r.updated, r.skipped), (0, 1, 0));
        let body: String = conn
            .query_row(
                "SELECT body FROM note WHERE import_source = 'a.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(body, "# A\n\nv2 updated");
    }

    #[test]
    fn edited_in_lore_then_reimport_conflicts_and_writes_nothing() {
        let (mut conn, space) = test_db();
        let dir = TmpDir::new();
        dir.write("a.md", "# A\n\norig");
        dir.write("b.md", "# B\n\nb");
        import_markdown_dir(&mut conn, &dir.0, space, "docs", false).unwrap();

        // edit the note inside lore (diverge from base)
        conn.execute(
            "UPDATE note SET body = 'edited in lore' WHERE import_source = 'a.md'",
            [],
        )
        .unwrap();
        // and change the file too
        dir.write("a.md", "# A\n\nnew from file");
        // also change b so we can prove the abort is atomic (b not written)
        dir.write("b.md", "# B\n\nb changed");

        let err = import_markdown_dir(&mut conn, &dir.0, space, "docs", false).unwrap_err();
        assert!(err.to_string().contains("a.md"));

        // nothing was written: a still the lore edit, b still original
        let a: String = conn
            .query_row(
                "SELECT body FROM note WHERE import_source = 'a.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(a, "edited in lore");
        let b: String = conn
            .query_row(
                "SELECT body FROM note WHERE import_source = 'b.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b, "# B\n\nb");
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let (mut conn, space) = test_db();
        let dir = TmpDir::new();
        dir.write("a.md", "# A\n\nx");
        let r = import_markdown_dir(&mut conn, &dir.0, space, "docs", true).unwrap();
        assert_eq!(r.inserted, 1);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM note", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0); // dry run wrote nothing
    }
}
