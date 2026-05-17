use std::collections::BTreeMap;

use serde_json::Value;

use websh_core::domain::{
    DerivedIndex, MountDeclaration, RuntimeBackendKind, RuntimeMount, VirtualPath,
};
use websh_core::filesystem::{BackendRegistry, GlobalFs};
use websh_core::ports::StorageBackendRef;
use websh_core::runtime as core_runtime;
use websh_site::BOOTSTRAP_SITE;

use super::error::RuntimeLoadError;
use super::github_backend;
use super::mounts::{MountLoadSet, MountLoadStatus, MountScanJob, MountScanResult};

#[derive(Clone)]
pub struct RuntimeLoad {
    pub global_fs: GlobalFs,
    pub backends: BackendRegistry,
    pub remote_heads: BTreeMap<VirtualPath, String>,
    pub total_files: usize,
    pub mounts: MountLoadSet,
}

fn bootstrap_runtime_mounts() -> Vec<RuntimeMount> {
    vec![core_runtime::bootstrap_runtime_mount(&BOOTSTRAP_SITE)]
}

fn bootstrap_backends() -> BackendRegistry {
    let mut backends = BTreeMap::new();
    let mount = core_runtime::bootstrap_runtime_mount(&BOOTSTRAP_SITE);
    backends.insert(
        mount.root.clone(),
        github_backend::build_backend_for_bootstrap_site(&BOOTSTRAP_SITE),
    );
    backends
}

pub fn bootstrap_runtime_load() -> RuntimeLoad {
    let global_fs = core_runtime::bootstrap_global_fs();
    let total_files = count_files(&global_fs, &VirtualPath::root());
    let mut mounts = MountLoadSet::empty();
    for mount in bootstrap_runtime_mounts() {
        mounts.insert_declared_loading(mount);
    }
    RuntimeLoad {
        global_fs,
        backends: bootstrap_backends(),
        remote_heads: BTreeMap::new(),
        total_files,
        mounts,
    }
}

pub async fn load_runtime() -> Result<RuntimeLoad, RuntimeLoadError> {
    let mut backends = bootstrap_backends();
    let roots: Vec<_> = backends.keys().cloned().collect();
    let mut scans = Vec::new();

    for root in roots {
        let Some(backend) = backends.get(&root).cloned() else {
            continue;
        };
        // The bootstrap site backend is not best-effort: if it can't scan
        // the local manifest, the app has no usable filesystem at all.
        let scan = backend
            .scan()
            .await
            .map_err(|source| RuntimeLoadError::BootstrapMount {
                label: mount_label_for_root(&root),
                source,
            })?;
        scans.push((root, scan));
    }

    let mut global_fs = core_runtime::assemble_global_fs(&scans)
        .map_err(|source| RuntimeLoadError::AssembleGlobalFs { source })?;
    let root_total_files = count_files(&global_fs, &VirtualPath::root());
    let mut mounts = MountLoadSet::empty();
    for mount in bootstrap_runtime_mounts() {
        mounts.insert_loaded(mount, root_total_files);
    }
    apply_runtime_conventions(&mut global_fs, &mut backends, &mut mounts).await?;
    let total_files = count_files(&global_fs, &VirtualPath::root());

    Ok(RuntimeLoad {
        global_fs,
        backends,
        remote_heads: BTreeMap::new(),
        total_files,
        mounts,
    })
}

pub async fn reload_runtime() -> Result<RuntimeLoad, RuntimeLoadError> {
    load_runtime().await
}

pub async fn scan_mount(job: MountScanJob) -> MountScanResult {
    let scan = job.backend.scan().await;
    MountScanResult {
        mount: job.mount,
        backend: job.backend,
        epoch: job.epoch,
        scan,
    }
}

