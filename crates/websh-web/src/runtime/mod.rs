//! Browser runtime services and adapter facades.
//!
//! UI features should call this module for browser-side runtime work instead
//! of reaching directly into core storage or runtime adapter internals.

pub(crate) mod content_cache;
pub(crate) mod drafts;
mod error;
pub(crate) mod github_backend;
pub(crate) mod idb;
pub(crate) mod loader;
pub(crate) mod mounts;
pub(crate) mod state;
pub(crate) mod storage_state;
mod system;
pub(crate) mod wallet;

pub use error::RuntimeLoadError;
pub use loader::RuntimeLoad;
pub use mounts::{MountEntry, MountLoadSet, MountLoadStatus, MountScanJob, MountScanResult};
pub use state::EnvironmentError;
pub use system::shell_execution_context;
pub use wallet::{ConnectOutcome, WalletError};
