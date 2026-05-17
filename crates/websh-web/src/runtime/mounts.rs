use std::collections::BTreeMap;

use websh_core::domain::{RuntimeMount, VirtualPath};
use websh_core::ports::{ScannedSubtree, StorageBackendRef, StorageError};

#[derive(Clone)]
pub struct MountLoadSet {
    pub entries: BTreeMap<VirtualPath, MountEntry>,
    pub scan_jobs: Vec<MountScanJob>,
    rejected_entries: Vec<MountEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountEntry {
    pub declared: RuntimeMount,
    pub status: MountLoadStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountLoadStatus {
    Loading { epoch: u64 },
    Loaded { total_files: usize, epoch: u64 },
    Failed { error: String, epoch: u64 },
}

#[derive(Clone)]
pub struct MountScanJob {
    pub mount: RuntimeMount,
    pub backend: StorageBackendRef,
    pub epoch: u64,
}

pub struct MountScanResult {
    pub mount: RuntimeMount,
    pub backend: StorageBackendRef,
    pub epoch: u64,
    pub scan: Result<ScannedSubtree, StorageError>,
}

impl MountLoadSet {
    pub fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
            scan_jobs: Vec::new(),
            rejected_entries: Vec::new(),
        }
    }

    pub fn insert_loaded(&mut self, declared: RuntimeMount, total_files: usize) {
        let root = declared.root.clone();
        self.entries.insert(
            root,
            MountEntry {
                declared,
                status: MountLoadStatus::Loaded {
                    total_files,
                    epoch: 0,
                },
            },
        );
    }

    pub fn insert_loading(&mut self, declared: RuntimeMount, backend: StorageBackendRef) {
        let epoch = 0;
        let root = declared.root.clone();
        self.entries.insert(
            root,
            MountEntry {
                declared: declared.clone(),
                status: MountLoadStatus::Loading { epoch },
            },
        );
        self.scan_jobs.push(MountScanJob {
            mount: declared,
            backend,
            epoch,
        });
    }

    pub(crate) fn insert_declared_loading(&mut self, declared: RuntimeMount) {
        let root = declared.root.clone();
        self.entries.insert(
            root,
            MountEntry {
                declared,
                status: MountLoadStatus::Loading { epoch: 0 },
            },
        );
    }

    pub fn insert_failed(&mut self, declared: RuntimeMount, error: impl Into<String>) {
        let root = declared.root.clone();
        self.entries.insert(
            root,
            MountEntry {
                declared,
                status: MountLoadStatus::Failed {
                    error: error.into(),
                    epoch: 0,
                },
            },
        );
    }

    pub fn reject(&mut self, declared: RuntimeMount, error: impl Into<String>) {
        self.rejected_entries.push(MountEntry {
            declared,
            status: MountLoadStatus::Failed {
                error: error.into(),
                epoch: 0,
            },
        });
    }

    pub fn effective_mounts(&self) -> Vec<RuntimeMount> {
        self.entries
            .values()
            .map(MountEntry::effective_mount)
            .collect()
    }

    pub fn status(&self, root: &VirtualPath) -> Option<MountLoadStatus> {
        self.entries.get(root).map(|entry| entry.status.clone())
    }

    pub fn is_loaded(&self, root: &VirtualPath) -> bool {
        matches!(self.status(root), Some(MountLoadStatus::Loaded { .. }))
    }

    pub fn declared(&self, root: &VirtualPath) -> Option<RuntimeMount> {
        self.entries.get(root).map(|entry| entry.declared.clone())
    }

    pub fn failed_entries(&self) -> Vec<MountEntry> {
        self.entries
            .values()
            .chain(self.rejected_entries.iter())
            .filter(|entry| matches!(entry.status, MountLoadStatus::Failed { .. }))
            .cloned()
            .collect()
    }

    pub fn mark_loading(&mut self, root: &VirtualPath) -> Option<(RuntimeMount, u64)> {
        let entry = self.entries.get_mut(root)?;
        let epoch = entry.status.epoch().saturating_add(1);
        entry.status = MountLoadStatus::Loading { epoch };
        Some((entry.declared.clone(), epoch))
    }

    pub(crate) fn mark_failed(&mut self, root: &VirtualPath, error: impl Into<String>) -> bool {
        let Some(entry) = self.entries.get_mut(root) else {
            return false;
        };
        let epoch = entry.status.epoch();
        entry.status = MountLoadStatus::Failed {
            error: error.into(),
            epoch,
        };
        true
    }

    pub fn accepts_result(&self, root: &VirtualPath, epoch: u64) -> bool {
        self.entries
            .get(root)
            .is_some_and(|entry| entry.status.epoch() == epoch)
    }

    pub fn mark_loaded_if_current(
        &mut self,
        root: &VirtualPath,
        epoch: u64,
        total_files: usize,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(root) else {
            return false;
        };
        if entry.status.epoch() != epoch {
            return false;
        }
        entry.status = MountLoadStatus::Loaded { total_files, epoch };
        true
    }

