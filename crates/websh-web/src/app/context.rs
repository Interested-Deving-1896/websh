//! Application-wide reactive context and state accessors.

use std::collections::BTreeMap;
use std::rc::Rc;

use futures_util::FutureExt;
use leptos::prelude::*;

use super::{RuntimeServiceError, TerminalState};
use crate::config::APP_NAME;
use crate::runtime::content_cache::{ContentTextCache, ContentTextCacheKey};
use crate::runtime::{self, RuntimeLoad};
use websh_core::domain::{
    ChangeSet, RuntimeMount, VirtualPath, WalletState, is_runtime_overlay_path,
};
use websh_core::filesystem::{ContentReadError, GlobalFs, display_path_for};
use websh_core::ports::{LocalBoxFuture, StorageBackendRef};
use websh_core::runtime::RuntimeStateSnapshot;

type TextReadResult = Result<String, ContentReadError>;
type SharedTextRead = futures_util::future::Shared<LocalBoxFuture<'static, TextReadResult>>;

/// Application-wide reactive context.
///
/// This context is provided at the root of the component tree and can be
/// accessed from any child component using `use_context::<AppContext>()`.
///
/// # Architecture
///
/// The URL is the single source of truth for navigation state.
/// AppContext only manages non-navigation state:
/// - **Filesystem**: Virtual filesystem for file operations
/// - **Terminal state**: Command history, output
/// - **Wallet state**: Connection status, address, ENS name
///
/// `Clone + Copy` because every field is a signal handle or a nested
/// signal-container struct — see the module-level convention note.
#[derive(Clone, Copy)]
pub struct AppContext {
    /// Global canonical filesystem tree loaded from mounted content.
    pub global_fs: RwSignal<GlobalFs>,
    /// Current canonical working directory for shell surfaces.
    pub cwd: RwSignal<VirtualPath>,
    /// Wallet connection state.
    pub wallet: RwSignal<WalletState>,
    /// Installed browser wallet event listener handles. Stored so listener
    /// closures are not leaked and setup stays idempotent.
    wallet_event_listeners:
        StoredValue<Option<runtime::wallet::WalletEventListeners>, LocalStorage>,

    /// Current visual palette, mirrored to `html[data-theme]`.
    pub theme: RwSignal<&'static str>,

    /// Terminal state (history, commands).
    pub terminal: TerminalState,

    /// Staged + working-tree edits awaiting commit.
    pub changes: RwSignal<ChangeSet>,
    /// IndexedDB draft hydration has completed. Draft persistence is gated on
    /// this so the initial empty ChangeSet cannot overwrite a stored draft.
    pub drafts_hydrated: RwSignal<bool>,
    /// Content filesystem with local `changes` overlaid.
    pub view_global_fs: Signal<Rc<GlobalFs>, LocalStorage>,
    /// System filesystem with content changes plus synthetic `/.websh/state`.
    pub system_global_fs: Signal<Rc<GlobalFs>, LocalStorage>,
    /// Backend registry keyed by canonical mount roots.
    backends: StoredValue<BTreeMap<VirtualPath, StorageBackendRef>, LocalStorage>,
    /// Successful backend text reads scoped to the current runtime generation.
    content_text_cache: StoredValue<ContentTextCache, LocalStorage>,
    /// Backend text reads already in flight, keyed like the text cache.
    content_text_inflight: StoredValue<BTreeMap<ContentTextCacheKey, SharedTextRead>, LocalStorage>,
    /// Runtime mount declarations, effective write status, and scan jobs.
    pub mounts: RwSignal<runtime::MountLoadSet, LocalStorage>,
    /// Remote HEAD registry keyed by canonical mount roots.
    pub remote_heads: RwSignal<BTreeMap<VirtualPath, String>>,
    /// Runtime generation used to ignore stale background mount scans.
    runtime_generation: StoredValue<u64, LocalStorage>,
    /// Browser-hydrated runtime state rendered under `/.websh/state`.
    pub runtime_state: RwSignal<RuntimeStateSnapshot>,

    /// When `Some(path)`, the `EditModal` is open editing that path. `None` = closed.
    pub editor_open: RwSignal<Option<websh_core::domain::VirtualPath>>,
}

