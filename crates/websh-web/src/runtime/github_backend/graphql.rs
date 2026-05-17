//! Pure GraphQL commit-payload builders. No HTTP, no signals. Items are
//! consumed by the browser GitHub backend; on the host triple
//! the lint fires because the client isn't compiled. The host-only allow
//! keeps wasm32 honest while quieting the noisy host build.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::BTreeSet;

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde::Serialize;

use websh_core::domain::VirtualPath;
use websh_core::ports::CommitDelta;

use super::path::{
    RepoPathError, normalize_repo_prefix, prefixed_repo_path, validate_repo_relative_path,
};

#[derive(Debug, Serialize)]
pub struct CreateCommitInput {
    pub branch: BranchRef,
    pub message: CommitMessage,
    #[serde(rename = "expectedHeadOid", skip_serializing_if = "Option::is_none")]
    pub expected_head_oid: Option<String>,
    #[serde(rename = "fileChanges")]
    pub file_changes: FileChanges,
}

#[derive(Debug, Serialize)]
pub struct BranchRef {
    #[serde(rename = "repositoryNameWithOwner")]
    pub repo_with_owner: String,
    #[serde(rename = "branchName")]
    pub branch_name: String,
}

#[derive(Debug, Serialize)]
pub struct CommitMessage {
    pub headline: String,
}

#[derive(Debug, Default, Serialize)]
pub struct FileChanges {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub additions: Vec<FileAddition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deletions: Vec<FileDeletion>,
}

#[derive(Debug, Serialize)]
pub struct FileAddition {
    pub path: String,
    pub contents: String, // base64
}

