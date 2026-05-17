use std::collections::HashMap;

use crate::domain::{
    DirEntry, Fields, FsEntry, NodeKind, NodeMetadata, SCHEMA_VERSION, VirtualPath,
};

use super::global_fs::FsMutationError;

pub(super) fn synthetic_directory(name: &str) -> FsEntry {
    FsEntry::Directory {
        children: Default::default(),
        meta: directory_metadata(name),
    }
}

/// Build a `NodeMetadata` describing a directory whose only authored
/// information is its display title.
pub(super) fn directory_metadata(name: &str) -> NodeMetadata {
    let title = if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    };
    NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: NodeKind::Directory,
        bundle: None,
        authored: Fields {
            title,
            ..Fields::default()
        },
        derived: Fields::default(),
    }
}

pub(super) fn collect_metadata_entries<'a>(
    base: &VirtualPath,
    entry: &'a FsEntry,
    out: &mut Vec<(VirtualPath, &'a NodeMetadata)>,
) {
    out.push((base.clone(), entry.meta()));
    if let FsEntry::Directory { children, .. } = entry {
        for (name, child) in children {
            collect_metadata_entries(&base.join(name), child, out);
        }
    }
}

pub(super) fn sorted_dir_entries(
    base: &VirtualPath,
    children: &HashMap<String, FsEntry>,
) -> Vec<DirEntry> {
    let mut items: Vec<_> = children
        .iter()
        .map(|(name, entry)| {
            let is_dir = entry.is_directory();
            let title = match entry {
                FsEntry::Directory { meta, .. } => {
                    meta.title().unwrap_or(name.as_str()).to_string()
                }
                FsEntry::File { meta, .. } => meta.title().unwrap_or(name.as_str()).to_string(),
            };
            DirEntry {
                name: name.clone(),
                path: base.join(name),
                is_dir,
                title,
                meta: Some(entry.meta().clone()),
            }
        })
        .collect();

    items.sort_by(|a, b| {
        let a_hidden = a.name.starts_with('.');
        let b_hidden = b.name.starts_with('.');

        match (a.is_dir, b.is_dir, a_hidden, b_hidden) {
            (true, false, _, _) => std::cmp::Ordering::Less,
            (false, true, _, _) => std::cmp::Ordering::Greater,
            (_, _, false, true) => std::cmp::Ordering::Less,
            (_, _, true, false) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    items
}

pub(super) fn insert_tree_entry(
    root: &mut FsEntry,
    path: &VirtualPath,
    entry: FsEntry,
) -> Result<(), FsMutationError> {
    let parts: Vec<&str> = path.segments().collect();
    let mut current = match root {
        FsEntry::Directory { children, .. } => children,
        FsEntry::File { .. } => return Err(FsMutationError::RootMustBeDirectory),
    };

    if parts.is_empty() {
        *root = entry;
        return Ok(());
    }

    for (idx, part) in parts.iter().enumerate() {
        let is_last = idx == parts.len() - 1;

        if is_last {
            current.insert((*part).to_string(), entry);
            return Ok(());
        }

        let slot = current
            .entry((*part).to_string())
            .or_insert_with(|| synthetic_directory(part));
        if matches!(slot, FsEntry::File { .. }) {
            let parent = VirtualPath::root().join(&parts[..=idx].join("/"));
            return Err(FsMutationError::ParentIsFile { path: parent });
        }
        current = match slot {
            FsEntry::Directory { children, .. } => children,
            FsEntry::File { .. } => unreachable!(),
        };
    }

    Ok(())
}

pub(super) fn remove_tree_entry(root: &mut FsEntry, path: &VirtualPath) {
    let parts: Vec<&str> = path.segments().collect();
    if parts.is_empty() {
        if let FsEntry::Directory { children, .. } = root {
            children.clear();
        }
        return;
    }

    let mut current = match root {
        FsEntry::Directory { children, .. } => children,
        FsEntry::File { .. } => return,
    };

    for part in &parts[..parts.len() - 1] {
        current = match current.get_mut(*part) {
            Some(FsEntry::Directory { children, .. }) => children,
            _ => return,
        };
    }

    current.remove(parts.last().copied().unwrap_or_default());
}

pub(super) fn get_tree_entry_mut<'a>(
    root: &'a mut FsEntry,
    path: &VirtualPath,
) -> Option<&'a mut FsEntry> {
    if path.is_root() {
        return Some(root);
    }

    let mut current = root;
    for part in path.segments() {
        current = match current {
            FsEntry::Directory { children, .. } => children.get_mut(part)?,
            FsEntry::File { .. } => return None,
        };
    }

    Some(current)
}