impl AppContext {
    /// Creates a new application context with default state.
    ///
    /// All signals are initialized to their default values:
    /// - Terminal: Empty history
    /// - Wallet: Disconnected
    /// - Filesystem: Empty
    pub fn new() -> Self {
        super::RuntimeServices::install_browser_persistence();
        let initial_load = super::RuntimeServices::bootstrap_runtime_load();
        let global_fs = RwSignal::new(initial_load.global_fs);
        let changes = RwSignal::new(ChangeSet::new());
        let drafts_hydrated = RwSignal::new(false);
        let wallet = RwSignal::new(WalletState::default());
        let wallet_event_listeners = StoredValue::new_local(None);
        let runtime_state = RwSignal::new(super::RuntimeServices::runtime_state_snapshot());
        let view_global_fs = Signal::derive_local(move || {
            Rc::new(global_fs.with(|base| {
                changes.with(|cs| websh_core::runtime::build_content_view_global_fs(base, cs))
            }))
        });
        let system_global_fs = Signal::derive_local(move || {
            Rc::new(global_fs.with(|base| {
                changes.with(|cs| {
                    wallet.with(|ws| {
                        runtime_state
                            .with(|rs| websh_core::runtime::build_view_global_fs(base, cs, ws, rs))
                    })
                })
            }))
        });

        let backends: StoredValue<BTreeMap<VirtualPath, StorageBackendRef>, LocalStorage> =
            StoredValue::new_local(initial_load.backends);
        let content_text_cache = StoredValue::new_local(ContentTextCache::default());
        let content_text_inflight = StoredValue::new_local(BTreeMap::new());
        let mounts = RwSignal::new_local(initial_load.mounts);
        let remote_heads = RwSignal::new(initial_load.remote_heads);
        let runtime_generation = StoredValue::new_local(0_u64);
        let theme = RwSignal::new(crate::render::theme::initial_theme());

        let editor_open = RwSignal::new(None);

        Self {
            // Shared state
            global_fs,
            cwd: RwSignal::new(VirtualPath::root()),
            wallet,
            wallet_event_listeners,

            theme,

            // Terminal state
            terminal: TerminalState::new(),

            // Runtime filesystem/write state
            changes,
            drafts_hydrated,
            view_global_fs,
            system_global_fs,
            backends,
            content_text_cache,
            content_text_inflight,
            mounts,
            remote_heads,
            runtime_generation,
            runtime_state,

            // Editor state
            editor_open,
        }
    }

    pub fn runtime_mounts_snapshot(&self) -> Vec<RuntimeMount> {
        self.mounts.with(|mounts| mounts.effective_mounts())
    }

    pub fn wallet_event_listeners_installed(&self) -> bool {
        self.wallet_event_listeners
            .with_value(|listeners| listeners.is_some())
    }

    pub fn install_wallet_event_listeners(&self, listeners: runtime::wallet::WalletEventListeners) {
        self.wallet_event_listeners.set_value(Some(listeners));
    }

    pub fn mount_status_for(&self, root: &VirtualPath) -> Option<runtime::MountLoadStatus> {
        self.mounts.with(|mounts| mounts.status(root))
    }

    pub fn mount_is_loaded(&self, root: &VirtualPath) -> bool {
        self.mounts.with(|mounts| mounts.is_loaded(root))
    }

    /// Gets the current prompt string for display.
    ///
    /// Format: `{username}@{app_name}:{path}`
    ///
    /// The username is derived from the wallet state:
    /// - ENS name if available
    /// - Shortened address (0x1234...5678) if connected
    /// - "guest" if disconnected
    pub fn get_prompt(&self, cwd: &VirtualPath) -> String {
        let display_path = display_path_for(cwd);
        let username = self.wallet.get().display_name();
        format!("{}@{}:{}", username, APP_NAME, display_path)
    }

    /// Best-effort lookup for the backend responsible for a canonical path.
    /// Falls back to a parent mount via longest-prefix match — appropriate
    /// for *read* paths where missing a deeper mount means falling back to
    /// the parent's view is acceptable. **Do not use this for writes**: see
    /// `backend_for_mount_root` for the strict variant required by commits.
    pub fn backend_for_path(&self, path: &VirtualPath) -> Option<StorageBackendRef> {
        self.backends.with_value(|map| {
            map.iter()
                .filter(|(root, _)| path.starts_with(root))
                .max_by_key(|(root, _)| root.as_str().len())
                .map(|(_, backend)| backend.clone())
        })
    }