#[derive(Debug, Serialize)]
pub struct FileDeletion {
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GraphQLOperationBuildError {
    #[error(transparent)]
    RepoPath(#[from] RepoPathError),
    #[error("staged path {path} is outside mount root {mount_root}")]
    StagedPathOutsideMount {
        path: VirtualPath,
        mount_root: VirtualPath,
    },
    #[error("duplicate addition path: {path}")]
    DuplicateAddition { path: String },
    #[error("fileChanges has both addition and deletion for {path}")]
    AddDeleteCollision { path: String },
}

/// Build the fileChanges payload from a prepared backend-neutral commit delta.
///
/// `mount_root` is stripped from canonical filesystem paths before emission,
/// then `repo_prefix` is prepended to produce repo-relative GitHub paths.
pub fn build_file_changes(
    delta: &CommitDelta,
    mount_root: &VirtualPath,
    repo_prefix: &str,
    serialized_manifest: Option<(&str, &str)>, // (repo_path, body_bytes_utf8)
) -> Result<FileChanges, GraphQLOperationBuildError> {
    let mut fc = FileChanges::default();
    let repo_prefix = normalize_repo_prefix(repo_prefix)?;

    for addition in &delta.additions {
        let repo_path = join_repo_path(mount_root, &repo_prefix, &addition.path)?;
        fc.additions.push(FileAddition {
            path: repo_path,
            contents: B64.encode(addition.content.as_bytes()),
        });
    }

    for path in &delta.deletions {
        fc.deletions.push(FileDeletion {
            path: join_repo_path(mount_root, &repo_prefix, path)?,
        });
    }

    if let Some((path, body)) = serialized_manifest {
        validate_repo_relative_path(path, false)?;
        fc.additions.push(FileAddition {
            path: path.to_string(),
            contents: B64.encode(body.as_bytes()),
        });
    }

    // Sort both lists by path for deterministic GraphQL bodies.
    fc.additions.sort_by(|a, b| a.path.cmp(&b.path));
    fc.deletions.sort_by(|a, b| a.path.cmp(&b.path));
    fc.deletions.dedup_by(|left, right| left.path == right.path);
    reject_duplicate_additions(&fc)?;
    reject_add_delete_collisions(&fc)?;

    Ok(fc)
}

fn reject_duplicate_additions(fc: &FileChanges) -> Result<(), GraphQLOperationBuildError> {
    let mut seen = BTreeSet::new();
    for addition in &fc.additions {
        if !seen.insert(addition.path.as_str()) {
            return Err(GraphQLOperationBuildError::DuplicateAddition {
                path: addition.path.clone(),
            });
        }
    }
    Ok(())
}

fn reject_add_delete_collisions(fc: &FileChanges) -> Result<(), GraphQLOperationBuildError> {
    let additions = fc
        .additions
        .iter()
        .map(|addition| addition.path.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(deletion) = fc
        .deletions
        .iter()
        .find(|deletion| additions.contains(deletion.path.as_str()))
    {
        return Err(GraphQLOperationBuildError::AddDeleteCollision {
            path: deletion.path.clone(),
        });
    }
    Ok(())
}

fn join_repo_path(
    mount_root: &VirtualPath,
    prefix: &str,
    path: &VirtualPath,
) -> Result<String, GraphQLOperationBuildError> {
    let tail = path.strip_prefix(mount_root).ok_or_else(|| {
        GraphQLOperationBuildError::StagedPathOutsideMount {
            path: path.clone(),
            mount_root: mount_root.clone(),
        }
    })?;
    Ok(prefixed_repo_path(prefix, tail)?)
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;
    use websh_core::domain::VirtualPath;
    use websh_core::ports::{CommitDelta, CommitFileAddition};

    wasm_bindgen_test_configure!(run_in_browser);

    fn p(s: &str) -> VirtualPath {
        VirtualPath::from_absolute(s).unwrap()
    }

    fn add(path: &str, content: &str) -> CommitFileAddition {
        CommitFileAddition {
            path: p(path),
            content: content.to_string(),
        }
    }

    #[wasm_bindgen_test]
    fn additions_are_sorted_and_base64() {
        let delta = CommitDelta {
            additions: vec![add("/z.md", "zz"), add("/a.md", "aa")],
            ..Default::default()
        };
        let fc = build_file_changes(&delta, &VirtualPath::root(), "~", None).unwrap();
        assert_eq!(fc.additions.len(), 2);
        assert_eq!(fc.additions[0].path, "~/a.md");
        assert_eq!(fc.additions[1].path, "~/z.md");
        assert_eq!(fc.additions[0].contents, B64.encode(b"aa"));
    }

    #[wasm_bindgen_test]
    fn deletions_are_emitted() {
        let delta = CommitDelta {
            deletions: vec![p("/gone.md")],
            ..Default::default()
        };
        let fc = build_file_changes(&delta, &VirtualPath::root(), "", None).unwrap();
        assert_eq!(fc.deletions.len(), 1);
        assert_eq!(fc.deletions[0].path, "gone.md");
        assert!(fc.additions.is_empty());
    }

    #[wasm_bindgen_test]
    fn unstaged_is_excluded() {
        let delta = CommitDelta::default();
        let fc = build_file_changes(&delta, &VirtualPath::root(), "", None).unwrap();
        assert!(fc.additions.is_empty());
    }

    #[wasm_bindgen_test]
    fn manifest_is_appended_and_sorted_in() {
        let delta = CommitDelta {
            additions: vec![add("/b.md", "b")],
            ..Default::default()
        };
        let fc = build_file_changes(
            &delta,
            &VirtualPath::root(),
            "",
            Some(("manifest.json", "{}")),
        )
        .unwrap();
        let paths: Vec<_> = fc.additions.iter().map(|a| a.path.as_str()).collect();
        assert_eq!(paths, vec!["b.md", "manifest.json"]);
    }

    #[wasm_bindgen_test]
    fn manifest_path_collision_is_rejected() {
        let delta = CommitDelta {
            additions: vec![add("/manifest.json", "user")],
            ..Default::default()
        };
        let err = build_file_changes(
            &delta,
            &VirtualPath::root(),
            "~",
            Some(("~/manifest.json", "{}")),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            GraphQLOperationBuildError::DuplicateAddition { path }
                if path == "~/manifest.json"
        ));
    }

    #[wasm_bindgen_test]
    fn directory_creates_are_dropped() {
        let delta = CommitDelta::default();
        let fc = build_file_changes(&delta, &VirtualPath::root(), "", None).unwrap();
        assert!(fc.additions.is_empty());
        assert!(fc.deletions.is_empty());
    }

    #[wasm_bindgen_test]
    fn mount_root_is_stripped_before_repo_prefix_is_applied() {
        let delta = CommitDelta {
            additions: vec![add("/work/note.md", "hello")],
            ..Default::default()
        };
        let fc = build_file_changes(&delta, &p("/work"), "content", None).unwrap();
        assert_eq!(fc.additions[0].path, "content/note.md");
    }

    #[wasm_bindgen_test]
    fn staged_path_outside_mount_root_is_rejected() {
        let delta = CommitDelta {
            additions: vec![add("/other/note.md", "hello")],
            ..Default::default()
        };
        let err = build_file_changes(&delta, &p("/work"), "content", None).unwrap_err();
        assert!(matches!(
            err,
            GraphQLOperationBuildError::StagedPathOutsideMount { path, mount_root }
                if path == p("/other/note.md") && mount_root == p("/work")
        ));
    }

    #[wasm_bindgen_test]
    fn directory_delete_descendants_are_emitted() {
        let delta = CommitDelta {
            deletions: vec![p("/docs/a.md"), p("/docs/deep/b.md")],
            ..Default::default()
        };
        let fc = build_file_changes(&delta, &VirtualPath::root(), "~", None).unwrap();
        let paths: Vec<_> = fc
            .deletions
            .iter()
            .map(|delete| delete.path.as_str())
            .collect();
        assert_eq!(paths, vec!["~/docs/a.md", "~/docs/deep/b.md"]);
    }

    #[wasm_bindgen_test]
    fn prefixed_manifest_path_uses_content_prefix() {
        assert_eq!(
            prefixed_repo_path("~", "manifest.json").unwrap(),
            "~/manifest.json"
        );
        assert_eq!(
            prefixed_repo_path("content/site", "manifest.json").unwrap(),
            "content/site/manifest.json"
        );
        assert_eq!(
            prefixed_repo_path("", "manifest.json").unwrap(),
            "manifest.json"
        );
    }

    #[wasm_bindgen_test]
    fn invalid_repo_prefix_is_rejected() {
        let delta = CommitDelta {
            additions: vec![add("/a.md", "a")],
            ..Default::default()
        };
        let err =
            build_file_changes(&delta, &VirtualPath::root(), "content/../x", None).unwrap_err();
        assert!(matches!(
            err,
            GraphQLOperationBuildError::RepoPath(RepoPathError::Traversal { path })
                if path == "content/../x"
        ));
    }
}
