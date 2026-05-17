//! Pure mempool domain — used by both the browser UI and the native CLI.
//!
//! This module owns frontmatter parsing/serialization, the compose form
//! and validation, path conventions for `/mempool/...`, and the canonical
//! manifest-entry shape. Anything that depends on `AppContext` or
//! `commit_backend` lives in the web mempool feature, not here.

mod categories;
mod form;
mod manifest_entry;
mod parse;
mod path;
mod serialize;

pub use categories::LEDGER_CATEGORIES;
pub use form::{ComposeError, ComposeForm, form_to_payload, validate_form};
pub use manifest_entry::{MempoolManifestState, build_mempool_manifest_state};
pub use parse::{
    MempoolFrontmatterError, RawMempoolMeta, category_for_mempool_path, parse_mempool_frontmatter,
    strip_frontmatter_block, transform_mempool_frontmatter,
};
pub use path::{MempoolPathError, derive_new_path, mempool_root, placeholder_frontmatter};
pub use serialize::{ComposePayload, serialize_mempool_file, slug_from_title};