    /// Strict lookup for the backend whose mount root *exactly* matches the
    /// supplied root. Used by commit / write flows so that a write to
    /// `/mempool/...` cannot silently fall back to the parent `/` mount when
    /// `/mempool` itself is unregistered. Returns `None` when no backend is
    /// registered at exactly `root`.
    pub fn backend_for_mount_root(&self, root: &VirtualPath) -> Option<StorageBackendRef> {
        self.backends.with_value(|map| map.get(root).cloned())
    }

    pub async fn read_text(&self, path: &VirtualPath) -> Result<String, ContentReadError> {
        let generation = self.runtime_generation();
        let result = self.read_text_for_generation(path, generation).await;
        if self.runtime_generation() == generation {
            return result;
        }

        self.read_text_for_generation(path, self.runtime_generation())
            .await
    }

    async fn read_text_for_generation(
        &self,
        path: &VirtualPath,
        generation: u64,
    ) -> Result<String, ContentReadError> {
        let fs = self.view_fs_for_path(path);
        if let Some(text) = fs.read_pending_text(path) {
            return Ok(text);
        }

        let backends = self.backends.with_value(|map| map.clone());
        let cache_key = content_cache_key_for_path(generation, &backends, path)?;
        let mut cached = None;
        self.content_text_cache
            .update_value(|cache| cached = cache.get(&cache_key));
        if let Some(text) = cached {
            return Ok(text);
        }

        let mut read = None;
        self.content_text_inflight.update_value(|inflight| {
            if let Some(existing) = inflight.get(&cache_key) {
                read = Some(existing.clone());
                return;
            }

            let shared = shared_text_read(fs, backends, path.clone());
            inflight.insert(cache_key.clone(), shared.clone());
            read = Some(shared);
        });

        let result = read
            .expect("text read future must be installed before await")
            .await;

        if let Ok(text) = &result
            && self.runtime_generation() == generation
        {
            self.content_text_cache
                .update_value(|cache| cache.insert(cache_key.clone(), text.clone()));
        }
        self.content_text_inflight.update_value(|inflight| {
            let _ = inflight.remove(&cache_key);
        });
        result
    }

    pub async fn read_bytes(&self, path: &VirtualPath) -> Result<Vec<u8>, ContentReadError> {
        let fs = self.view_fs_for_path(path);
        let backends = self.backends.with_value(|map| map.clone());
        websh_core::filesystem::read_bytes(&fs, &backends, path).await
    }

    pub fn public_read_url(&self, path: &VirtualPath) -> Result<Option<String>, ContentReadError> {
        let fs = self.view_fs_for_path(path);
        let backends = self.backends.with_value(|map| map.clone());
        websh_core::filesystem::public_read_url(&fs, &backends, path)
    }

    fn view_fs_for_path(&self, path: &VirtualPath) -> Rc<GlobalFs> {
        if is_runtime_overlay_path(path) {
            self.system_global_fs.get()
        } else {
            self.view_global_fs.get()
        }
    }

    pub fn runtime_mount_for_path(&self, path: &VirtualPath) -> Option<RuntimeMount> {
        self.runtime_mounts_snapshot()
            .into_iter()
            .filter(|mount| mount.contains(path))
            .max_by_key(|mount| mount.root.as_str().len())
    }

    /// Best-effort lookup for the last known remote HEAD responsible for a
    /// canonical path. Reads untracked: callers (commit flows in
    /// `spawn_local`) want a one-shot snapshot, not a subscription.
    pub fn remote_head_for_path(&self, path: &VirtualPath) -> Option<String> {
        self.remote_heads.with_untracked(|map| {
            map.iter()
                .filter(|(root, _)| path.starts_with(root))
                .max_by_key(|(root, _)| root.as_str().len())
                .map(|(_, head)| head.clone())
        })
    }

    pub fn declared_mount_for_root(&self, root: &VirtualPath) -> Option<RuntimeMount> {
        self.mounts.with(|mounts| mounts.declared(root))
    }

    pub fn clear_text_cache(&self) {
        self.content_text_cache
            .update_value(ContentTextCache::clear);
        self.content_text_inflight.update_value(BTreeMap::clear);
    }

    pub fn evict_text_cache_path(&self, path: &VirtualPath) {
        self.content_text_cache
            .update_value(|cache| cache.evict_path(path));
        self.content_text_inflight
            .update_value(|inflight| evict_inflight_path(inflight, path));
    }

    pub fn evict_text_cache_mount(&self, mount_root: &VirtualPath) {
        self.content_text_cache
            .update_value(|cache| cache.evict_mount(mount_root));
        self.content_text_inflight
            .update_value(|inflight| evict_inflight_mount(inflight, mount_root));
    }

