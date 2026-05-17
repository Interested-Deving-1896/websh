//! Shared ports and DTOs for edge adapters.

mod manifest;
mod storage;

#[cfg(any(test, feature = "mock"))]
mod mock;

pub use manifest::{
    ManifestPathError, ManifestSnapshotError, ManifestSnapshotResult, parse_manifest_snapshot,
    serialize_manifest_snapshot,
};
pub use storage::{
    CommitBase, CommitDelta, CommitFileAddition, CommitOutcome, CommitRequest, LocalBoxFuture,
    ScannedDirectory, ScannedFile, ScannedSubtree, StorageBackend, StorageBackendRef, StorageError,
    StorageResult,
};

#[cfg(any(test, feature = "mock"))]
pub use mock::MockBackend;
