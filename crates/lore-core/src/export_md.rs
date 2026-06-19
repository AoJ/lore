//! Markdown export: write a space's notes out as a directory tree of `.md`
//! files — the inverse of [`crate::import_md`], for putting notes on GitHub or
//! converting to Word via a template.
//!
//! Layout: the note-folder tree becomes subdirectories under the output dir;
//! each note is `<slug(title)>.md` with YAML frontmatter (title / created /
//! updated). Attachments referenced in the body
//! (`https://attachment.lore.invalid/<id>`) are written next to the note in a
//! `<slug>.assets/` folder and the link rewritten to that relative path.
//!
//! Export overwrites files; it does not delete stale ones. Not idempotent-
//! tracked (it's an output operation) — re-export anytime.

use anyhow::{Context, Result};
use regex::Regex;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::db::{get_attachment, get_attachment_data, list_folders};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub notes: usize,
    pub attachments: usize,
}

/// A note row as needed for export.
struct NoteRow {
    title: String,
    body: String,
    folder_id: Option<i64>,
    created_at: String,
    updated_at: String,
}

static ATTACHMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https://attachment\.lore\.invalid/(\d+)").unwrap());

/// Slug for a filename: unicode alphanumerics kept (lowercased), everything else
/// collapsed to single dashes. Empty → "untitled".
fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let s = out.trim_end_matches('-').to_string();
    if s.is_empty() {
        "untitled".to_string()
    } else {
        s
    }
}

/// Sanitise a folder name into a single path component (keep it readable, just
/// drop path-illegal characters).
fn sanitize_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if "/\\:".contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "folder".to_string()
    } else {
        trimmed.to_string()
    }
}

/// YAML double-quoted scalar.
fn yaml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Minimal link-target encoding: spaces → %20 (enough for markdown link paths).
fn encode_target(s: &str) -> String {
    s.replace(' ', "%20")
}

