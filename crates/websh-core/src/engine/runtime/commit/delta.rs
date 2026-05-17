use std::collections::BTreeSet;

use crate::domain::{ChangeSet, ChangeType, VirtualPath};
use crate::ports::{CommitDelta, CommitFileAddition, ScannedSubtree};

use super::CommitPrepareError;

pub(super) fn normalized_staged_changes(changes: &ChangeSet) -> ChangeSet {
    let deleted_dirs = delete_directory_paths(changes);
    let mut normalized = ChangeSet::new();

    for (path, entry) in changes.iter_staged() {
        if is_descendant_of_deleted_dir(path, &deleted_dirs) {
            continue;
        }
        normalized.upsert_at(path.clone(), entry.change.clone(), entry.timestamp);
    }

    normalized
}

pub(super) fn build_commit_delta(
    base_snapshot: &ScannedSubtree,
    mount_root: &VirtualPath,
    normalized_changes: &ChangeSet,
) -> Result<CommitDelta, CommitPrepareError> {
    let mut additions = Vec::new();
    let mut deletions = Vec::new();
    let deleted_dirs = delete_directory_paths(normalized_changes);

    for (path, entry) in normalized_changes.iter_staged() {
        match &entry.change {
            ChangeType::CreateFile { content, .. } | ChangeType::UpdateFile { content, .. } => {
                additions.push(CommitFileAddition {
                    path: path.clone(),
                    content: content.clone(),
                });
            }
            ChangeType::DeleteFile => {
                deletions.push(path.clone());
            }
            ChangeType::DeleteDirectory => {}
            ChangeType::CreateBinary { .. } => {
                return Err(CommitPrepareError::UnsupportedBinaryChange { path: path.clone() });
            }
            ChangeType::CreateDirectory { .. } => {}
        }
    }
    deletions.extend(deleted_files_for_directory_changes(
        base_snapshot,
        mount_root,
        &deleted_dirs,
    ));

    additions.sort_by(|left, right| left.path.cmp(&right.path));
    deletions.sort();
    deletions.dedup();

    let addition_paths = additions
        .iter()
        .map(|addition| addition.path.clone())
        .collect::<BTreeSet<_>>();
    if let Some(conflict) = deletions.iter().find(|path| addition_paths.contains(*path)) {
        return Err(CommitPrepareError::DeltaConflict {
            path: conflict.clone(),
        });
    }

    Ok(CommitDelta {
        additions,
        deletions,
    })
}

pub(super) fn staged_cleanup_paths(changes: &ChangeSet) -> Vec<VirtualPath> {
    let mut paths: Vec<_> = changes
        .iter_staged()
        .map(|(path, _)| path.clone())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

fn delete_directory_paths(changes: &ChangeSet) -> Vec<VirtualPath> {
    let mut paths = changes
        .iter_staged()
        .filter(|(_, entry)| matches!(entry.change, ChangeType::DeleteDirectory))
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();

    let mut collapsed = Vec::new();
    for path in paths {
        if collapsed
            .iter()
            .any(|parent| path != *parent && path.starts_with(parent))
        {
            continue;
        }
        collapsed.push(path);
    }
    collapsed
}

fn is_descendant_of_deleted_dir(path: &VirtualPath, deleted_dirs: &[VirtualPath]) -> bool {
    deleted_dirs
        .iter()
        .any(|deleted_dir| path != deleted_dir && path.starts_with(deleted_dir))
}

fn deleted_files_for_directory_changes(
    base_snapshot: &ScannedSubtree,
    mount_root: &VirtualPath,
    paths: &[VirtualPath],
) -> Vec<VirtualPath> {
    let mut deleted = Vec::new();
    let rel_dirs = paths
        .iter()
        .filter_map(|path| path.strip_prefix(mount_root))
        .collect::<Vec<_>>();

    if rel_dirs.is_empty() {
        return deleted;
    }

    for file in &base_snapshot.files {
        if rel_dirs.iter().any(|rel_dir| {
            rel_dir.is_empty()
                || file.path == *rel_dir
                || file
                    .path
                    .strip_prefix(rel_dir)
                    .is_some_and(|rest| rest.starts_with('/'))
        }) {
            deleted.push(mount_root.join(&file.path));
        }
    }

    deleted.sort();
    deleted.dedup();
    deleted
}
