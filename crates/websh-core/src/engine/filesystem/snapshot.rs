use std::collections::HashMap;

use crate::domain::{FsEntry, NodeMetadata, VirtualPath};
use crate::ports::{ScannedDirectory, ScannedFile, ScannedSubtree};

use super::tree::directory_metadata;

pub(super) fn scanned_subtree_root(snapshot: &ScannedSubtree) -> FsEntry {
    let dir_meta_map: HashMap<String, &ScannedDirectory> = snapshot
        .directories
        .iter()
        .map(|dir| (dir.path.clone(), dir))
        .collect();

    let mut children = HashMap::new();

    for file in &snapshot.files {
        insert_scanned_file(&mut children, file, &dir_meta_map);
    }

    for dir in &snapshot.directories {
        if !dir.path.is_empty() {
            ensure_scanned_directory(&mut children, &dir.path, &dir_meta_map);
        }
    }

    let root_meta = dir_meta_map
        .get("")
        .map(|dir| dir.meta.clone())
        .unwrap_or_default();

    FsEntry::Directory {
        children,
        meta: root_meta,
    }
}

fn insert_scanned_file(
    tree: &mut HashMap<String, FsEntry>,
    file: &ScannedFile,
    dir_meta_map: &HashMap<String, &ScannedDirectory>,
) {
    let parts: Vec<&str> = file
        .path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return;
    }

    let mut current = tree;
    let mut current_path = String::new();
    for (idx, part) in parts.iter().enumerate() {
        let is_last = idx == parts.len() - 1;
        if is_last {
            current.insert(
                (*part).to_string(),
                FsEntry::content_file_with_meta(
                    &file.path,
                    file.meta.clone(),
                    file.extensions.clone(),
                ),
            );
            return;
        }

        if !current_path.is_empty() {
            current_path.push('/');
        }
        current_path.push_str(part);

        let slot = current
            .entry((*part).to_string())
            .or_insert_with(|| scanned_directory_entry(&current_path, part, dir_meta_map));

        current = match slot {
            FsEntry::Directory { children, .. } => children,
            FsEntry::File { .. } => return,
        };
    }
}

fn ensure_scanned_directory(
    tree: &mut HashMap<String, FsEntry>,
    path: &str,
    dir_meta_map: &HashMap<String, &ScannedDirectory>,
) {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let mut current = tree;
    let mut current_path = String::new();

    for part in parts {
        if !current_path.is_empty() {
            current_path.push('/');
        }
        current_path.push_str(part);

        let slot = current
            .entry(part.to_string())
            .or_insert_with(|| scanned_directory_entry(&current_path, part, dir_meta_map));

        current = match slot {
            FsEntry::Directory { children, .. } => children,
            FsEntry::File { .. } => return,
        };
    }
}

fn scanned_directory_entry(
    path: &str,
    name: &str,
    dir_meta_map: &HashMap<String, &ScannedDirectory>,
) -> FsEntry {
    FsEntry::Directory {
        children: HashMap::new(),
        meta: dir_meta_map
            .get(path)
            .map(|dir| dir.meta.clone())
            .unwrap_or_else(|| directory_metadata(name)),
    }
}

pub(super) fn collect_scanned_files(
    mount_root: &VirtualPath,
    prefix: &str,
    children: &HashMap<String, FsEntry>,
    excluded_roots: &[VirtualPath],
    out: &mut Vec<ScannedFile>,
) {
    let mut names: Vec<&String> = children.keys().collect();
    names.sort();
    for name in names {
        let entry = &children[name];
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };
        let abs = mount_root.join(&rel);
        if path_is_excluded(&abs, excluded_roots) {
            continue;
        }
        match entry {
            FsEntry::File {
                content_path,
                meta,
                extensions,
            } => {
                if content_path.is_none() {
                    continue;
                }
                out.push(ScannedFile {
                    path: content_path
                        .as_ref()
                        .filter(|path| !path.is_empty())
                        .cloned()
                        .unwrap_or(rel),
                    meta: meta.clone(),
                    extensions: extensions.clone(),
                });
            }
            FsEntry::Directory { children, .. } => {
                collect_scanned_files(mount_root, &rel, children, excluded_roots, out);
            }
        }
    }
}

pub(super) fn collect_scanned_directories(
    mount_root: &VirtualPath,
    prefix: &str,
    children: &HashMap<String, FsEntry>,
    excluded_roots: &[VirtualPath],
    out: &mut Vec<ScannedDirectory>,
) {
    let mut names: Vec<&String> = children.keys().collect();
    names.sort();
    for name in names {
        let entry = &children[name];
        if let FsEntry::Directory {
            children: sub,
            meta,
        } = entry
        {
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            let abs = mount_root.join(&rel);
            if path_is_excluded(&abs, excluded_roots) {
                continue;
            }
            if exportable_children_empty(mount_root, &rel, sub, excluded_roots)
                || has_manifest_metadata(&rel, meta)
            {
                out.push(ScannedDirectory {
                    path: rel.clone(),
                    meta: meta.clone(),
                });
            }
            collect_scanned_directories(mount_root, &rel, sub, excluded_roots, out);
        }
    }
}

fn exportable_children_empty(
    mount_root: &VirtualPath,
    prefix: &str,
    children: &HashMap<String, FsEntry>,
    excluded_roots: &[VirtualPath],
) -> bool {
    !children.iter().any(|(name, _)| {
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };
        !path_is_excluded(&mount_root.join(&rel), excluded_roots)
    })
}

fn path_is_excluded(path: &VirtualPath, excluded_roots: &[VirtualPath]) -> bool {
    excluded_roots
        .iter()
        .any(|excluded| path.starts_with(excluded))
}

pub(super) fn has_manifest_metadata(path: &str, meta: &NodeMetadata) -> bool {
    if meta.is_bundle() || meta.bundle.is_some() {
        return true;
    }
    if meta.description().is_some()
        || meta.icon().is_some()
        || meta.thumbnail().is_some()
        || meta.tags().map(|t| !t.is_empty()).unwrap_or(false)
    {
        return true;
    }
    let title = meta.title().unwrap_or("");
    if path.is_empty() {
        return !title.is_empty();
    }
    let last_segment = path.rsplit('/').next().unwrap_or("");
    !title.is_empty() && title != last_segment
}
