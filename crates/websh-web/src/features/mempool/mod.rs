//! Mempool — pending content entries displayed above the chain on /ledger.
//!
//! UI-coupled surface only. Pure mempool logic (parsing, serialization,
//! form validation, paths, manifest-entry shape) lives in
//! `websh_core::mempool::*`.

mod commit;
mod component;
mod error;
mod loader;
mod model;

pub use commit::save_raw;
pub use component::Mempool;
pub use error::MempoolSaveError;
pub use loader::load_mempool_files;
pub use model::{
    LedgerFilterShape, LoadedMempoolFile, MempoolEntry, MempoolModel, build_mempool_model,
};
