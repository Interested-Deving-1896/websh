use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::{DirEntry, FsEntry, NodeMetadata, RouteIndexEntry, VirtualPath};

use super::intent::{RenderIntent, build_render_intent};
use super::routing::{RouteRequest, RouteResolution, resolve_route};
use super::tree::directory_metadata;

mod export;
mod mount;
mod mutation;
mod query;
#[cfg(test)]
mod tests;

/// Error returned when assembling a global tree from mounted subtrees.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum MountError {
    #[error("mount root must be a directory")]
    RootMustBeDirectory,
    #[error("mount parent is a file: {path}")]
    ParentIsFile { path: VirtualPath },
    #[error("mount point is a file: {path}")]
    MountPointIsFile { path: VirtualPath },
    #[error("mount point is already occupied: {path}")]
    MountPointOccupied { path: VirtualPath },
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FsMutationError {
    #[error("filesystem root must be a directory")]
    RootMustBeDirectory,
    #[error("parent is a file: {path}")]
    ParentIsFile { path: VirtualPath },
    #[error("target is a directory: {path}")]
    TargetIsDirectory { path: VirtualPath },
    #[error("target is missing: {path}")]
    TargetMissing { path: VirtualPath },
}

/// Minimal engine trait for the canonical-path read surface.
pub trait FsEngine {
    fn stat(&self, path: &VirtualPath) -> Option<&FsEntry>;
    fn list(&self, path: &VirtualPath) -> Option<Vec<DirEntry>>;
    fn resolve_route(&self, request: &RouteRequest) -> Option<RouteResolution>;
    fn build_render_intent(&self, resolution: &RouteResolution) -> Option<RenderIntent>;
}

/// Global filesystem assembled from mounted subtrees plus local overlays.
#[derive(Clone, Debug)]
pub struct GlobalFs {
    root: FsEntry,
    mount_points: BTreeSet<VirtualPath>,
    pending_text: BTreeMap<VirtualPath, String>,
    route_index: BTreeMap<String, RouteIndexEntry>,
}

impl GlobalFs {
    pub fn empty() -> Self {
        Self {
            root: FsEntry::Directory {
                children: Default::default(),
                meta: directory_metadata(""),
            },
            mount_points: BTreeSet::new(),
            pending_text: BTreeMap::new(),
            route_index: BTreeMap::new(),
        }
    }

    pub fn mount_points(&self) -> impl Iterator<Item = &VirtualPath> {
        self.mount_points.iter()
    }

    /// Returns the unified metadata for the node at `path`, if any. The
    /// metadata lives directly inside the [`FsEntry`] so this is a tree
    /// lookup rather than a separate map.
    pub fn node_metadata(&self, path: &VirtualPath) -> Option<&NodeMetadata> {
        self.get_entry(path).map(|entry| entry.meta())
    }

    pub fn replace_route_index(&mut self, routes: impl IntoIterator<Item = RouteIndexEntry>) {
        self.route_index = routes
            .into_iter()
            .map(|entry| (entry.route.clone(), entry))
            .collect();
    }

    pub fn route_entry(&self, route: &str) -> Option<&RouteIndexEntry> {
        self.route_index.get(route)
    }

    pub fn read_pending_text(&self, path: &VirtualPath) -> Option<String> {
        self.pending_text.get(path).cloned()
    }
}

impl Default for GlobalFs {
    fn default() -> Self {
        Self::empty()
    }
}

impl FsEngine for GlobalFs {
    fn stat(&self, path: &VirtualPath) -> Option<&FsEntry> {
        self.get_entry(path)
    }

    fn list(&self, path: &VirtualPath) -> Option<Vec<DirEntry>> {
        self.list_dir(path)
    }

    fn resolve_route(&self, request: &RouteRequest) -> Option<RouteResolution> {
        resolve_route(self, request)
    }

    fn build_render_intent(&self, resolution: &RouteResolution) -> Option<RenderIntent> {
        build_render_intent(self, resolution)
    }
}