async fn apply_runtime_conventions(
    global: &mut GlobalFs,
    backends: &mut BackendRegistry,
    mounts: &mut MountLoadSet,
) -> Result<(), RuntimeLoadError> {
    core_runtime::seed_bootstrap_routes(global);
    load_site_json_if_present(global, backends).await?;

    let bootstrap_roots = bootstrap_runtime_mounts()
        .into_iter()
        .map(|mount| mount.root)
        .collect::<Vec<_>>();
    let stale_roots = backends
        .keys()
        .filter(|root| {
            !bootstrap_roots
                .iter()
                .any(|bootstrap_root| bootstrap_root == *root)
        })
        .cloned()
        .collect::<Vec<_>>();
    for stale_root in stale_roots {
        backends.remove(&stale_root);
        global.remove_subtree(&stale_root);
    }

    register_external_mounts(
        global,
        backends,
        mounts,
        load_mount_declarations(global, backends).await?,
        &bootstrap_roots,
    );

    load_route_index(global, backends).await?;
    core_runtime::seed_bootstrap_routes(global);
    Ok(())
}

struct ExternalMountCandidate {
    order: usize,
    mount: RuntimeMount,
    backend: Option<StorageBackendRef>,
    build_error: Option<String>,
}

struct FailedMountDeclaration {
    mount: RuntimeMount,
    error: String,
}

enum LoadedMountDeclaration {
    Parsed(MountDeclaration),
    Failed(FailedMountDeclaration),
}

