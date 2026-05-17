use websh_core::domain::VirtualPath;

use crate::app::{CommitServiceError, RuntimeServiceError};

#[derive(Debug, thiserror::Error)]
pub enum MempoolSaveError {
    #[error("draft already exists at {path} — pick a different slug")]
    DraftAlreadyExists { path: VirtualPath },
    #[error("mempool mount is still loading — try again in a moment")]
    MountLoading,
    #[error("mempool mount is unavailable — {message}")]
    MountFailed { message: String },
    #[error("mempool mount is not loaded")]
    MountMissing,
    #[error(
        "mempool mount is not registered — check that content/.websh/mounts/mempool.mount.json exists and content/manifest.json is up to date"
    )]
    BackendMissing,
    #[error("missing GitHub token for mempool commit")]
    MissingToken,
    #[error(transparent)]
    Commit(#[from] CommitServiceError),
    #[error("saved, but runtime reload after save failed: {source}")]
    RuntimeReload {
        #[from]
        source: RuntimeServiceError,
    },
}
