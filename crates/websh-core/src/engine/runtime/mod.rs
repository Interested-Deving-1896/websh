//! Pure runtime orchestration helpers shared by the web app and CLI.

use crate::domain::{
    ChangeSet, EntryExtensions, Fields, NodeKind, NodeMetadata, RuntimeMount, SCHEMA_VERSION,
    VirtualPath, WalletState, runtime_state_root,
};
use crate::engine::filesystem::GlobalFs;

pub(crate) mod boot;
mod commit;
mod state;

pub use boot::{
    assemble_global_fs, bootstrap_global_fs, bootstrap_runtime_mount, seed_bootstrap_routes,
};
pub use commit::{CommitError, CommitPrepareError, CommitResult, commit_backend};
pub use state::RuntimeStateSnapshot;

pub fn build_content_view_global_fs(base: &GlobalFs, changes: &ChangeSet) -> GlobalFs {
    let mut merged = base.clone();
    crate::engine::filesystem::merge::apply_all_changes_to_global(&mut merged, changes);
    merged
}

pub fn build_view_global_fs(
    base: &GlobalFs,
    changes: &ChangeSet,
    wallet_state: &WalletState,
    runtime_state: &RuntimeStateSnapshot,
) -> GlobalFs {
    let mut merged = build_content_view_global_fs(base, changes);
    populate_runtime_state(&mut merged, changes, wallet_state, runtime_state);
    merged
}

fn populate_runtime_state(
    fs: &mut GlobalFs,
    changes: &ChangeSet,
    wallet_state: &WalletState,
    runtime_state: &RuntimeStateSnapshot,
) {
    let state_root = runtime_state_root().clone();
    fs.remove_subtree(&state_root);

    let dir = |title: &str| NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: NodeKind::Directory,
        bundle: None,
        authored: Fields {
            title: Some(title.to_string()),
            ..Fields::default()
        },
        derived: Fields::default(),
    };
    let data_file = || NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: NodeKind::Data,
        bundle: None,
        authored: Fields::default(),
        derived: Fields::default(),
    };

    fs.upsert_directory(state_root.clone(), dir("state"));
    fs.upsert_directory(state_root.join("env"), dir("env"));
    fs.upsert_directory(state_root.join("session"), dir("session"));
    fs.upsert_directory(state_root.join("wallet"), dir("wallet"));
    fs.upsert_directory(state_root.join("drafts"), dir("drafts"));

    for (key, value) in &runtime_state.env {
        fs.upsert_file(
            state_root.join(&format!("env/{key}")),
            value.clone(),
            data_file(),
            EntryExtensions::default(),
        );
    }

    if runtime_state.github_token_present {
        fs.upsert_file(
            state_root.join("session/github_token_present"),
            "1".to_string(),
            data_file(),
            EntryExtensions::default(),
        );
    }

    let wallet_session = if runtime_state.wallet_session {
        "1"
    } else {
        "0"
    }
    .to_string();
    fs.upsert_file(
        state_root.join("session/wallet_session"),
        wallet_session,
        data_file(),
        EntryExtensions::default(),
    );

    let wallet_json = serde_json::to_string_pretty(wallet_state).unwrap_or_default();
    fs.upsert_file(
        state_root.join("wallet/connection.json"),
        wallet_json,
        data_file(),
        EntryExtensions::default(),
    );

    let draft_json = serde_json::to_string_pretty(&changes.summary()).unwrap_or_default();
    fs.upsert_file(
        state_root.join("drafts/summary.json"),
        draft_json,
        data_file(),
        EntryExtensions::default(),
    );
}

pub fn writable_mount_for_path(
    mounts: &[RuntimeMount],
    path: &VirtualPath,
) -> Option<RuntimeMount> {
    mounts
        .iter()
        .filter(|mount| mount.contains(path))
        .max_by_key(|mount| mount.root.as_str().len())
        .cloned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::{ChangeSet, VirtualPath, WalletState, runtime_state_root};

    #[test]
    fn content_view_does_not_materialize_runtime_state_overlay() {
        let base = GlobalFs::empty();
        let changes = ChangeSet::new();

        let content = build_content_view_global_fs(&base, &changes);

        assert!(!content.exists(runtime_state_root()));
    }

    #[test]
    fn system_view_materializes_runtime_state_overlay() {
        let base = GlobalFs::empty();
        let changes = ChangeSet::new();
        let runtime_state = RuntimeStateSnapshot {
            env: BTreeMap::from([("USER".to_string(), "wonj".to_string())]),
            github_token_present: true,
            wallet_session: true,
        };

        let system =
            build_view_global_fs(&base, &changes, &WalletState::Disconnected, &runtime_state);

        assert!(system.exists(runtime_state_root()));
        assert!(system.exists(&VirtualPath::from_absolute("/.websh/state/env/USER").unwrap()));
        assert!(system.exists(
            &VirtualPath::from_absolute("/.websh/state/session/github_token_present").unwrap()
        ));
    }
}