#[derive(Clone)]
struct RejectedMount {
    error: String,
    kind: RejectedMountKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RejectedMountKind {
    Duplicate,
    Overlap,
}

fn register_external_mounts(
    global: &mut GlobalFs,
    backends: &mut BackendRegistry,
    mounts: &mut MountLoadSet,
    declarations: Vec<LoadedMountDeclaration>,
    bootstrap_roots: &[VirtualPath],
) {
    let candidates = external_mount_candidates(declarations, bootstrap_roots);
    let rejected_by_order = rejected_mount_candidates(&candidates);

    for candidate in candidates {
        if let Some(rejected) = rejected_by_order.get(&candidate.order) {
            match rejected.kind {
                RejectedMountKind::Duplicate => {
                    mounts.reject(candidate.mount, rejected.error.clone());
                }
                RejectedMountKind::Overlap => {
                    mounts.insert_failed(candidate.mount, rejected.error.clone());
                }
            }
            continue;
        }

        if let Some(error) = candidate.build_error {
            mounts.insert_failed(candidate.mount, error);
            continue;
        }

        let Some(backend) = candidate.backend else {
            continue;
        };
        if let Err(error) = global.reserve_mount_point(candidate.mount.root.clone()) {
            mounts.insert_failed(candidate.mount, error.to_string());
            continue;
        }

        backends.insert(candidate.mount.root.clone(), backend.clone());
        mounts.insert_loading(candidate.mount, backend);
    }

    reserve_failed_mount_points(global, mounts);
}

fn external_mount_candidates(
    declarations: Vec<LoadedMountDeclaration>,
    bootstrap_roots: &[VirtualPath],
) -> Vec<ExternalMountCandidate> {
    let mut out = Vec::new();
    for (order, declaration) in declarations.into_iter().enumerate() {
        match declaration {
            LoadedMountDeclaration::Parsed(declaration) => {
                let mount_root = match VirtualPath::from_absolute(declaration.mount_at.clone()) {
                    Ok(root) => root,
                    Err(error) => {
                        leptos::logging::warn!(
                            "runtime: ignoring mount declaration with invalid mount_at `{}`: {error}",
                            declaration.mount_at
                        );
                        continue;
                    }
                };
                if bootstrap_roots.iter().any(|root| root == &mount_root) {
                    continue;
                }

                match github_backend::build_backend_for_declaration(&declaration) {
                    Ok(Some((mount, backend))) => out.push(ExternalMountCandidate {
                        order,
                        mount,
                        backend: Some(backend),
                        build_error: None,
                    }),
                    Ok(None) => {}
                    Err(error) => out.push(ExternalMountCandidate {
                        order,
                        mount: fallback_mount_for_declaration(&declaration, mount_root),
                        backend: None,
                        build_error: Some(error.to_string()),
                    }),
                }
            }
            LoadedMountDeclaration::Failed(failed) => {
                if bootstrap_roots
                    .iter()
                    .any(|root| root == &failed.mount.root)
                {
                    continue;
                }
                out.push(ExternalMountCandidate {
                    order,
                    mount: failed.mount,
                    backend: None,
                    build_error: Some(failed.error),
                });
            }
        }
    }
    out
}

fn rejected_mount_candidates(
    candidates: &[ExternalMountCandidate],
) -> BTreeMap<usize, RejectedMount> {
    let mut rejected = BTreeMap::new();
    let mut first_by_root: BTreeMap<VirtualPath, usize> = BTreeMap::new();
    for candidate in candidates {
        if first_by_root.contains_key(&candidate.mount.root) {
            rejected.insert(
                candidate.order,
                RejectedMount {
                    error: format!(
                        "duplicate mount root {}; first declaration kept",
                        candidate.mount.root.as_str()
                    ),
                    kind: RejectedMountKind::Duplicate,
                },
            );
        } else {
            first_by_root.insert(candidate.mount.root.clone(), candidate.order);
        }
    }

    let roots = first_by_root.keys().cloned().collect::<Vec<_>>();
    for candidate in candidates {
        if rejected.contains_key(&candidate.order) {
            continue;
        }
        if let Some(ancestor) = roots
            .iter()
            .filter(|root| *root != &candidate.mount.root && candidate.mount.root.starts_with(root))
            .min_by_key(|root| root.as_str().len())
        {
            rejected.insert(
                candidate.order,
                RejectedMount {
                    error: format!(
                        "mount root {} overlaps ancestor {}; shallowest mount kept",
                        candidate.mount.root.as_str(),
                        ancestor.as_str()
                    ),
                    kind: RejectedMountKind::Overlap,
                },
            );
        }
    }

    rejected
}

fn fallback_mount_for_declaration(
    declaration: &MountDeclaration,
    mount_root: VirtualPath,
) -> RuntimeMount {
    let label = declaration
        .name
        .clone()
        .unwrap_or_else(|| mount_label_for_root(&mount_root));
    RuntimeMount::new(
        mount_root,
        label,
        RuntimeBackendKind::GitHub,
        declaration.writable,
    )
}

fn reserve_failed_mount_points(global: &mut GlobalFs, mounts: &MountLoadSet) {
    let mut roots = mounts
        .entries
        .iter()
        .filter(|(_, entry)| matches!(entry.status, MountLoadStatus::Failed { .. }))
        .map(|(root, _)| root.clone())
        .collect::<Vec<_>>();
    roots.sort_by_key(|root| root.as_str().len());
    for root in roots {
        let _ = global.reserve_mount_point(root);
    }
}

fn mount_label_for_root(root: &VirtualPath) -> String {
    if root.is_root() {
        "~".to_string()
    } else {
        root.file_name()
            .map(str::to_string)
            .unwrap_or_else(|| root.as_str().to_string())
    }
}

async fn load_site_json_if_present(
    global: &GlobalFs,
    backends: &BackendRegistry,
) -> Result<(), RuntimeLoadError> {
    let path = VirtualPath::from_absolute("/.websh/site.json").expect("constant path");
    if !global.exists(&path) {
        return Ok(());
    }

    let site_root = BOOTSTRAP_SITE.mount_root();
    let Some(site_backend) = backends.get(&site_root) else {
        return Ok(());
    };
    let body = read_backend_text(site_backend, &site_root, &path).await?;
    let _: Value = serde_json::from_str(&body).map_err(|source| RuntimeLoadError::ParseJson {
        path: path.clone(),
        source,
    })?;
    Ok(())
}

async fn load_mount_declarations(
    global: &GlobalFs,
    backends: &BackendRegistry,
) -> Result<Vec<LoadedMountDeclaration>, RuntimeLoadError> {
    let site_root = BOOTSTRAP_SITE.mount_root();
    let mounts_root = VirtualPath::from_absolute("/.websh/mounts").expect("constant path");
    let Some(site_backend) = backends.get(&site_root) else {
        return Ok(Vec::new());
    };
    if !global.is_directory(&mounts_root) {
        return Ok(Vec::new());
    }

    let mut declarations = Vec::new();
    for entry in global.list_dir(&mounts_root).unwrap_or_default() {
        if entry.is_dir || !entry.name.ends_with(".mount.json") {
            continue;
        }

        let body = read_backend_text(site_backend, &site_root, &entry.path).await?;
        match serde_json::from_str::<MountDeclaration>(&body) {
            Ok(declaration) => declarations.push(LoadedMountDeclaration::Parsed(declaration)),
            Err(source) => {
                if let Some(failed) = recover_failed_mount_declaration(&entry.path, &body, &source)
                {
                    declarations.push(LoadedMountDeclaration::Failed(failed));
                } else {
                    leptos::logging::warn!(
                        "runtime: ignoring mount declaration {}: {source}",
                        entry.path.as_str()
                    );
                }
            }
        }
    }

    Ok(declarations)
}

fn recover_failed_mount_declaration(
    path: &VirtualPath,
    body: &str,
    source: &serde_json::Error,
) -> Option<FailedMountDeclaration> {
    let value: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(error) => {
            leptos::logging::warn!(
                "runtime: ignoring malformed mount declaration {}: {error}",
                path.as_str()
            );
            return None;
        }
    };

