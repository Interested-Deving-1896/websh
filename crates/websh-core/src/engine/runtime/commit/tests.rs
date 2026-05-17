use std::rc::Rc;
use std::sync::Mutex;

use crate::domain::{ChangeType, EntryExtensions, Fields, NodeKind, NodeMetadata, SCHEMA_VERSION};
use crate::ports::{
    CommitBase, CommitRequest, LocalBoxFuture, ScannedFile, ScannedSubtree, StorageBackend,
    StorageBackendRef, StorageResult,
};

use super::*;

fn blank_meta() -> NodeMetadata {
    NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: NodeKind::Page,
        bundle: None,
        authored: Fields::default(),
        derived: Fields::default(),
    }
}

struct PrepareBackend {
    scan: Mutex<Option<ScannedSubtree>>,
}

struct CommitBaseBackend {
    base: Mutex<Option<CommitBase>>,
}

impl StorageBackend for PrepareBackend {
    fn backend_type(&self) -> &'static str {
        "prepare"
    }

    fn scan(&self) -> LocalBoxFuture<'_, StorageResult<ScannedSubtree>> {
        let scan = self.scan.lock().unwrap().take().unwrap_or_default();
        Box::pin(async move { Ok(scan) })
    }

    fn read_text<'a>(&'a self, _rel_path: &'a str) -> LocalBoxFuture<'a, StorageResult<String>> {
        Box::pin(async move { unreachable!("read unused") })
    }

    fn read_bytes<'a>(&'a self, _rel_path: &'a str) -> LocalBoxFuture<'a, StorageResult<Vec<u8>>> {
        Box::pin(async move { unreachable!("read unused") })
    }

    fn commit<'a>(
        &'a self,
        _request: &'a CommitRequest,
    ) -> LocalBoxFuture<'a, StorageResult<CommitOutcome>> {
        Box::pin(async move { unreachable!("commit unused") })
    }
}

impl StorageBackend for CommitBaseBackend {
    fn backend_type(&self) -> &'static str {
        "commit-base"
    }

    fn scan(&self) -> LocalBoxFuture<'_, StorageResult<ScannedSubtree>> {
        Box::pin(async move { unreachable!("commit preparation should use commit_base") })
    }

    fn commit_base(
        &self,
        _expected_head: Option<String>,
        _auth_token: Option<String>,
    ) -> LocalBoxFuture<'_, StorageResult<CommitBase>> {
        let base = self.base.lock().unwrap().take().unwrap_or_default();
        Box::pin(async move { Ok(base) })
    }

    fn read_text<'a>(&'a self, _rel_path: &'a str) -> LocalBoxFuture<'a, StorageResult<String>> {
        Box::pin(async move { unreachable!("read unused") })
    }

    fn read_bytes<'a>(&'a self, _rel_path: &'a str) -> LocalBoxFuture<'a, StorageResult<Vec<u8>>> {
        Box::pin(async move { unreachable!("read unused") })
    }

    fn commit<'a>(
        &'a self,
        _request: &'a CommitRequest,
    ) -> LocalBoxFuture<'a, StorageResult<CommitOutcome>> {
        Box::pin(async move { unreachable!("commit unused") })
    }
}

fn p(s: &str) -> VirtualPath {
    VirtualPath::from_absolute(s).unwrap()
}