    pub fn runtime_generation(&self) -> u64 {
        self.runtime_generation.get_value()
    }

    pub fn mark_mount_loading(
        &self,
        root: &VirtualPath,
    ) -> Result<(RuntimeMount, u64), RuntimeServiceError> {
        let mut marked = None;
        self.mounts.update(|mounts| {
            marked = mounts.mark_loading(root);
        });
        marked.ok_or_else(|| RuntimeServiceError::MissingDeclaration { root: root.clone() })
    }

    pub(crate) fn mark_mount_failed(
        &self,
        root: &VirtualPath,
        error: impl Into<String>,
    ) -> Result<(), RuntimeServiceError> {
        let error = error.into();
        let mut marked = false;
        self.mounts.update(|mounts| {
            marked = mounts.mark_failed(root, error.clone());
        });
        if marked {
            Ok(())
        } else {
            Err(RuntimeServiceError::MissingDeclaration { root: root.clone() })
        }
    }

    pub fn apply_runtime_load(&self, load: RuntimeLoad) -> u64 {
        let generation = self.runtime_generation.get_value().saturating_add(1);
        self.runtime_generation.set_value(generation);
        self.global_fs.set(load.global_fs);
        self.backends.set_value(load.backends);
        self.mounts.set(load.mounts);
        self.remote_heads.set(load.remote_heads);
        generation
    }

    pub fn apply_mount_scan_result(
        &self,
        generation: u64,
        result: runtime::MountScanResult,
    ) -> Result<(), RuntimeServiceError> {
        if generation != self.runtime_generation() {
            return Ok(());
        }

        let root = result.mount.root.clone();
        let label = result.mount.label.clone();
        let epoch = result.epoch;
        if !self
            .mounts
            .with_untracked(|mounts| mounts.accepts_result(&root, epoch))
        {
            return Ok(());
        }

        match result.scan {
            Ok(scan) => {
                let total_files = scan.files.len();
                self.evict_text_cache_mount(&root);
                let mut global = self.global_fs.get_untracked();
                if let Err(error) = global.replace_scanned_subtree(root.clone(), &scan) {
                    let message = format!("mount {label}: {error}");
                    self.mounts.update(|mounts| {
                        mounts.mark_failed_if_current(&root, epoch, message.clone());
                    });
                    return Err(RuntimeServiceError::ReplaceScannedSubtree {
                        label,
                        source: error,
                    });
                }
                let failed_descendants = self
                    .mounts
                    .with_untracked(|mounts| mounts.failed_roots_under(&root));
                for failed_root in failed_descendants {
                    let _ = global.reserve_mount_point(failed_root);
                }
                self.global_fs.set(global);
                self.backends.update_value(|backends| {
                    backends.insert(root.clone(), result.backend);
                });
                self.mounts.update(|mounts| {
                    mounts.mark_loaded_if_current(&root, epoch, total_files);
                });
                Ok(())
            }
            Err(error) => {
                self.evict_text_cache_mount(&root);
                self.mounts.update(|mounts| {
                    mounts.mark_failed_if_current(&root, epoch, error.to_string());
                });
                Ok(())
            }
        }
    }
}

fn content_cache_key_for_path(
    generation: u64,
    backends: &BTreeMap<VirtualPath, StorageBackendRef>,
    path: &VirtualPath,
) -> Result<ContentTextCacheKey, ContentReadError> {
    let mount_root = backends
        .keys()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.as_str().len())
        .cloned()
        .ok_or_else(|| ContentReadError::NoBackend { path: path.clone() })?;
    let rel_path = path
        .strip_prefix(&mount_root)
        .ok_or_else(|| ContentReadError::PathOutsideBackendRoot {
            path: path.clone(),
            root: mount_root.clone(),
        })?
        .to_string();
    Ok(ContentTextCacheKey {
        generation,
        mount_root,
        rel_path,
    })
}

fn shared_text_read(
    fs: Rc<GlobalFs>,
    backends: BTreeMap<VirtualPath, StorageBackendRef>,
    path: VirtualPath,
) -> SharedTextRead {
    let read: LocalBoxFuture<'static, TextReadResult> =
        Box::pin(async move { websh_core::filesystem::read_text(&fs, &backends, &path).await });
    read.shared()
}

