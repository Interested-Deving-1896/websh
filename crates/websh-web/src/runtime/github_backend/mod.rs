//! Browser GitHub storage backend and mount builders.

use std::rc::Rc;

use websh_core::domain::{
    BootstrapSiteSource, MountDeclaration, RuntimeBackendKind, RuntimeMount, VirtualPath,
};
use websh_core::ports::StorageBackendRef;

mod client;
mod graphql;
mod path;

pub use client::{GitHubBackend, GitHubBackendConfigError};

use path::{RepoPathError, normalize_repo_prefix};

type DeclaredBackend = (RuntimeMount, StorageBackendRef);
const RAW_GITHUB_GATEWAY: &str = "https://raw.githubusercontent.com";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GitHubBackendDeclarationError {
    #[error("github mount {mount_at} is missing repo")]
    MissingRepo { mount_at: String },
    #[error("invalid mount_at: {mount_at}")]
    InvalidMountAt { mount_at: String },
    #[error("noncanonical mount_at: {mount_at}")]
    NoncanonicalMountAt { mount_at: String },
    #[error("invalid root for {mount_at}: {source}")]
    InvalidRoot {
        mount_at: String,
        source: RepoPathError,
    },
    #[error(
        "unsupported gateway for {mount_at}: `{gateway}` is not allowed by the browser runtime; allowed gateways are `self` and `https://raw.githubusercontent.com`"
    )]
    UnsupportedGateway { mount_at: String, gateway: String },
    #[error("invalid github backend {mount_at}: {source}")]
    InvalidBackend {
        mount_at: String,
        source: GitHubBackendConfigError,
    },
}

pub fn build_backend_for_bootstrap_site(source: &BootstrapSiteSource) -> StorageBackendRef {
    let prefix = source.content_root.trim_matches('/').to_string();
    let gateway = normalize_allowed_browser_gateway(source.gateway)
        .expect("bootstrap site source must use a browser-allowed gateway");

    Rc::new(
        GitHubBackend::new_with_manifest_policy(
            source.repo_with_owner,
            source.branch,
            source.mount_root(),
            prefix,
            gateway,
            false,
        )
        .expect("bootstrap site source must have a valid content root"),
    )
}

pub fn build_backend_for_declaration(
    declaration: &MountDeclaration,
) -> Result<Option<DeclaredBackend>, GitHubBackendDeclarationError> {
    match declaration.backend.as_str() {
        "github" => {
            let repo = declaration.repo.clone().ok_or_else(|| {
                GitHubBackendDeclarationError::MissingRepo {
                    mount_at: declaration.mount_at.clone(),
                }
            })?;
            let branch = declaration
                .branch
                .clone()
                .unwrap_or_else(|| "main".to_string());
            let mount_root =
                VirtualPath::from_absolute(declaration.mount_at.clone()).map_err(|_| {
                    GitHubBackendDeclarationError::InvalidMountAt {
                        mount_at: declaration.mount_at.clone(),
                    }
                })?;
            if !is_canonical_mount_root(&mount_root) {
                return Err(GitHubBackendDeclarationError::NoncanonicalMountAt {
                    mount_at: declaration.mount_at.clone(),
                });
            }
            let prefix = normalize_repo_prefix(&declaration.root.clone().unwrap_or_default())
                .map_err(|source| GitHubBackendDeclarationError::InvalidRoot {
                    mount_at: declaration.mount_at.clone(),
                    source,
                })?;
            let gateway = declaration.gateway.as_deref().unwrap_or(RAW_GITHUB_GATEWAY);
            let gateway = normalize_allowed_browser_gateway(gateway).ok_or_else(|| {
                GitHubBackendDeclarationError::UnsupportedGateway {
                    mount_at: declaration.mount_at.clone(),
                    gateway: normalized_gateway_for_error(gateway),
                }
            })?;
            let label = declaration.name.clone().unwrap_or_else(|| {
                mount_root
                    .file_name()
                    .map(str::to_string)
                    .unwrap_or_else(|| mount_root.as_str().to_string())
            });

            let mount = RuntimeMount::new(
                mount_root.clone(),
                label,
                RuntimeBackendKind::GitHub,
                declaration.writable,
            );

            let backend = GitHubBackend::new(repo, branch, mount_root, prefix, gateway).map_err(
                |source| GitHubBackendDeclarationError::InvalidBackend {
                    mount_at: declaration.mount_at.clone(),
                    source,
                },
            )?;

            Ok(Some((mount, Rc::new(backend))))
        }
        _ => Ok(None),
    }
}

fn normalize_allowed_browser_gateway(gateway: &str) -> Option<&'static str> {
    match gateway.trim_end_matches('/') {
        "self" => Some("self"),
        RAW_GITHUB_GATEWAY => Some(RAW_GITHUB_GATEWAY),
        _ => None,
    }
}

fn normalized_gateway_for_error(gateway: &str) -> String {
    gateway.trim_end_matches('/').to_string()
}

fn is_canonical_mount_root(path: &VirtualPath) -> bool {
    if path.is_root() || path.as_str().contains('\\') {
        return false;
    }
    let segments = path.segments().collect::<Vec<_>>();
    if segments
        .iter()
        .any(|segment| *segment == "." || *segment == ".." || segment.chars().any(char::is_control))
    {
        return false;
    }
    format!("/{}", segments.join("/")) == path.as_str()
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn declaration_builds_github_backend() {
        let declaration = MountDeclaration {
            backend: "github".to_string(),
            mount_at: "/db".to_string(),
            repo: Some("0xwonj/db".to_string()),
            branch: Some("main".to_string()),
            root: Some("content".to_string()),
            ..Default::default()
        };

        let (mount, backend) = build_backend_for_declaration(&declaration)
            .expect("valid declaration")
            .expect("backend");
        assert_eq!(mount.root.as_str(), "/db");
        assert_eq!(mount.label, "db");
        assert_eq!(backend.backend_type(), "github");
    }

    #[wasm_bindgen_test]
    fn declaration_rejects_noncanonical_mount_root() {
        let declaration = MountDeclaration {
            backend: "github".to_string(),
            mount_at: "/db/../bad".to_string(),
            repo: Some("0xwonj/db".to_string()),
            branch: Some("main".to_string()),
            root: Some("content".to_string()),
            ..Default::default()
        };

        assert!(build_backend_for_declaration(&declaration).is_err());
    }

    #[wasm_bindgen_test]
    fn declaration_allows_self_gateway() {
        let declaration = MountDeclaration {
            backend: "github".to_string(),
            mount_at: "/db".to_string(),
            repo: Some("0xwonj/db".to_string()),
            branch: Some("main".to_string()),
            root: Some("content".to_string()),
            gateway: Some("self".to_string()),
            ..Default::default()
        };

        assert!(
            build_backend_for_declaration(&declaration)
                .unwrap()
                .is_some()
        );
    }

    #[wasm_bindgen_test]
    fn declaration_rejects_gateways_outside_browser_policy() {
        let declaration = MountDeclaration {
            backend: "github".to_string(),
            mount_at: "/db".to_string(),
            repo: Some("0xwonj/db".to_string()),
            branch: Some("main".to_string()),
            root: Some("content".to_string()),
            gateway: Some("https://example.com/raw".to_string()),
            ..Default::default()
        };

        let error = match build_backend_for_declaration(&declaration) {
            Err(error) => error,
            Ok(_) => panic!("unsupported gateway must be rejected"),
        };
        assert!(matches!(
            error,
            GitHubBackendDeclarationError::UnsupportedGateway {
                mount_at,
                gateway,
            } if mount_at == "/db" && gateway == "https://example.com/raw"
        ));
    }
}
