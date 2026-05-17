//! Data models and types for the application.

mod bundle;
mod changes;
mod filesystem;
mod manifest;
mod mempool;
mod metadata;
mod mount;
mod site;
mod virtual_path;
mod wallet;

pub use bundle::{
    BundleMetadata, BundleValidationError, BundleValidationResult, BundleVariant,
    validate_bundle_metadata, validate_bundle_metadata_with_targets,
    validate_bundle_route_collisions, validate_bundle_variant, validate_bundle_variant_id,
    validate_relative_bundle_path,
};
pub use changes::{ChangeSet, ChangeType, Entry as ChangeEntry, Summary as ChangeSummary};
pub use filesystem::{DirEntry, DisplayPermissions, EntryExtensions, FileType, FsEntry};
pub use manifest::{ContentManifestDocument, ContentManifestEntry};
pub use mempool::{MempoolFields, MempoolStatus, Priority};
#[cfg(test)]
pub(crate) use metadata::test_support;
pub use metadata::{
    AccessFilter, Fields, ImageDim, LinkRef, NodeKind, NodeMetadata, PageSize, Recipient,
    RendererKind, SCHEMA_VERSION, TrustLevel,
};
pub use mount::{
    BootstrapSiteSource, RuntimeBackendKind, RuntimeMount, RuntimeMountKind,
    is_runtime_overlay_path, runtime_state_root,
};
pub use site::{DerivedIndex, MountDeclaration, RouteIndexEntry};
pub use virtual_path::{VirtualPath, VirtualPathParseError};
pub use wallet::{WalletState, chain_name};
