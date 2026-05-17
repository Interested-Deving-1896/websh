//! Application-owned runtime services.

use leptos::prelude::*;
use std::collections::BTreeMap;
use wasm_bindgen_futures::spawn_local;
use websh_core::attestation::ledger::CONTENT_LEDGER_CONTENT_PATH;
use websh_core::domain::{ChangeSet, VirtualPath, WalletState};
use websh_core::ports::{CommitOutcome, StorageBackendRef};
use websh_core::runtime::{self as core_runtime, RuntimeStateSnapshot};

use crate::render::theme;
use crate::runtime::{drafts, loader, state, storage_state, wallet};

use super::{
    AppContext, CommitServiceError, CommitServiceResult, RuntimeServiceError, RuntimeServiceResult,
    ThemeError,
};
use crate::runtime::loader::RuntimeLoad;
use crate::runtime::mounts::{MountLoadStatus, MountScanJob, MountScanResult};
use crate::runtime::{EnvironmentError, RuntimeLoadError};

#[derive(Clone, Copy)]
pub struct RuntimeServices {
    ctx: AppContext,
}

impl RuntimeServices {
    pub fn new(ctx: AppContext) -> Self {
        Self { ctx }
    }

    pub fn install_browser_persistence() {
        state::install_browser_persistence();
    }

    pub fn runtime_state_snapshot() -> RuntimeStateSnapshot {
        state::snapshot()
    }

    pub fn bootstrap_runtime_load() -> RuntimeLoad {
        loader::bootstrap_runtime_load()
    }

    pub fn refresh_runtime_state(&self) {
        self.ctx.runtime_state.set(state::snapshot());
    }

    pub fn init_default_env(&self) {
        state::install_browser_persistence();
        state::init_default_env();
        self.refresh_runtime_state();
    }

    pub fn set_env_var(&self, key: &str, value: &str) -> Result<(), EnvironmentError> {
        let snapshot = state::set_env_var(key, value)?;
        self.ctx.runtime_state.set(snapshot);
        Ok(())
    }

    pub fn unset_env_var(&self, key: &str) -> Result<(), EnvironmentError> {
        let snapshot = state::unset_env_var(key)?;
        self.ctx.runtime_state.set(snapshot);
        Ok(())
    }