fn upsert(changes: &mut ChangeSet, path: VirtualPath, change: ChangeType) {
    changes.upsert_at(path, change, 1234);
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_commit_contains_merged_staged_snapshot() {
    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree {
            files: vec![ScannedFile {
                path: "keep.md".to_string(),
                meta: blank_meta(),
                extensions: EntryExtensions::default(),
            }],
            directories: vec![],
        })),
    });
    let mut changes = ChangeSet::new();
    upsert(
        &mut changes,
        p("/new.md"),
        ChangeType::CreateFile {
            content: "new".to_string(),
            meta: blank_meta(),
            extensions: EntryExtensions::default(),
        },
    );
    let unstaged = p("/draft.md");
    upsert(
        &mut changes,
        unstaged.clone(),
        ChangeType::CreateFile {
            content: "draft".to_string(),
            meta: blank_meta(),
            extensions: EntryExtensions::default(),
        },
    );
    changes.unstage(&unstaged);

    let request = prepare_commit(
        &backend,
        &VirtualPath::root(),
        &changes,
        "msg".to_string(),
        Some("old".to_string()),
        None,
    )
    .await
    .unwrap();

    let paths: Vec<_> = request
        .merged_snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(paths, vec!["keep.md", "new.md"]);
    assert!(request.delta.deletions.is_empty());
    assert_eq!(request.delta.additions.len(), 1);
    assert_eq!(request.cleanup_paths, vec![p("/new.md")]);
    assert_eq!(request.expected_head.as_deref(), Some("old"));
    assert_eq!(request.auth_token, None);
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_commit_rejects_binary_changes_until_storage_supports_them() {
    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree::default())),
    });
    let mut changes = ChangeSet::new();
    upsert(
        &mut changes,
        p("/asset.bin"),
        ChangeType::CreateBinary {
            blob_id: "draft-blob".to_string(),
            mime: "application/octet-stream".to_string(),
            meta: blank_meta(),
            extensions: EntryExtensions::default(),
        },
    );

    let err = prepare_commit(
        &backend,
        &VirtualPath::root(),
        &changes,
        "binary".to_string(),
        None,
        None,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        CommitError::Prepare(CommitPrepareError::UnsupportedBinaryChange { path })
            if path.as_str() == "/asset.bin"
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_commit_uses_backend_commit_base_expected_head() {
    let backend: StorageBackendRef = Rc::new(CommitBaseBackend {
        base: Mutex::new(Some(CommitBase {
            expected_head: Some("fresh-head".to_string()),
            snapshot: ScannedSubtree {
                files: vec![ScannedFile {
                    path: "remote-only.md".to_string(),
                    meta: blank_meta(),
                    extensions: EntryExtensions::default(),
                }],
                directories: vec![],
            },
        })),
    });
    let mut changes = ChangeSet::new();
    upsert(
        &mut changes,
        p("/new.md"),
        ChangeType::CreateFile {
            content: "new".to_string(),
            meta: blank_meta(),
            extensions: EntryExtensions::default(),
        },
    );

    let request = prepare_commit(
        &backend,
        &VirtualPath::root(),
        &changes,
        "msg".to_string(),
        Some("stale-head".to_string()),
        Some("qa-token".to_string()),
    )
    .await
    .unwrap();

    assert_eq!(request.expected_head.as_deref(), Some("fresh-head"));
    let paths: Vec<_> = request
        .merged_snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(paths, vec!["new.md", "remote-only.md"]);
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_commit_rejects_staged_changes_outside_mount_root() {
    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree::default())),
    });
    let mut changes = ChangeSet::new();
    upsert(
        &mut changes,
        p("/other/new.md"),
        ChangeType::CreateFile {
            content: "db".to_string(),
            meta: blank_meta(),
            extensions: EntryExtensions::default(),
        },
    );

    let error = prepare_commit(
        &backend,
        &p("/db"),
        &changes,
        "msg".to_string(),
        Some("old".to_string()),
        None,
    )
    .await
    .expect_err("commit preparation must reject cross-mount staged changes");

    assert!(matches!(
        error,
        CommitError::Prepare(CommitPrepareError::StagedPathOutsideMount { path, mount_root })
            if path.as_str() == "/other/new.md" && mount_root.as_str() == "/db"
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_commit_rejects_deleting_commit_root() {
    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree {
            files: vec![ScannedFile {
                path: "a.md".to_string(),
                meta: blank_meta(),
                extensions: EntryExtensions::default(),
            }],
            directories: vec![],
        })),
    });
    let mut changes = ChangeSet::new();
    upsert(&mut changes, p("/db"), ChangeType::DeleteDirectory);

    let error = prepare_commit(
        &backend,
        &p("/db"),
        &changes,
        "delete db root".to_string(),
        Some("old".to_string()),
        None,
    )
    .await
    .expect_err("commit preparation must reject mount-root deletion");

    assert!(matches!(
        error,
        CommitError::Prepare(CommitPrepareError::DeleteCommitRoot { mount_root })
            if mount_root.as_str() == "/db"
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_commit_expands_directory_delete_to_descendant_files() {
    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree {
            files: vec![
                ScannedFile {
                    path: "docs/a.md".to_string(),
                    meta: blank_meta(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "docs/deep/b.md".to_string(),
                    meta: blank_meta(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "keep.md".to_string(),
                    meta: blank_meta(),
                    extensions: EntryExtensions::default(),
                },
            ],
            directories: vec![],
        })),
    });
    let mut changes = ChangeSet::new();
    upsert(&mut changes, p("/docs"), ChangeType::DeleteDirectory);

    let request = prepare_commit(
        &backend,
        &VirtualPath::root(),
        &changes,
        "msg".to_string(),
        Some("old".to_string()),
        None,
    )
    .await
    .unwrap();

    let paths: Vec<_> = request
        .delta
        .deletions
        .iter()
        .map(|path| path.as_str())
        .collect();
    assert_eq!(paths, vec!["/docs/a.md", "/docs/deep/b.md"]);
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_commit_delete_directory_suppresses_descendant_additions() {
    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree {
            files: vec![ScannedFile {
                path: "docs/a.md".to_string(),
                meta: blank_meta(),
                extensions: EntryExtensions::default(),
            }],
            directories: vec![],
        })),
    });
    let mut changes = ChangeSet::new();
    upsert(
        &mut changes,
        p("/docs/a.md"),
        ChangeType::UpdateFile {
            content: "new".to_string(),
            meta: None,
            extensions: None,
        },
    );
    upsert(&mut changes, p("/docs"), ChangeType::DeleteDirectory);

    let request = prepare_commit(
        &backend,
        &VirtualPath::root(),
        &changes,
        "msg".to_string(),
        Some("old".to_string()),
        None,
    )
    .await
    .unwrap();

    assert!(request.delta.additions.is_empty());
    assert_eq!(request.delta.deletions, vec![p("/docs/a.md")]);
    assert_eq!(request.cleanup_paths, vec![p("/docs"), p("/docs/a.md")]);
    assert!(request.merged_snapshot.files.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_commit_collapses_nested_directory_deletes() {
    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree {
            files: vec![
                ScannedFile {
                    path: "docs/a.md".to_string(),
                    meta: blank_meta(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "docs/deep/b.md".to_string(),
                    meta: blank_meta(),
                    extensions: EntryExtensions::default(),
                },
            ],
            directories: vec![],
        })),
    });
    let mut changes = ChangeSet::new();
    upsert(&mut changes, p("/docs"), ChangeType::DeleteDirectory);
    upsert(&mut changes, p("/docs/deep"), ChangeType::DeleteDirectory);

    let request = prepare_commit(
        &backend,
        &VirtualPath::root(),
        &changes,
        "msg".to_string(),
        Some("old".to_string()),
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        request.delta.deletions,
        vec![p("/docs/a.md"), p("/docs/deep/b.md")]
    );
}

