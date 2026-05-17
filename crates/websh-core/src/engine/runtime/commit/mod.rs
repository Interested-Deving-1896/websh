use crate::domain::{ChangeSet, VirtualPath};
use crate::ports::{CommitOutcome, StorageBackendRef, StorageError};

mod delta;
mod prepare;
#[cfg(test)]
mod tests;

use prepare::prepare_commit;

pub type CommitResult<T> = Result<T, CommitError>;

#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    #[error(transparent)]
    Prepare(#[from] CommitPrepareError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CommitPrepareError {
    #[error("staged change {path} is outside commit root {mount_root}")]
    StagedPathOutsideMount {
        path: VirtualPath,
        mount_root: VirtualPath,
    },
    #[error("cannot delete commit root {mount_root}")]
    DeleteCommitRoot { mount_root: VirtualPath },
    #[error("binary changes are not supported yet: {path}")]
    UnsupportedBinaryChange { path: VirtualPath },
    #[error("commit delta has both addition and deletion for {path}")]
    DeltaConflict { path: VirtualPath },
    #[error("failed to assemble commit view: {source}")]
    AssembleCommitView {
        #[from]
        source: crate::filesystem::MountError,
    },
    #[error("missing mount root {mount_root}")]
    MissingMountRoot { mount_root: VirtualPath },
}

pub async fn commit_backend(
    backend: StorageBackendRef,
    mount_root: VirtualPath,
    changes: ChangeSet,
    message: String,
    expected_head: Option<String>,
    auth_token: Option<String>,
) -> CommitResult<CommitOutcome> {
    let request = prepare_commit(
        &backend,
        &mount_root,
        &changes,
        message,
        expected_head,
        auth_token,
    )
    .await?;
    backend.commit(&request).await.map_err(Into::into)
}