    let backend = value.get("backend").and_then(Value::as_str);
    if backend != Some("github") {
        return None;
    }

    let mount_at = value.get("mount_at").and_then(Value::as_str)?;
    let mount_root = match VirtualPath::from_absolute(mount_at.to_string()) {
        Ok(root) => root,
        Err(error) => {
            leptos::logging::warn!(
                "runtime: ignoring mount declaration {} with invalid mount_at `{mount_at}`: {error}",
                path.as_str()
            );
            return None;
        }
    };
    let label = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| mount_label_for_root(&mount_root));
    let writable = value
        .get("writable")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Some(FailedMountDeclaration {
        mount: RuntimeMount::new(mount_root, label, RuntimeBackendKind::GitHub, writable),
        error: format!("parse {}: {source}", path.as_str()),
    })
}

// Sidecar metadata is no longer fetched at runtime. The CLI
// `content manifest` step pre-bakes every node's full `NodeMetadata`
// into the bundled `manifest.json`, and the manifest scan deserializes
// it directly into each `FsEntry`. This eliminates the previous
// per-file `.meta.json` fetches (and the rate-limit failures they were
// prone to).

async fn load_route_index(
    global: &mut GlobalFs,
    backends: &BackendRegistry,
) -> Result<(), RuntimeLoadError> {
    let site_root = BOOTSTRAP_SITE.mount_root();
    let index_path = VirtualPath::from_absolute("/.websh/index.json").expect("constant path");
    let Some(site_backend) = backends.get(&site_root) else {
        global.replace_route_index(Vec::new());
        return Ok(());
    };
    if !global.exists(&index_path) {
        global.replace_route_index(Vec::new());
        return Ok(());
    }

    let body = read_backend_text(site_backend, &site_root, &index_path).await?;
    let index: DerivedIndex =
        serde_json::from_str(&body).map_err(|source| RuntimeLoadError::ParseJson {
            path: index_path.clone(),
            source,
        })?;
    global.replace_route_index(index.routes);
    Ok(())
}

async fn read_backend_text(
    backend: &StorageBackendRef,
    mount_root: &VirtualPath,
    path: &VirtualPath,
) -> Result<String, RuntimeLoadError> {
    let rel_path =
        path.strip_prefix(mount_root)
            .ok_or_else(|| RuntimeLoadError::PathOutsideMount {
                path: path.clone(),
                mount_root: mount_root.clone(),
            })?;
    backend
        .read_text(rel_path)
        .await
        .map_err(|source| RuntimeLoadError::Read {
            path: path.clone(),
            source,
        })
}

