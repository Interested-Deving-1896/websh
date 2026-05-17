//! In-memory filesystem engine: globally-mounted tree, render intent,
//! routing, content reads, and change-merge.

mod content;
mod content_routes;
mod global_fs;
mod intent;
pub(crate) mod merge;
mod routing;
mod snapshot;
mod tree;

pub use crate::domain::{NodeKind, RendererKind, TrustLevel};

pub use content::{BackendRegistry, ContentReadError, public_read_url, read_bytes, read_text};
pub use content_routes::{
    attestation_route_for_node_path, content_href_for_path, content_route_for_path,
};
pub use global_fs::{FsEngine, FsMutationError, GlobalFs, MountError};
pub use intent::{RenderIntent, build_render_intent, build_render_intent_with_preferred_locale};
pub use routing::{
    ResolvedKind, RouteFrame, RouteRequest, RouteResolution, RouteSurface, canonicalize_user_path,
    display_path_for, is_new_request_path, parent_request_path, request_path_for_canonical_path,
    resolve_route, route_cwd,
};
