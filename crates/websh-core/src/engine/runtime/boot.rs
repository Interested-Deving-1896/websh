//! Pure runtime-layer scaffolding: the canonical bootstrap mount and an
//! empty `GlobalFs` ready to receive scans. Compiles on every target so
//! host-side shell tests can rebuild the same fixtures the web runtime
//! sees on first boot.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use crate::domain::{BootstrapSiteSource, RuntimeBackendKind, RuntimeMount, VirtualPath};
use crate::engine::filesystem::{GlobalFs, MountError};
use crate::ports::ScannedSubtree;

pub fn bootstrap_runtime_mount(source: &BootstrapSiteSource) -> RuntimeMount {
    RuntimeMount::new(
        source.mount_root(),
        source.label(),
        RuntimeBackendKind::GitHub,
        source.writable,
    )
}

pub fn bootstrap_global_fs() -> GlobalFs {
    GlobalFs::empty()
}

pub fn seed_bootstrap_routes(_global: &mut GlobalFs) {
    // Shell is a reserved code route, not a filesystem app node.
}

pub fn assemble_global_fs(scans: &[(VirtualPath, ScannedSubtree)]) -> Result<GlobalFs, MountError> {
    let mut global = GlobalFs::empty();
    for (mount_root, scan) in scans {
        global.mount_scanned_subtree(mount_root.clone(), scan)?;
    }
    Ok(global)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{EntryExtensions, Fields, NodeKind, NodeMetadata, SCHEMA_VERSION};
    use crate::ports::{ScannedDirectory, ScannedFile};

    fn bootstrap_source() -> BootstrapSiteSource {
        BootstrapSiteSource {
            repo_with_owner: "example/site",
            branch: "main",
            content_root: "content",
            gateway: "self",
            writable: true,
        }
    }

    #[test]
    fn bootstrap_global_fs_has_root_directory_without_app_routes() {
        let global = bootstrap_global_fs();
        assert!(global.exists(&VirtualPath::root()));
        assert!(!global.exists(&VirtualPath::from_absolute("/websh.app").unwrap()));
        assert!(!global.exists(&VirtualPath::from_absolute("/fs.app").unwrap()));
    }

    #[test]
    fn bootstrap_runtime_mount_is_root() {
        let mount = bootstrap_runtime_mount(&bootstrap_source());
        assert_eq!(mount.root.as_str(), "/");
        assert_eq!(mount.label, "~");
        assert!(mount.writable);
    }

    fn file_meta(kind: NodeKind) -> NodeMetadata {
        NodeMetadata {
            schema: SCHEMA_VERSION,
            kind,
            bundle: None,
            authored: Fields::default(),
            derived: Fields::default(),
        }
    }

    fn dir_meta(name: &str) -> NodeMetadata {
        NodeMetadata {
            schema: SCHEMA_VERSION,
            kind: NodeKind::Directory,
            bundle: None,
            authored: Fields {
                title: if name.is_empty() {
                    None
                } else {
                    Some(name.to_string())
                },
                ..Fields::default()
            },
            derived: Fields::default(),
        }
    }

    #[test]
    fn assembles_global_fs_under_canonical_mount_roots() {
        let scan = ScannedSubtree {
            files: vec![ScannedFile {
                path: "index.md".to_string(),
                meta: file_meta(NodeKind::Page),
                extensions: EntryExtensions::default(),
            }],
            directories: vec![ScannedDirectory {
                path: "".to_string(),
                meta: dir_meta("home"),
            }],
        };

        let fs =
            assemble_global_fs(&[(VirtualPath::root(), scan)]).expect("global fs should assemble");

        assert!(
            fs.get_entry(&VirtualPath::from_absolute("/index.md").unwrap())
                .is_some()
        );
    }
}