    pub fn mark_failed_if_current(
        &mut self,
        root: &VirtualPath,
        epoch: u64,
        error: impl Into<String>,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(root) else {
            return false;
        };
        if entry.status.epoch() != epoch {
            return false;
        }
        entry.status = MountLoadStatus::Failed {
            error: error.into(),
            epoch,
        };
        true
    }

    pub fn failed_roots_under(&self, root: &VirtualPath) -> Vec<VirtualPath> {
        self.entries
            .iter()
            .filter(|(candidate, entry)| {
                *candidate != root
                    && candidate.starts_with(root)
                    && matches!(entry.status, MountLoadStatus::Failed { .. })
            })
            .map(|(candidate, _)| candidate.clone())
            .collect()
    }
}

impl MountEntry {
    pub fn effective_mount(&self) -> RuntimeMount {
        let mut mount = self.declared.clone();
        if !matches!(self.status, MountLoadStatus::Loaded { .. }) {
            mount.writable = false;
        }
        mount
    }

    pub fn error(&self) -> Option<&str> {
        match &self.status {
            MountLoadStatus::Failed { error, .. } => Some(error),
            MountLoadStatus::Loading { .. } | MountLoadStatus::Loaded { .. } => None,
        }
    }
}

impl MountLoadStatus {
    pub fn epoch(&self) -> u64 {
        match self {
            Self::Loading { epoch } | Self::Loaded { epoch, .. } | Self::Failed { epoch, .. } => {
                *epoch
            }
        }
    }
}

impl Default for MountLoadSet {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use std::rc::Rc;

    use super::*;
    use wasm_bindgen_test::*;
    use websh_core::domain::RuntimeBackendKind;
    use websh_core::ports::{
        CommitOutcome, CommitRequest, LocalBoxFuture, ScannedSubtree, StorageBackend, StorageError,
        StorageResult,
    };

    wasm_bindgen_test_configure!(run_in_browser);

    struct NoopBackend;

    impl StorageBackend for NoopBackend {
        fn backend_type(&self) -> &'static str {
            "noop"
        }

        fn scan(&self) -> LocalBoxFuture<'_, StorageResult<ScannedSubtree>> {
            Box::pin(async { Ok(ScannedSubtree::default()) })
        }

        fn read_text<'a>(
            &'a self,
            _rel_path: &'a str,
        ) -> LocalBoxFuture<'a, StorageResult<String>> {
            Box::pin(async { Ok(String::new()) })
        }

        fn read_bytes<'a>(
            &'a self,
            _rel_path: &'a str,
        ) -> LocalBoxFuture<'a, StorageResult<Vec<u8>>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn commit<'a>(
            &'a self,
            _request: &'a CommitRequest,
        ) -> LocalBoxFuture<'a, StorageResult<CommitOutcome>> {
            Box::pin(async {
                Err(StorageError::InvalidRequest {
                    message: "unused".to_string(),
                })
            })
        }
    }

    fn mount(root: &str, writable: bool) -> RuntimeMount {
        RuntimeMount::new(
            VirtualPath::from_absolute(root).expect("mount root"),
            root.trim_start_matches('/'),
            RuntimeBackendKind::GitHub,
            writable,
        )
    }

    #[wasm_bindgen_test]
    fn loading_and_failed_effective_mounts_are_read_only() {
        let mut set = MountLoadSet::empty();
        set.insert_loading(mount("/db", true), Rc::new(NoopBackend));
        set.insert_failed(mount("/bad", true), "unavailable");

        let effective = set.effective_mounts();
        assert!(effective.iter().all(|mount| !mount.writable));
    }

    #[wasm_bindgen_test]
    fn declared_loading_mount_does_not_queue_scan_job() {
        let mut set = MountLoadSet::empty();
        let root = VirtualPath::root();
        set.insert_declared_loading(RuntimeMount::new(
            root.clone(),
            "~",
            RuntimeBackendKind::GitHub,
            true,
        ));

        assert!(matches!(
            set.status(&root),
            Some(MountLoadStatus::Loading { .. })
        ));
        assert!(set.scan_jobs.is_empty());
    }

    #[wasm_bindgen_test]
    fn loaded_effective_mount_restores_declared_writability() {
        let mut set = MountLoadSet::empty();
        set.insert_loading(mount("/db", true), Rc::new(NoopBackend));
        let root = VirtualPath::from_absolute("/db").expect("root");
        assert!(set.mark_loaded_if_current(&root, 0, 3));

        let effective = set.effective_mounts();
        assert_eq!(effective[0].root, root);
        assert!(effective[0].writable);
    }

    #[wasm_bindgen_test]
    fn old_epoch_result_is_stale() {
        let mut set = MountLoadSet::empty();
        let root = VirtualPath::from_absolute("/db").expect("root");
        set.insert_loaded(mount("/db", true), 1);

        let (_, epoch) = set.mark_loading(&root).expect("mount should reload");

        assert_eq!(epoch, 1);
        assert!(!set.accepts_result(&root, 0));
        assert!(set.accepts_result(&root, 1));
    }
}