    pub fn set_theme(&self, raw_theme: &str) -> Result<&'static str, ThemeError> {
        let Some(theme_id) = theme::normalize_theme_id(raw_theme) else {
            return Err(ThemeError::unknown(raw_theme));
        };
        let snapshot = state::set_env_var("THEME", theme_id)
            .map_err(|source| ThemeError::Persist { theme_id, source })?;
        if self.ctx.theme.get_untracked() != theme_id {
            self.ctx.theme.set(theme_id);
        }
        self.ctx.runtime_state.set(snapshot);
        theme::apply_theme_to_document(theme_id);
        Ok(theme_id)
    }

    pub fn set_github_token(&self, token: &str) -> Result<(), EnvironmentError> {
        let snapshot = state::set_github_token(token)?;
        self.ctx.runtime_state.set(snapshot);
        Ok(())
    }

    pub fn clear_github_token(&self) -> Result<(), EnvironmentError> {
        let snapshot = state::clear_github_token()?;
        self.ctx.runtime_state.set(snapshot);
        Ok(())
    }

    pub fn github_token_for_commit(&self) -> Option<String> {
        state::github_token_for_commit()
    }

    pub async fn reload_runtime(&self) -> RuntimeServiceResult {
        self.ctx.clear_text_cache();
        self.mark_root_mount_loading();
        let load = match self.load_runtime().await {
            Ok(load) => load,
            Err(error) => {
                self.apply_failed_root_mount_load(error.to_string());
                return Err(error.into());
            }
        };
        let jobs = load.mounts.scan_jobs.clone();
        let generation = self.apply_successful_root_mount_load(load);
        self.start_mount_scans(generation, jobs);
        Ok(())
    }

    pub async fn reload_runtime_mount(&self, mount_root: VirtualPath) -> RuntimeServiceResult {
        if mount_root.is_root() {
            return self.reload_runtime().await;
        }

        self.ctx.evict_text_cache_mount(&mount_root);
        let backend = self
            .ctx
            .backend_for_mount_root(&mount_root)
            .ok_or_else(|| {
                RuntimeServiceError::Commit(CommitServiceError::NoBackend {
                    mount_root: mount_root.clone(),
                })
            })?;
        let generation = self.ctx.runtime_generation();
        let (declared_mount, epoch) = self.ctx.mark_mount_loading(&mount_root)?;
        let result = loader::scan_mount(MountScanJob {
            mount: declared_mount,
            backend,
            epoch,
        })
        .await;
        self.apply_mount_scan_result(generation, result)?;
        Ok(())
    }

    pub async fn load_runtime(&self) -> Result<RuntimeLoad, RuntimeLoadError> {
        let mut load = loader::reload_runtime().await?;
        load.remote_heads = hydrate_remote_heads(&load.mounts.effective_mounts()).await;
        Ok(load)
    }

    pub fn apply_runtime_load(&self, load: RuntimeLoad) -> u64 {
        self.ctx.apply_runtime_load(load)
    }

    pub(crate) fn apply_successful_root_mount_load(&self, load: RuntimeLoad) -> u64 {
        let generation = self.apply_runtime_load(load);
        self.start_ledger_prefetch(generation);
        generation
    }

    pub(crate) fn mark_root_mount_loading(&self) {
        let root = VirtualPath::root();
        if let Err(error) = self.ctx.mark_mount_loading(&root) {
            leptos::logging::warn!("runtime: failed to mark root mount loading: {error}");
        }
    }

    fn mark_root_mount_failed(&self, error: impl Into<String>) {
        let root = VirtualPath::root();
        if let Err(error) = self.ctx.mark_mount_failed(&root, error) {
            leptos::logging::warn!("runtime: failed to mark root mount failed: {error}");
        }
    }

    pub(crate) fn apply_failed_root_mount_load(&self, error: impl Into<String>) -> u64 {
        let generation = self.apply_runtime_load(loader::bootstrap_runtime_load());
        self.mark_root_mount_failed(error);
        generation
    }

    pub fn start_mount_scans(&self, generation: u64, jobs: Vec<MountScanJob>) {
        for job in jobs {
            let services = *self;
            spawn_local(async move {
                let result = loader::scan_mount(job).await;
                if let Err(error) = services.apply_mount_scan_result(generation, result) {
                    leptos::logging::warn!("runtime: mount apply failed: {error}");
                }
            });
        }
    }

    pub fn apply_mount_scan_result(
        &self,
        generation: u64,
        result: MountScanResult,
    ) -> RuntimeServiceResult {
        self.ctx.apply_mount_scan_result(generation, result)
    }

    fn start_ledger_prefetch(&self, generation: u64) {
        let ctx = self.ctx;
        let path = VirtualPath::from_absolute(format!("/{CONTENT_LEDGER_CONTENT_PATH}"))
            .expect("ledger path is absolute");
        let root = VirtualPath::root();
        if !matches!(
            ctx.mount_status_for(&root),
            Some(MountLoadStatus::Loaded { .. })
        ) {
            return;
        }
        if !ctx.view_global_fs.with_untracked(|fs| fs.exists(&path)) {
            return;
        }

        spawn_local(async move {
            if ctx.runtime_generation() != generation {
                return;
            }

            let _ = ctx.read_text(&path).await;
            if ctx.runtime_generation() != generation {
                ctx.evict_text_cache_path(&path);
            }
        });
    }

    pub fn set_wallet_session(&self, active: bool) -> Result<(), EnvironmentError> {
        let snapshot = state::set_wallet_session(active)?;
        self.ctx.runtime_state.set(snapshot);
        Ok(())
    }

    pub fn wallet_available(&self) -> bool {
        wallet::is_available()
    }

    pub fn has_wallet_session(&self) -> bool {
        state::has_wallet_session()
    }

    pub async fn wallet_account(&self) -> Option<String> {
        wallet::get_account().await
    }

    pub async fn wallet_chain_id(&self) -> Option<u64> {
        wallet::get_chain_id().await
    }

    pub async fn resolve_wallet_ens(&self, address: &str) -> Option<String> {
        wallet::resolve_ens(address).await
    }

    pub async fn connect_wallet_with_session(
        &self,
    ) -> Result<wallet::ConnectOutcome, wallet::WalletError> {
        if !self.wallet_available() {
            return Err(wallet::WalletError::NotInstalled);
        }
        self.ctx.wallet.set(WalletState::Connecting);

        let address = match wallet::connect().await {
            Ok(addr) => addr,
            Err(err) => {
                self.ctx.wallet.set(WalletState::Disconnected);
                return Err(err);
            }
        };

        let session_persist_error = self.set_wallet_session(true).err();

        let chain_id = self.wallet_chain_id().await;
        self.ctx.wallet.set(WalletState::Connected {
            address: address.clone(),
            ens_name: None,
            chain_id,
        });

        let ens_name = self.resolve_wallet_ens(&address).await;
        if ens_name.is_some() {
            self.ctx.wallet.set(WalletState::Connected {
                address: address.clone(),
                ens_name: ens_name.clone(),
                chain_id,
            });
        }

        Ok(wallet::ConnectOutcome {
            address,
            chain_id,
            ens_name,
            session_persist_error,
        })
    }

    pub fn restore_wallet_session(
        &self,
        address: String,
        chain_id: Option<u64>,
        ens_name: Option<String>,
    ) -> Result<(), EnvironmentError> {
        self.ctx.wallet.set(WalletState::Connected {
            address,
            ens_name,
            chain_id,
        });
        self.set_wallet_session(true)
    }

    pub fn disconnect_wallet(&self) -> Result<(), EnvironmentError> {
        self.set_wallet_session(false)?;
        self.ctx.wallet.set(WalletState::Disconnected);
        Ok(())
    }

    pub fn install_wallet_event_listeners(&self) {
        if self.ctx.wallet_event_listeners_installed() {
            return;
        }

        let services_for_accounts = *self;
        let accounts_listener =
            match wallet::on_accounts_changed(move |account: Option<String>| match account {
                Some(new_addr) => {
                    services_for_accounts.ctx.wallet.update(|w| {
                        if let WalletState::Connected { chain_id, .. } = w {
                            *w = WalletState::Connected {
                                address: new_addr,
                                ens_name: None,
                                chain_id: *chain_id,
                            };
                        }
                    });
                }
                None => {
                    let _ = services_for_accounts.disconnect_wallet();
                }
            }) {
                Ok(listener) => listener,
                Err(error) => {
                    leptos::logging::warn!("wallet: account listener unavailable: {error}");
                    return;
                }
            };

        let services_for_chain = *self;
        let chain_listener = match wallet::on_chain_changed(move |chain_id_hex: String| {
            let new_chain_id = u64::from_str_radix(chain_id_hex.trim_start_matches("0x"), 16).ok();

            services_for_chain.ctx.wallet.update(|w| {
                if let WalletState::Connected { chain_id, .. } = w {
                    *chain_id = new_chain_id;
                }
            });
        }) {
            Ok(listener) => listener,
            Err(error) => {
                leptos::logging::warn!("wallet: chain listener unavailable: {error}");
                return;
            }
        };

        self.ctx
            .install_wallet_event_listeners(wallet::WalletEventListeners::new(
                accounts_listener,
                chain_listener,
            ));
    }

    pub async fn hydrate_global_draft(&self) -> websh_core::ports::StorageResult<ChangeSet> {
        drafts::hydrate_global().await
    }

    pub fn schedule_global_draft(&self, changes: ChangeSet) {
        drafts::schedule_global(changes);
    }

    pub async fn commit_staged(
        &self,
        mount_root: VirtualPath,
        message: String,
    ) -> CommitServiceResult {
        let changes = self.ctx.changes.with_untracked(|changes| changes.clone());
        let auth_token = self.github_token_for_commit();
        let outcome = self
            .commit_changes(mount_root.clone(), changes, message, auth_token)
            .await?;
        self.record_commit_outcome(&mount_root, &outcome).await;
        Ok(outcome)
    }

    pub async fn commit_changes(
        &self,
        mount_root: VirtualPath,
        changes: ChangeSet,
        message: String,
        auth_token: Option<String>,
    ) -> CommitServiceResult {
        let backend = self.backend_for_mount_root(&mount_root)?;
        let expected_head = self.ctx.remote_head_for_path(&mount_root);
        core_runtime::commit_backend(
            backend,
            mount_root,
            changes,
            message,
            expected_head,
            auth_token,
        )
        .await
        .map_err(Into::into)
    }

    pub async fn record_commit_outcome(&self, mount_root: &VirtualPath, outcome: &CommitOutcome) {
        for path in &outcome.committed_paths {
            self.ctx.evict_text_cache_path(path);
        }
        self.ctx.evict_text_cache_mount(mount_root);

        self.ctx.remote_heads.update(|map| {
            map.insert(mount_root.clone(), outcome.new_head.clone());
        });

        let mounts = self.ctx.runtime_mounts_snapshot();
        let storage_id = storage_state::storage_id_for_mount_root(&mounts, mount_root);

        if let Err(error) = storage_state::persist_remote_head(&storage_id, &outcome.new_head).await
        {
            leptos::logging::warn!(
                "runtime: persist remote_head for {} failed: {error}",
                mount_root.as_str()
            );
        }
    }

    fn backend_for_mount_root(
        &self,
        mount_root: &VirtualPath,
    ) -> Result<StorageBackendRef, CommitServiceError> {
        self.ctx
            .backend_for_mount_root(mount_root)
            .ok_or_else(|| CommitServiceError::NoBackend {
                mount_root: mount_root.clone(),
            })
    }
}

