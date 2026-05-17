//! Async commit handlers for mempool authoring (browser write path).
//!
//! `save_raw` is the single browser-side mempool write path. It serves
//! both reader-page New (`is_new=true`) and Edit (`is_new=false`).
//! Validation and frontmatter parsing are advisory: the page calls
//! `derive_new_path` for new drafts; existing edits trust the user's
//! typed bytes. The manifest's authored + derived + mempool block are
//! always recomputed from the bytes via
//! `build_mempool_manifest_state`, so a status edit lands in the
//! manifest without going through compose's structured form.

use leptos::prelude::*;

use crate::app::AppContext;
use crate::app::RuntimeServices;
use websh_core::domain::{ChangeSet, ChangeType, VirtualPath};
use websh_core::mempool::{MempoolManifestState, build_mempool_manifest_state, mempool_root};

use super::MempoolSaveError;

/// Save raw markdown bytes (frontmatter included) to the mempool repo.
///
/// `is_new` controls whether the change emitted is `CreateFile` (new
/// entry) or `UpdateFile` (in-place edit). Both branches recompute the
/// manifest's authored + derived + mempool block from the bytes via
/// `build_mempool_manifest_state` — so a status edit, frontmatter title
/// change, or any other authored mutation lands in the manifest without
/// going through a structured compose form.
pub async fn save_raw(
    ctx: AppContext,
    path: VirtualPath,
    body: String,
    message: String,
    is_new: bool,
) -> Result<(), MempoolSaveError> {
    if is_new {
        let collides = ctx.view_global_fs.with_untracked(|fs| fs.exists(&path));
        if collides {
            return Err(MempoolSaveError::DraftAlreadyExists { path });
        }
    }

    let MempoolManifestState { meta, extensions } = build_mempool_manifest_state(&body, &path);

    let root = mempool_root();
    if !ctx.mount_is_loaded(root) {
        match ctx.mount_status_for(root) {
            Some(crate::runtime::MountLoadStatus::Loading { .. }) => {
                return Err(MempoolSaveError::MountLoading);
            }
            Some(crate::runtime::MountLoadStatus::Failed { error, .. }) => {
                return Err(MempoolSaveError::MountFailed { message: error });
            }
            Some(crate::runtime::MountLoadStatus::Loaded { .. }) => unreachable!(),
            None => return Err(MempoolSaveError::MountMissing),
        };
    }
    if ctx.backend_for_mount_root(root).is_none() {
        return Err(MempoolSaveError::BackendMissing);
    }

    let services = RuntimeServices::new(ctx);
    let token = services
        .github_token_for_commit()
        .ok_or(MempoolSaveError::MissingToken)?;

    let mut changes = ChangeSet::new();
    let change = if is_new {
        ChangeType::CreateFile {
            content: body,
            meta,
            extensions,
        }
    } else {
        ChangeType::UpdateFile {
            content: body,
            meta: Some(meta),
            extensions: Some(extensions),
        }
    };
    changes.upsert_at(path, change, crate::platform::current_timestamp());

    let outcome = services
        .commit_changes(root.clone(), changes, message, Some(token))
        .await?;
    services.record_commit_outcome(root, &outcome).await;

    services.reload_runtime_mount(root.clone()).await?;
    Ok(())
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use crate::app::RuntimeServiceError;
    use crate::runtime::{MountLoadSet, RuntimeLoad};
    use leptos::prelude::Owner;
    use std::collections::BTreeMap;
    use std::rc::Rc;
    use wasm_bindgen_test::*;
    use websh_core::domain::{
        EntryExtensions, Fields, NodeKind, NodeMetadata, RuntimeBackendKind, RuntimeMount,
        SCHEMA_VERSION,
    };
    use websh_core::filesystem::GlobalFs;
    use websh_core::ports::{
        CommitBase, CommitOutcome, CommitRequest, LocalBoxFuture, ScannedSubtree, StorageBackend,
        StorageBackendRef, StorageResult,
    };

    wasm_bindgen_test_configure!(run_in_browser);

    struct CommitThenReloadFailureBackend;

    impl StorageBackend for CommitThenReloadFailureBackend {
        fn backend_type(&self) -> &'static str {
            "test"
        }

        fn scan(&self) -> LocalBoxFuture<'_, StorageResult<ScannedSubtree>> {
            Box::pin(async { Ok(ScannedSubtree::default()) })
        }

        fn commit_base(
            &self,
            expected_head: Option<String>,
            _auth_token: Option<String>,
        ) -> LocalBoxFuture<'_, StorageResult<CommitBase>> {
            Box::pin(async move {
                Ok(CommitBase {
                    snapshot: ScannedSubtree::default(),
                    expected_head,
                })
            })
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
                Ok(CommitOutcome {
                    new_head: "new-head".to_string(),
                    committed_paths: Vec::new(),
                })
            })
        }
    }

    fn backend() -> StorageBackendRef {
        Rc::new(CommitThenReloadFailureBackend)
    }

    fn data_meta() -> NodeMetadata {
        NodeMetadata {
            schema: SCHEMA_VERSION,
            kind: NodeKind::Data,
            bundle: None,
            authored: Fields::default(),
            derived: Fields::default(),
        }
    }

    #[wasm_bindgen_test(async)]
    async fn save_raw_propagates_post_commit_reload_failure() {
        let owner = Owner::new();
        let ctx = owner.with(|| {
            let ctx = AppContext::new();
            let root = mempool_root().clone();
            let mount =
                RuntimeMount::new(root.clone(), "mempool", RuntimeBackendKind::GitHub, true);
            let mut global_fs = GlobalFs::empty();
            global_fs.upsert_file(
                root.clone(),
                "not a directory".to_string(),
                data_meta(),
                EntryExtensions::default(),
            );
            let mut backends = BTreeMap::new();
            backends.insert(root.clone(), backend());
            let mut mounts = MountLoadSet::empty();
            mounts.insert_loaded(mount, 1);
            ctx.apply_runtime_load(RuntimeLoad {
                global_fs,
                backends,
                remote_heads: BTreeMap::new(),
                total_files: 1,
                mounts,
            });

            RuntimeServices::new(ctx)
                .set_github_token("ghp_test")
                .expect("token should persist");
            ctx
        });

        let path = VirtualPath::from_absolute("/mempool/writing/reload.md").expect("path");
        let err = save_raw(
            ctx,
            path,
            "---\ntitle: Reload\ncategory: writing\n---\nbody\n".to_string(),
            "save draft".to_string(),
            true,
        )
        .await
        .expect_err("reload failure should be returned");

        assert!(matches!(
            err,
            MempoolSaveError::RuntimeReload {
                source: RuntimeServiceError::ReplaceScannedSubtree { .. }
            }
        ));
        assert!(
            err.to_string()
                .starts_with("saved, but runtime reload after save failed:")
        );
    }
}
