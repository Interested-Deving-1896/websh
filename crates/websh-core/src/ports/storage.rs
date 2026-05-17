//! Storage backend port and DTOs shared by runtime engines and adapters.
//!
//! The current contract is intentionally local-task oriented:
//! [`StorageBackendRef`] is an `Rc<dyn StorageBackend>` and futures returned
//! by the trait are not `Send`. That matches the browser/WASM runtime and
//! keeps adapters cheap to clone. Native code that needs cross-thread storage
//! execution should wrap it at the adapter boundary rather than assuming this
//! core port is thread-safe.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use crate::domain::{EntryExtensions, NodeMetadata, VirtualPath};

use super::ManifestSnapshotError;

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StorageError {
    #[error("token invalid or lacks permission")]
    AuthFailed,
    #[error("remote changed; run `sync refresh`{}", conflict_suffix(remote_head))]
    Conflict { remote_head: Option<String> },
    #[error("path not found on remote: {path}")]
    NotFound { path: String },
    #[error("remote rejected request: {message}")]
    RemoteRejected { message: String },
    #[error("rate limited{}", rate_limit_suffix(retry_after))]
    RateLimited { retry_after: Option<u64> },
    #[error("remote server error: http {status}")]
    Server { status: u16 },
    #[error("network error: {message}")]
    Network { message: String },
    #[error("missing GitHub token")]
    MissingToken,
    #[error("invalid storage request: {message}")]
    InvalidRequest { message: String },
    #[error("invalid manifest snapshot: {message}")]
    InvalidSnapshot { message: String },
}

fn conflict_suffix(remote_head: &Option<String>) -> String {
    remote_head
        .as_ref()
        .map(|head| format!(" (remote head: {head})"))
        .unwrap_or_default()
}

fn rate_limit_suffix(retry_after: &Option<u64>) -> String {
    retry_after
        .map(|seconds| format!("; retry after {seconds}s"))
        .unwrap_or_default()
}

impl From<ManifestSnapshotError> for StorageError {
    fn from(source: ManifestSnapshotError) -> Self {
        Self::InvalidSnapshot {
            message: source.to_string(),
        }
    }
}

pub type LocalBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
pub type StorageBackendRef = Rc<dyn StorageBackend>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScannedSubtree {
    pub files: Vec<ScannedFile>,
    pub directories: Vec<ScannedDirectory>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitBase {
    pub snapshot: ScannedSubtree,
    pub expected_head: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: String,
    pub meta: NodeMetadata,
    pub extensions: EntryExtensions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScannedDirectory {
    pub path: String,
    pub meta: NodeMetadata,
}

#[derive(Debug)]
pub struct CommitOutcome {
    pub new_head: String,
    pub committed_paths: Vec<VirtualPath>,
}

#[derive(Clone, Debug)]
pub struct CommitFileAddition {
    pub path: VirtualPath,
    pub content: String,
}

#[derive(Clone, Debug, Default)]
pub struct CommitDelta {
    pub additions: Vec<CommitFileAddition>,
    pub deletions: Vec<VirtualPath>,
}

#[derive(Clone, Debug)]
pub struct CommitRequest {
    pub delta: CommitDelta,
    pub cleanup_paths: Vec<VirtualPath>,
    pub merged_snapshot: ScannedSubtree,
    pub message: String,
    pub expected_head: Option<String>,
    pub auth_token: Option<String>,
}

pub trait StorageBackend {
    fn backend_type(&self) -> &'static str;

    /// Scan the mount and return its current tree.
    fn scan(&self) -> LocalBoxFuture<'_, StorageResult<ScannedSubtree>>;

    /// Return the remote tree that commit preparation should merge against.
    ///
    /// Most backends can use the same path as [`StorageBackend::scan`].
    /// Backends with cached scan reads should override this to return a base
    /// tied to the same optimistic-concurrency token used for the commit.
    fn commit_base(
        &self,
        expected_head: Option<String>,
        _auth_token: Option<String>,
    ) -> LocalBoxFuture<'_, StorageResult<CommitBase>> {
        Box::pin(async move {
            Ok(CommitBase {
                snapshot: self.scan().await?,
                expected_head,
            })
        })
    }

    fn read_text<'a>(&'a self, rel_path: &'a str) -> LocalBoxFuture<'a, StorageResult<String>>;

    fn read_bytes<'a>(&'a self, rel_path: &'a str) -> LocalBoxFuture<'a, StorageResult<Vec<u8>>>;

    /// Return a browser-readable URL for a file when the backend can expose
    /// one directly. Backends that require authenticated/proxied reads should
    /// keep the default and let callers fall back to `read_text`/`read_bytes`.
    fn public_read_url(&self, _rel_path: &str) -> StorageResult<Option<String>> {
        Ok(None)
    }

    /// Commit one prepared atomic batch. Runtime code prepares the merged
    /// metadata snapshot so backend implementations do not assemble filesystems.
    fn commit<'a>(
        &'a self,
        request: &'a CommitRequest,
    ) -> LocalBoxFuture<'a, StorageResult<CommitOutcome>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_display_includes_remote_head_when_present() {
        let error = StorageError::Conflict {
            remote_head: Some("abc123".to_string()),
        };

        assert_eq!(
            error.to_string(),
            "remote changed; run `sync refresh` (remote head: abc123)"
        );
    }

    #[test]
    fn rate_limited_display_includes_retry_after_when_present() {
        let error = StorageError::RateLimited {
            retry_after: Some(30),
        };

        assert_eq!(error.to_string(), "rate limited; retry after 30s");
    }
}