fn collect_file_paths(global: &GlobalFs, root: &VirtualPath) -> Vec<VirtualPath> {
    let mut out = Vec::new();
    collect_file_paths_recursive(global, root, &mut out);
    out
}

fn collect_file_paths_recursive(global: &GlobalFs, path: &VirtualPath, out: &mut Vec<VirtualPath>) {
    let Some(entry) = global.get_entry(path) else {
        return;
    };
    if !entry.is_directory() {
        out.push(path.clone());
        return;
    }

    for child in global.list_dir(path).unwrap_or_default() {
        collect_file_paths_recursive(global, &child.path, out);
    }
}

fn count_files(global: &GlobalFs, root: &VirtualPath) -> usize {
    collect_file_paths(global, root).len()
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    fn declaration(mount_at: &str, name: &str) -> MountDeclaration {
        MountDeclaration {
            backend: "github".to_string(),
            mount_at: mount_at.to_string(),
            repo: Some("0xwonj/websh-test".to_string()),
            branch: Some("main".to_string()),
            root: Some("content".to_string()),
            name: Some(name.to_string()),
            writable: true,
            ..Default::default()
        }
    }

    #[wasm_bindgen_test]
    fn duplicate_and_nested_mounts_fail_without_blocking_root_load() {
        let mut global = GlobalFs::empty();
        let mut backends = BTreeMap::new();
        let mut mounts = MountLoadSet::empty();
        let declarations = vec![
            LoadedMountDeclaration::Parsed(declaration("/db", "db")),
            LoadedMountDeclaration::Parsed(declaration("/db", "db-duplicate")),
            LoadedMountDeclaration::Parsed(declaration("/db/sub", "db-sub")),
        ];

        register_external_mounts(
            &mut global,
            &mut backends,
            &mut mounts,
            declarations,
            &[VirtualPath::root()],
        );

        let db = VirtualPath::from_absolute("/db").expect("db");
        let nested = VirtualPath::from_absolute("/db/sub").expect("nested");
        assert!(matches!(
            mounts.status(&db),
            Some(MountLoadStatus::Loading { .. })
        ));
        assert!(matches!(
            mounts.status(&nested),
            Some(MountLoadStatus::Failed { .. })
        ));
        assert_eq!(mounts.scan_jobs.len(), 1);
        assert!(global.is_directory(&db));
        assert!(global.is_directory(&nested));

        let failures = mounts.failed_entries();
        assert_eq!(failures.len(), 2);
        assert!(failures.iter().any(|entry| entry.declared.root == db));
        assert!(failures.iter().any(|entry| entry.declared.root == nested));
    }

    #[wasm_bindgen_test]
    fn missing_required_writable_becomes_failed_mount_declaration() {
        let path = VirtualPath::from_absolute("/.websh/mounts/db.mount.json").expect("path");
        let body = r#"{
            "backend": "github",
            "mount_at": "/db",
            "repo": "0xwonj/db",
            "branch": "main",
            "root": "content"
        }"#;
        let source = serde_json::from_str::<MountDeclaration>(body).unwrap_err();
        let failed = recover_failed_mount_declaration(&path, body, &source)
            .expect("github declaration with mount_at can be represented as failed");

        assert_eq!(failed.mount.root.as_str(), "/db");
        assert_eq!(failed.mount.label, "db");
        assert!(!failed.mount.writable);
        assert!(failed.error.contains("missing field `writable`"));

        let mut global = GlobalFs::empty();
        let mut backends = BTreeMap::new();
        let mut mounts = MountLoadSet::empty();
        register_external_mounts(
            &mut global,
            &mut backends,
            &mut mounts,
            vec![LoadedMountDeclaration::Failed(failed)],
            &[VirtualPath::root()],
        );

        let db = VirtualPath::from_absolute("/db").expect("db");
        assert!(matches!(
            mounts.status(&db),
            Some(MountLoadStatus::Failed { ref error, .. })
                if error.contains("missing field `writable`")
        ));
        assert!(global.is_directory(&db));
        assert!(backends.is_empty());
    }
}
