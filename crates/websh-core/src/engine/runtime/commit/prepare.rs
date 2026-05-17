use crate::domain::{ChangeSet, ChangeType, VirtualPath};
use crate::engine::filesystem::GlobalFs;
use crate::engine::filesystem::merge;
use crate::ports::{CommitRequest, StorageBackendRef};

use super::delta::{build_commit_delta, normalized_staged_changes, staged_cleanup_paths};
use super::{CommitError, CommitPrepareError, CommitResult};

/// Compose a [`CommitRequest`] from the local change set + the remote
/// base snapshot.
///
/// `StorageBackend::commit_base` is intentionally separate from `scan()`:
/// a backend may serve normal reads from a cache, while commit preparation
/// needs a snapshot tied to the same optimistic-concurrency token as the
/// eventual write.
pub(super) async fn prepare_commit(
    backend: &StorageBackendRef,
    mount_root: &VirtualPath,
    changes: &ChangeSet,
    message: String,
    expected_head: Option<String>,
    auth_token: Option<String>,
) -> CommitResult<CommitRequest> {
    let staged_changes = changes.staged_subset();
    for (path, entry) in staged_changes.iter_staged() {
        if !path.starts_with(mount_root) {
            return Err(CommitPrepareError::StagedPathOutsideMount {
                path: path.clone(),
                mount_root: mount_root.clone(),
            }
            .into());
        }
        if path == mount_root && matches!(entry.change, ChangeType::DeleteDirectory) {
            return Err(CommitPrepareError::DeleteCommitRoot {
                mount_root: mount_root.clone(),
            }
            .into());
        }
    }

    let commit_base = backend
        .commit_base(expected_head, auth_token.clone())
        .await?;
    let base_snapshot = commit_base.snapshot;
    let mut merged = GlobalFs::empty();
    merged
        .mount_scanned_subtree(mount_root.clone(), &base_snapshot)
        .map_err(CommitPrepareError::from)?;

    let normalized_changes = normalized_staged_changes(&staged_changes);
    let cleanup_paths = staged_cleanup_paths(&staged_changes);
    let delta = build_commit_delta(&base_snapshot, mount_root, &normalized_changes)?;

    merge::apply_staged_changes_to_global_for_root(&mut merged, &normalized_changes, mount_root);

    let merged_snapshot = merged.export_mount_snapshot(mount_root).ok_or_else(|| {
        CommitError::from(CommitPrepareError::MissingMountRoot {
            mount_root: mount_root.clone(),
        })
    })?;

    Ok(CommitRequest {
        delta,
        cleanup_paths,
        merged_snapshot,
        message,
        expected_head: commit_base.expected_head,
        auth_token,
    })
}
