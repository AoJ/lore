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

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: i64, name: &str, parent_id: Option<i64>) -> FolderRow {
        FolderRow {
            id,
            name: name.to_string(),
            parent_id,
            sort_order: 0,
            space_id: Some(1),
        }
    }

    #[test]
    fn flat_tree_listed_in_order() {
        let folders = vec![folder(1, "A", None), folder(2, "B", None)];
        let tree = build_folder_tree(&folders, None, 0);
        assert_eq!(
            tree,
            vec![(1, "A".into(), 0), (2, "B".into(), 0)],
        );
    }

    #[test]
    fn nested_children_get_increasing_depth() {
        let folders = vec![
            folder(1, "Work", None),
            folder(2, "Projects", Some(1)),
            folder(3, "Lore", Some(2)),
            folder(4, "Personal", None),
        ];
        let tree = build_folder_tree(&folders, None, 0);
        assert_eq!(
            tree,
            vec![
                (1, "Work".into(), 0),
                (2, "Projects".into(), 1),
                (3, "Lore".into(), 2),
                (4, "Personal".into(), 0),
            ],
        );
    }

    #[test]
    fn folder_path_returns_none_for_root() {
        let folders = vec![folder(1, "A", None)];
        assert_eq!(folder_path(&folders, None), None);
    }

    #[test]
    fn folder_path_returns_none_for_missing_id() {
        let folders = vec![folder(1, "A", None)];
        assert_eq!(folder_path(&folders, Some(999)), None);
    }

    #[test]
    fn folder_path_joins_breadcrumb() {
        let folders = vec![
            folder(1, "Work", None),
            folder(2, "Projects", Some(1)),
            folder(3, "Lore", Some(2)),
        ];
        assert_eq!(
            folder_path(&folders, Some(3)),
            Some("Work / Projects / Lore".to_string()),
        );
    }

    #[test]
    fn folder_path_for_root_level_folder_is_name_only() {
        let folders = vec![folder(7, "Inbox", None)];
        assert_eq!(folder_path(&folders, Some(7)), Some("Inbox".to_string()));
    }
}