fn meta_with_title(title: &str) -> NodeMetadata {
    NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: NodeKind::Page,
        bundle: None,
        authored: Fields {
            title: Some(title.to_string()),
            ..Fields::default()
        },
        derived: Fields::default(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn update_file_change_propagates_meta_into_exported_snapshot() {
    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree {
            files: vec![ScannedFile {
                path: "a.md".to_string(),
                meta: meta_with_title("old"),
                extensions: EntryExtensions::default(),
            }],
            directories: vec![],
        })),
    });
    let mut changes = ChangeSet::new();
    upsert(
        &mut changes,
        p("/a.md"),
        ChangeType::UpdateFile {
            content: "new body".to_string(),
            meta: Some(meta_with_title("new")),
            extensions: None,
        },
    );

    let request = prepare_commit(
        &backend,
        &VirtualPath::root(),
        &changes,
        "msg".to_string(),
        Some("old".to_string()),
        None,
    )
    .await
    .unwrap();

    let updated = request
        .merged_snapshot
        .files
        .iter()
        .find(|f| f.path == "a.md")
        .expect("exported file");
    assert_eq!(updated.meta.authored.title.as_deref(), Some("new"));
}

#[tokio::test(flavor = "current_thread")]
async fn update_file_change_propagates_extensions_into_exported_snapshot() {
    use crate::domain::{MempoolFields, MempoolStatus};

    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree {
            files: vec![ScannedFile {
                path: "mempool/foo.md".to_string(),
                meta: blank_meta(),
                extensions: EntryExtensions {
                    mempool: Some(MempoolFields {
                        status: MempoolStatus::Draft,
                        priority: None,
                        category: Some("writing".to_string()),
                    }),
                },
            }],
            directories: vec![],
        })),
    });
    let new_ext = EntryExtensions {
        mempool: Some(MempoolFields {
            status: MempoolStatus::Review,
            priority: None,
            category: Some("writing".to_string()),
        }),
    };
    let mut changes = ChangeSet::new();
    upsert(
        &mut changes,
        p("/mempool/foo.md"),
        ChangeType::UpdateFile {
            content: "body".to_string(),
            meta: None,
            extensions: Some(new_ext.clone()),
        },
    );

    let request = prepare_commit(
        &backend,
        &VirtualPath::root(),
        &changes,
        "msg".to_string(),
        Some("old".to_string()),
        None,
    )
    .await
    .unwrap();

    let updated = request
        .merged_snapshot
        .files
        .iter()
        .find(|f| f.path == "mempool/foo.md")
        .expect("exported file");
    let mp = updated
        .extensions
        .mempool
        .as_ref()
        .expect("mempool extensions");
    assert_eq!(mp.status, MempoolStatus::Review);
}

#[tokio::test(flavor = "current_thread")]
async fn update_file_with_none_meta_preserves_base_scan_meta() {
    let backend: StorageBackendRef = Rc::new(PrepareBackend {
        scan: Mutex::new(Some(ScannedSubtree {
            files: vec![ScannedFile {
                path: "a.md".to_string(),
                meta: meta_with_title("preserved"),
                extensions: EntryExtensions::default(),
            }],
            directories: vec![],
        })),
    });
    let mut changes = ChangeSet::new();
    upsert(
        &mut changes,
        p("/a.md"),
        ChangeType::UpdateFile {
            content: "new body".to_string(),
            meta: None,
            extensions: None,
        },
    );

    let request = prepare_commit(
        &backend,
        &VirtualPath::root(),
        &changes,
        "msg".to_string(),
        Some("old".to_string()),
        None,
    )
    .await
    .unwrap();

    let updated = request
        .merged_snapshot
        .files
        .iter()
        .find(|f| f.path == "a.md")
        .expect("exported file");
    assert_eq!(updated.meta.authored.title.as_deref(), Some("preserved"));
}