fn note_rows(conn: &Connection, space_id: i64) -> Result<Vec<NoteRow>> {
    let mut stmt = conn.prepare(
        "SELECT title, body, folder_id, created_at, updated_at FROM note \
         WHERE space_id = ?1 AND deleted_at IS NULL ORDER BY title",
    )?;
    let rows = stmt
        .query_map([space_id], |r| {
            Ok(NoteRow {
                title: r.get(0)?,
                body: r.get(1)?,
                folder_id: r.get(2)?,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// folder_id → (name, parent_id)
fn folder_map(conn: &Connection, space_id: i64) -> Result<HashMap<i64, (String, Option<i64>)>> {
    let folders = list_folders(conn, space_id)?;
    Ok(folders
        .into_iter()
        .map(|f| (f.id, (f.name, f.parent_id)))
        .collect())
}

/// Relative directory components (root → leaf) for a note's folder.
fn folder_path(folder_id: Option<i64>, map: &HashMap<i64, (String, Option<i64>)>) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = folder_id;
    while let Some(id) = cur {
        let Some((name, parent)) = map.get(&id) else {
            break;
        };
        parts.push(sanitize_component(name));
        cur = *parent;
    }
    parts.reverse();
    parts
}

/// Folder ids in the subtree rooted at `root` (inclusive).
fn descendants(root: i64, map: &HashMap<i64, (String, Option<i64>)>) -> HashSet<i64> {
    let mut set = HashSet::new();
    set.insert(root);
    let mut changed = true;
    while changed {
        changed = false;
        for (id, (_, parent)) in map {
            if let Some(p) = parent
                && set.contains(p)
                && set.insert(*id)
            {
                changed = true;
            }
        }
    }
    set
}

/// Export notes of `space_id` (optionally only the subtree under `folder_id`) to
/// `out` as a markdown tree. `dry_run` counts without writing.
pub fn export_markdown_dir(
    conn: &Connection,
    out: &Path,
    space_id: i64,
    folder_id: Option<i64>,
    dry_run: bool,
) -> Result<ExportReport> {
    let fmap = folder_map(conn, space_id)?;
    let scope = folder_id.map(|id| descendants(id, &fmap));
    let mut report = ExportReport::default();
    // de-dup note filenames within each directory
    let mut used: HashMap<PathBuf, HashSet<String>> = HashMap::new();

    for note in note_rows(conn, space_id)? {
        if let Some(ref scope) = scope {
            match note.folder_id {
                Some(fid) if scope.contains(&fid) => {}
                _ => continue, // outside the requested subtree
            }
        }

        let dir = {
            let mut d = out.to_path_buf();
            for c in folder_path(note.folder_id, &fmap) {
                d.push(c);
            }
            d
        };

        // unique <slug>.md within this dir
        let base = slug(&note.title);
        let names = used.entry(dir.clone()).or_default();
        let mut stem = base.clone();
        let mut n = 2;
        while !names.insert(stem.clone()) {
            stem = format!("{base}-{n}");
            n += 1;
        }
        let md_path = dir.join(format!("{stem}.md"));

        // rewrite attachments referenced in the body
        let (body, n_att) = rewrite_attachments(conn, &note.body, &dir, &stem, dry_run)?;
        report.attachments += n_att;

        let content = format!(
            "---\ntitle: {}\ncreated: {}\nupdated: {}\n---\n\n{}",
            yaml_str(&note.title),
            note.created_at,
            note.updated_at,
            body,
        );

        if !dry_run {
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            std::fs::write(&md_path, content)
                .with_context(|| format!("writing {}", md_path.display()))?;
        }
        report.notes += 1;
    }

    Ok(report)
}

/// Write each referenced attachment to `<dir>/<stem>.assets/<name>` and rewrite
/// its `attachment.lore.invalid/<id>` URL to that relative path. Returns the
/// rewritten body + the number of distinct attachments written.
fn rewrite_attachments(
    conn: &Connection,
    body: &str,
    dir: &Path,
    stem: &str,
    dry_run: bool,
) -> Result<(String, usize)> {
    // collect distinct ids referenced
    let ids: Vec<i64> = ATTACHMENT_RE
        .captures_iter(body)
        .filter_map(|c| c.get(1)?.as_str().parse::<i64>().ok())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return Ok((body.to_string(), 0));
    }

    let assets_rel = format!("{stem}.assets");
    let assets_dir = dir.join(&assets_rel);
    let mut link_for: HashMap<i64, String> = HashMap::new();
    let mut used_names: HashSet<String> = HashSet::new();
    let mut written = 0usize;

    for id in ids {
        let Ok(att) = get_attachment(conn, id) else {
            continue; // referenced id no longer exists — leave the URL as-is
        };
        // unique filename within the assets dir
        let mut name = att.name.clone();
        if !used_names.insert(name.clone()) {
            name = format!("{id}-{}", att.name);
            used_names.insert(name.clone());
        }
        if !dry_run {
            let (_, data) = get_attachment_data(conn, id)?;
            std::fs::create_dir_all(&assets_dir)
                .with_context(|| format!("creating {}", assets_dir.display()))?;
            std::fs::write(assets_dir.join(&name), data)
                .with_context(|| format!("writing attachment {name}"))?;
        }
        link_for.insert(id, format!("{}/{}", assets_rel, encode_target(&name)));
        written += 1;
    }

    let new_body = ATTACHMENT_RE
        .replace_all(body, |caps: &regex::Captures<'_>| {
            let id: i64 = caps[1].parse().unwrap_or(-1);
            link_for
                .get(&id)
                .cloned()
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned();

    Ok((new_body, written))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let p = std::env::temp_dir().join(format!("lore-export-{}-{}", std::process::id(), n));
            std::fs::create_dir_all(&p).unwrap();
            TmpDir(p)
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_db() -> (Connection, i64) {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::apply(&mut conn).unwrap();
        let space = crate::db::insert_space(&conn, "Test").unwrap();
        (conn, space)
    }

    #[test]
    fn slug_handles_unicode_and_punctuation() {
        assert_eq!(slug("Hello, World!"), "hello-world");
        assert_eq!(slug("Produkční NAS"), "produkční-nas");
        assert_eq!(slug("  ...  "), "untitled");
    }

    #[test]
    fn exports_tree_with_frontmatter() {
        let (conn, space) = test_db();
        let folder = crate::db::insert_folder(&conn, "sub", None, space).unwrap();
        crate::db::insert_note(&conn, "Root Note", "# Root Note\n\nbody", None, space).unwrap();
        crate::db::insert_note(&conn, "Deep", "# Deep\n\nx", Some(folder), space).unwrap();

        let out = TmpDir::new();
        let r = export_markdown_dir(&conn, &out.0, space, None, false).unwrap();
        assert_eq!(r.notes, 2);

        let root_md = std::fs::read_to_string(out.0.join("root-note.md")).unwrap();
        assert!(root_md.starts_with("---\ntitle: \"Root Note\"\n"));
        assert!(root_md.contains("\n# Root Note\n"));
        let deep_md = std::fs::read_to_string(out.0.join("sub").join("deep.md")).unwrap();
        assert!(deep_md.contains("title: \"Deep\""));
    }

    #[test]
    fn exports_attachment_next_to_note_and_rewrites_link() {
        let (conn, space) = test_db();
        let nid =
            crate::db::insert_note(&conn, "Pic", "# Pic\n\nplaceholder", None, space).unwrap();
        let (aid, _) =
            crate::db::insert_attachment(&conn, nid, "img.png", "image/png", b"PNGDATA").unwrap();
        let body = format!("# Pic\n\n![p](https://attachment.lore.invalid/{aid})");
        crate::db::update_note(&conn, nid, "Pic", &body).unwrap();

        let out = TmpDir::new();
        let r = export_markdown_dir(&conn, &out.0, space, None, false).unwrap();
        assert_eq!((r.notes, r.attachments), (1, 1));

        let md = std::fs::read_to_string(out.0.join("pic.md")).unwrap();
        assert!(md.contains("![p](pic.assets/img.png)"), "got: {md}");
        let bytes = std::fs::read(out.0.join("pic.assets").join("img.png")).unwrap();
        assert_eq!(bytes, b"PNGDATA");
    }

    #[test]
    fn dry_run_writes_nothing() {
        let (conn, space) = test_db();
        crate::db::insert_note(&conn, "A", "# A", None, space).unwrap();
        let out = TmpDir::new();
        let r = export_markdown_dir(&conn, &out.0, space, None, true).unwrap();
        assert_eq!(r.notes, 1);
        assert!(std::fs::read_dir(&out.0).unwrap().next().is_none()); // empty
    }

    #[test]
    fn folder_scope_limits_export() {
        let (conn, space) = test_db();
        let f = crate::db::insert_folder(&conn, "keep", None, space).unwrap();
        crate::db::insert_note(&conn, "In", "# In", Some(f), space).unwrap();
        crate::db::insert_note(&conn, "Out", "# Out", None, space).unwrap();

        let out = TmpDir::new();
        let r = export_markdown_dir(&conn, &out.0, space, Some(f), false).unwrap();
        assert_eq!(r.notes, 1);
        assert!(out.0.join("keep").join("in.md").exists());
        assert!(!out.0.join("out.md").exists());
    }
}