fn evict_inflight_path(
    inflight: &mut BTreeMap<ContentTextCacheKey, SharedTextRead>,
    path: &VirtualPath,
) {
    let keys = inflight
        .keys()
        .filter(|key| {
            path.starts_with(&key.mount_root)
                && path
                    .strip_prefix(&key.mount_root)
                    .is_some_and(|rel| rel == key.rel_path)
        })
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        inflight.remove(&key);
    }
}

fn evict_inflight_mount(
    inflight: &mut BTreeMap<ContentTextCacheKey, SharedTextRead>,
    mount_root: &VirtualPath,
) {
    let keys = inflight
        .keys()
        .filter(|key| &key.mount_root == mount_root)
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        inflight.remove(&key);
    }
}

impl Default for AppContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use gloo_timers::future::TimeoutFuture;
    use leptos::prelude::Owner;
    use std::cell::Cell;
    use std::rc::Rc;
    use wasm_bindgen_test::*;
    use websh_core::domain::{EntryExtensions, Fields, NodeKind, NodeMetadata, SCHEMA_VERSION};
    use websh_core::filesystem::MountError;
    use websh_core::ports::{
        CommitOutcome, CommitRequest, LocalBoxFuture, ScannedSubtree, StorageBackend,
        StorageBackendRef, StorageError, StorageResult,
    };

    wasm_bindgen_test_configure!(run_in_browser);

    struct CountingBackend {
        reads: Rc<Cell<u32>>,
        text: String,
        delay_ms: u32,
    }

    impl StorageBackend for CountingBackend {
        fn backend_type(&self) -> &'static str {
            "counting"
        }

        fn scan(&self) -> LocalBoxFuture<'_, StorageResult<ScannedSubtree>> {
            Box::pin(async { Ok(ScannedSubtree::default()) })
        }

        fn read_text<'a>(
            &'a self,
            _rel_path: &'a str,
        ) -> LocalBoxFuture<'a, StorageResult<String>> {
            self.reads.set(self.reads.get() + 1);
            let text = self.text.clone();
            let delay_ms = self.delay_ms;
            Box::pin(async move {
                if delay_ms > 0 {
                    TimeoutFuture::new(delay_ms).await;
                }
                Ok(text)
            })
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

    fn counting_backend(reads: Rc<Cell<u32>>, text: &str, delay_ms: u32) -> StorageBackendRef {
        Rc::new(CountingBackend {
            reads,
            text: text.to_string(),
            delay_ms,
        })
    }

    fn root_mount() -> RuntimeMount {
        RuntimeMount::new(
            VirtualPath::root(),
            "~",
            websh_core::domain::RuntimeBackendKind::GitHub,
            true,
        )
    }

    fn apply_loaded_root_backend(ctx: AppContext, backend: StorageBackendRef) {
        let mut backends = BTreeMap::new();
        backends.insert(VirtualPath::root(), backend);
        let mut mounts = runtime::MountLoadSet::empty();
        mounts.insert_loaded(root_mount(), 0);

        ctx.apply_runtime_load(RuntimeLoad {
            global_fs: GlobalFs::empty(),
            backends,
            remote_heads: BTreeMap::new(),
            total_files: 0,
            mounts,
        });
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
    async fn read_text_caches_backend_results_within_generation_and_pending_text_bypasses_cache() {
        let owner = Owner::new();
        let reads = Rc::new(Cell::new(0));
        let path = VirtualPath::from_absolute("/cached.txt").expect("path");

        let ctx = owner.with(|| {
            let ctx = AppContext::new();
            apply_loaded_root_backend(ctx, counting_backend(reads.clone(), "remote", 0));
            ctx.clear_text_cache();
            ctx
        });

        assert_eq!(ctx.read_text(&path).await.unwrap(), "remote");
        assert_eq!(ctx.read_text(&path).await.unwrap(), "remote");
        assert_eq!(reads.get(), 1);

        ctx.global_fs.update(|fs| {
            fs.upsert_file(
                path.clone(),
                "pending".to_string(),
                data_meta(),
                EntryExtensions::default(),
            );
        });

        assert_eq!(ctx.read_text(&path).await.unwrap(), "pending");
        assert_eq!(reads.get(), 1);
    }

    #[wasm_bindgen_test(async)]
    async fn concurrent_same_generation_read_text_calls_share_one_backend_request() {
        let owner = Owner::new();
        let reads = Rc::new(Cell::new(0));
        let path = VirtualPath::from_absolute("/shared.txt").expect("path");

        let ctx = owner.with(|| {
            let ctx = AppContext::new();
            apply_loaded_root_backend(ctx, counting_backend(reads.clone(), "shared", 20));
            ctx.clear_text_cache();
            ctx
        });

        let (left, right) = futures_util::join!(ctx.read_text(&path), ctx.read_text(&path));

        assert_eq!(left.unwrap(), "shared");
        assert_eq!(right.unwrap(), "shared");
        assert_eq!(reads.get(), 1);
    }

    #[wasm_bindgen_test(async)]
    async fn generation_change_retries_inflight_text_read_without_caching_stale_result() {
        let owner = Owner::new();
        let stale_reads = Rc::new(Cell::new(0));
        let fresh_reads = Rc::new(Cell::new(0));
        let path = VirtualPath::from_absolute("/generation.txt").expect("path");

        let ctx = owner.with(|| {
            let ctx = AppContext::new();
            apply_loaded_root_backend(ctx, counting_backend(stale_reads.clone(), "stale", 50));
            ctx.clear_text_cache();
            ctx
        });

        let fresh_backend = counting_backend(fresh_reads.clone(), "fresh", 0);
        let swap_generation = async move {
            while stale_reads.get() == 0 {
                TimeoutFuture::new(1).await;
            }
            apply_loaded_root_backend(ctx, fresh_backend);
        };

        let (value, ()) = futures_util::join!(ctx.read_text(&path), swap_generation);
        let value = value.expect("read should retry against current generation");
        assert_eq!(value, "fresh");
        assert_eq!(fresh_reads.get(), 1);

        assert_eq!(ctx.read_text(&path).await.unwrap(), "fresh");
        assert_eq!(fresh_reads.get(), 1);
    }

    #[wasm_bindgen_test]
    fn root_mount_status_tracks_root_runtime_loads() {
        let owner = Owner::new();
        owner.with(|| {
            let ctx = AppContext::new();
            let root = VirtualPath::root();

            assert!(matches!(
                ctx.mount_status_for(&root),
                Some(runtime::MountLoadStatus::Loading { .. })
            ));
            assert!(!ctx.mount_is_loaded(&root));

            let root_mount = root_mount();
            apply_loaded_root_backend(ctx, counting_backend(Rc::new(Cell::new(0)), "", 0));

            assert!(ctx.mount_is_loaded(&root));
            assert!(matches!(
                ctx.mount_status_for(&root),
                Some(runtime::MountLoadStatus::Loaded { .. })
            ));

            let mut failed_mounts = runtime::MountLoadSet::empty();
            failed_mounts.insert_declared_loading(root_mount);
            ctx.apply_runtime_load(RuntimeLoad {
                global_fs: GlobalFs::empty(),
                backends: BTreeMap::new(),
                remote_heads: BTreeMap::new(),
                total_files: 0,
                mounts: failed_mounts,
            });
            ctx.mark_mount_failed(&root, "manifest unavailable")
                .expect("root mount should be declared");

            assert!(matches!(
                ctx.mount_status_for(&root),
                Some(runtime::MountLoadStatus::Failed { error, .. })
                    if error == "manifest unavailable"
            ));
        });
    }

    #[wasm_bindgen_test]
    fn mount_apply_failure_marks_mount_failed() {
        let owner = Owner::new();
        owner.with(|| {
            let root = VirtualPath::from_absolute("/db").expect("root");
            let mount = RuntimeMount::new(
                root.clone(),
                "db",
                websh_core::domain::RuntimeBackendKind::GitHub,
                true,
            );
            let backend = counting_backend(Rc::new(Cell::new(0)), "", 0);

            let ctx = AppContext::new();
            let mut mounts = runtime::MountLoadSet::empty();
            mounts.insert_loading(mount.clone(), backend.clone());
            ctx.mounts.set(mounts);
            ctx.global_fs.update(|fs| {
                fs.upsert_file(
                    root.clone(),
                    "not a directory".to_string(),
                    data_meta(),
                    EntryExtensions::default(),
                );
            });

            let result = runtime::MountScanResult {
                mount,
                backend,
                epoch: 0,
                scan: Ok(ScannedSubtree::default()),
            };

            let error = ctx
                .apply_mount_scan_result(ctx.runtime_generation(), result)
                .expect_err("mount apply should fail");
            assert!(matches!(
                error,
                RuntimeServiceError::ReplaceScannedSubtree {
                    source: MountError::MountPointIsFile { .. },
                    ..
                }
            ));
            assert!(matches!(
                ctx.mount_status_for(&root),
                Some(runtime::MountLoadStatus::Failed { .. })
            ));
        });
    }
}
