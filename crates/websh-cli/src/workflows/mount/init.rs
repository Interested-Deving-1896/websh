use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, bail};
use serde::Serialize;

use crate::CliResult;
use crate::infra::gh::{GhResourceStatus, gh_resource_status, require_gh};
use crate::workflows::content::{DEFAULT_CONTENT_DIR, sync_content};

use super::remote::push_empty_manifest;

#[derive(Clone, Debug)]
pub(crate) struct MountInitOptions {
    pub(crate) name: String,
    pub(crate) repo: String,
    pub(crate) mount_at: String,
    pub(crate) branch: String,
    pub(crate) root: String,
    pub(crate) writable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ManifestBootstrapStatus {
    AlreadyPresent,
    Created,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MountInitOutcome {
    pub(crate) mount_name: String,
    pub(crate) mount_at: String,
    pub(crate) repo: String,
    pub(crate) branch: String,
    pub(crate) manifest_path_in_repo: String,
    pub(crate) manifest_status: ManifestBootstrapStatus,
    pub(crate) mount_decl_path: PathBuf,
    pub(crate) bundle_entries: usize,
}

#[derive(Serialize)]
struct MountFile {
    backend: &'static str,
    mount_at: String,
    repo: String,
    branch: String,
    root: String,
    name: String,
    writable: bool,
}

pub(crate) fn init_mount(root: &Path, init: MountInitOptions) -> CliResult<MountInitOutcome> {
    require_gh()?;
    let mount_name = MountName::from_str(&init.name)
        .with_context(|| format!("invalid --name `{}`", init.name))?;
    websh_core::domain::VirtualPath::from_absolute(init.mount_at.clone())
        .with_context(|| format!("invalid --mount-at `{}`", init.mount_at))?;
    let root_prefix = RepoRootPrefix::from_str(&init.root)
        .with_context(|| format!("invalid --root `{}`", init.root))?;

    match gh_resource_status(["api", &format!("repos/{}", init.repo), "--silent"])? {
        GhResourceStatus::Exists => {}
        GhResourceStatus::Missing => {
            bail!(
                "github repo {} not found - create it first with `gh repo create {}` \
                 (or via the web UI), then re-run this command",
                init.repo,
                init.repo,
            );
        }
    }

    let manifest_path_in_repo = manifest_repo_path(&root_prefix);
    let manifest_status = gh_resource_status([
        "api",
        &format!(
            "repos/{}/contents/{}?ref={}",
            init.repo, manifest_path_in_repo, init.branch
        ),
        "--silent",
    ])?;
    let manifest_status = match manifest_status {
        GhResourceStatus::Exists => ManifestBootstrapStatus::AlreadyPresent,
        GhResourceStatus::Missing => {
            push_empty_manifest(&init.repo, &init.branch, &manifest_path_in_repo)?;
            ManifestBootstrapStatus::Created
        }
    };

    let mount_file = MountFile {
        backend: "github",
        mount_at: init.mount_at.clone(),
        repo: init.repo.clone(),
        branch: init.branch.clone(),
        root: root_prefix.to_string(),
        name: mount_name.to_string(),
        writable: init.writable,
    };
    let mount_decl_path = mount_declaration_path(root, &mount_name);
    if let Some(parent) = mount_decl_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    let body =
        serde_json::to_string_pretty(&mount_file).context("serialize mount declaration json")?;
    std::fs::write(&mount_decl_path, format!("{body}\n"))
        .with_context(|| format!("write {}", mount_decl_path.display()))?;

    let bundle = sync_content(root, Path::new(DEFAULT_CONTENT_DIR))?;

    Ok(MountInitOutcome {
        mount_name: mount_name.to_string(),
        mount_at: init.mount_at,
        repo: init.repo,
        branch: init.branch,
        manifest_path_in_repo,
        manifest_status,
        mount_decl_path,
        bundle_entries: bundle.entries.len(),
    })
}

fn manifest_repo_path(root: &RepoRootPrefix) -> String {
    if root.is_repo_root() {
        "manifest.json".to_string()
    } else {
        format!("{root}/manifest.json")
    }
}

fn mount_declaration_path(root: &Path, name: &MountName) -> PathBuf {
    root.join(DEFAULT_CONTENT_DIR)
        .join(".websh")
        .join("mounts")
        .join(format!("{name}.mount.json"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MountName(String);

impl FromStr for MountName {
    type Err = MountNameError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.is_empty() {
            return Err(MountNameError::Empty);
        }
        if raw == "." || raw == ".." {
            return Err(MountNameError::Traversal);
        }
        if raw.contains('/') || raw.contains('\\') {
            return Err(MountNameError::Separator);
        }
        if raw.len() > 64 {
            return Err(MountNameError::TooLong);
        }
        let mut chars = raw.chars();
        let Some(first) = chars.next() else {
            return Err(MountNameError::Empty);
        };
        if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
            return Err(MountNameError::InvalidCharacter);
        }
        if !raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(MountNameError::InvalidCharacter);
        }
        Ok(Self(raw.to_string()))
    }
}

impl std::fmt::Display for MountName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
enum MountNameError {
    #[error("mount name is empty")]
    Empty,
    #[error("mount name cannot be . or ..")]
    Traversal,
    #[error("mount name cannot contain path separators")]
    Separator,
    #[error("mount name is too long")]
    TooLong,
    #[error("mount name must match [a-z0-9][a-z0-9_-]*")]
    InvalidCharacter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RepoRootPrefix(String);

impl RepoRootPrefix {
    fn is_repo_root(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromStr for RepoRootPrefix {
    type Err = RepoRootPrefixError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.contains('\\') {
            return Err(RepoRootPrefixError::Backslash);
        }
        if raw.chars().any(char::is_control) {
            return Err(RepoRootPrefixError::ControlCharacter);
        }

        let trimmed = raw.trim_matches('/');
        if trimmed.is_empty() {
            return Ok(Self(String::new()));
        }
        if trimmed.contains("//") {
            return Err(RepoRootPrefixError::EmptySegment);
        }

        for segment in trimmed.split('/') {
            if segment.is_empty() {
                return Err(RepoRootPrefixError::EmptySegment);
            }
            if matches!(segment, "." | "..") {
                return Err(RepoRootPrefixError::Traversal);
            }
        }

        Ok(Self(trimmed.to_string()))
    }
}

impl std::fmt::Display for RepoRootPrefix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
enum RepoRootPrefixError {
    #[error("repo root prefix contains an empty segment")]
    EmptySegment,
    #[error("repo root prefix cannot contain . or ..")]
    Traversal,
    #[error("repo root prefix cannot contain backslashes")]
    Backslash,
    #[error("repo root prefix cannot contain control characters")]
    ControlCharacter,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_repo_path_at_repo_root() {
        assert_eq!(
            manifest_repo_path(&RepoRootPrefix::from_str("").unwrap()),
            "manifest.json"
        );
        assert_eq!(
            manifest_repo_path(&RepoRootPrefix::from_str("/").unwrap()),
            "manifest.json"
        );
    }

    #[test]
    fn manifest_repo_path_with_subdir() {
        assert_eq!(
            manifest_repo_path(&RepoRootPrefix::from_str("content").unwrap()),
            "content/manifest.json"
        );
        assert_eq!(
            manifest_repo_path(&RepoRootPrefix::from_str("/content/").unwrap()),
            "content/manifest.json"
        );
    }

    #[test]
    fn repo_root_prefix_rejects_invalid_paths() {
        for raw in ["../content", "content/..", "a//b", "a\\b", "a/\u{7}"] {
            assert!(RepoRootPrefix::from_str(raw).is_err(), "{raw} should fail");
        }
    }

    #[test]
    fn repo_root_prefix_canonicalizes_slashes() {
        assert_eq!(
            RepoRootPrefix::from_str("/content/posts/")
                .unwrap()
                .to_string(),
            "content/posts"
        );
    }

    #[test]
    fn mount_declaration_path_uses_websh_mounts_dir() {
        let p = mount_declaration_path(
            Path::new("/tmp/proj"),
            &MountName::from_str("mempool").unwrap(),
        );
        assert!(p.ends_with("content/.websh/mounts/mempool.mount.json"));
    }

    #[test]
    fn mount_name_rejects_path_traversal() {
        for raw in ["", ".", "..", "../evil", "/tmp/evil", "bad/name", "Bad"] {
            assert!(MountName::from_str(raw).is_err(), "{raw} should fail");
        }
    }
}