async fn hydrate_remote_heads(
    runtime_mounts: &[websh_core::domain::RuntimeMount],
) -> BTreeMap<VirtualPath, String> {
    let mut out = BTreeMap::new();

    for mount in runtime_mounts {
        if let Ok(Some(head)) = storage_state::hydrate_remote_head(&mount.storage_id()).await {
            out.insert(mount.root.clone(), head);
        }
    }

    out
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use leptos::prelude::Owner;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn set_theme_persists_user_theme_and_runtime_snapshot() {
        let owner = Owner::new();
        owner.with(|| {
            let storage = web_sys::window()
                .and_then(|window| window.local_storage().ok().flatten())
                .expect("localStorage should be available");
            let _ = storage.remove_item(theme::STORAGE_KEY);

            let ctx = AppContext::new();
            let services = RuntimeServices::new(ctx);
            let theme_id = services.set_theme("dracula").expect("theme should apply");
            let document = web_sys::window()
                .and_then(|window| window.document())
                .expect("document should be available");
            let root = document
                .document_element()
                .expect("documentElement should be available");
            let meta = document
                .query_selector(r#"meta[name="theme-color"]"#)
                .ok()
                .flatten();

            assert_eq!(theme_id, "dracula");
            assert_eq!(ctx.theme.get_untracked(), "dracula");
            assert_eq!(root.get_attribute("data-theme").as_deref(), Some("dracula"));
            if let Some(meta) = meta {
                assert_eq!(meta.get_attribute("content").as_deref(), Some("#282a36"));
            }
            assert_eq!(
                storage.get_item(theme::STORAGE_KEY).unwrap().as_deref(),
                Some("dracula")
            );
            assert_eq!(
                ctx.runtime_state
                    .get_untracked()
                    .env
                    .get("THEME")
                    .map(String::as_str),
                Some("dracula")
            );

            let _ = storage.remove_item(theme::STORAGE_KEY);
        });
    }
}
