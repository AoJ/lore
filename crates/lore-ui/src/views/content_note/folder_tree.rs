//! Flatten the recursive folder tree into a depth-tagged list, and resolve a
//! folder id to its breadcrumb path. Both used by the Move-to menu and the
//! note footer.

use lore_core::db::FolderRow;

/// Returns `(id, name, depth)` in DFS order so the Move-to menu can render
/// nested folders with consistent indent.
pub fn build_folder_tree(
    folders: &[FolderRow],
    parent_id: Option<i64>,
    depth: usize,
) -> Vec<(i64, String, usize)> {
    let mut result = Vec::new();
    for f in folders.iter().filter(|f| f.parent_id == parent_id) {
        result.push((f.id, f.name.clone(), depth));
        result.extend(build_folder_tree(folders, Some(f.id), depth + 1));
    }
    result
}

/// Walk up `parent_id` chain and produce e.g. `"Work / Projects / Lore"`.
/// Returns `None` for root (no folder).
pub fn folder_path(folders: &[FolderRow], folder_id: Option<i64>) -> Option<String> {
    let fid = folder_id?;
    let mut parts = Vec::new();
    let mut current = fid;
    loop {
        let folder = folders.iter().find(|f| f.id == current)?;
        parts.push(folder.name.clone());
        match folder.parent_id {
            Some(pid) => current = pid,
            None => break,
        }
    }
    parts.reverse();
    Some(parts.join(" / "))
}
