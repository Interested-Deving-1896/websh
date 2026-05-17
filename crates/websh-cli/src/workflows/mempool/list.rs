use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

use websh_core::domain::MempoolStatus;
use websh_core::domain::{ContentManifestDocument, ContentManifestEntry};

use crate::CliResult;
use crate::infra::gh::{GhApiOutput, gh_capture, gh_capture_status, require_gh};

use super::mount::{MempoolMountInfo, read_mempool_mount_declaration};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MempoolListEntry {
    pub(crate) status: String,
    pub(crate) path: String,
    pub(crate) size_hint: String,
    pub(crate) modified: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MempoolListOutcome {
    pub(crate) repo: String,
    pub(crate) branch: String,
    pub(crate) entries: Vec<MempoolListEntry>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn list_entries(root: &Path) -> CliResult<MempoolListOutcome> {
    let mount = read_mempool_mount_declaration(root)?;
    require_gh()?;

    let manifest_body = match fetch_manifest_body(&mount)? {
        Some(body) => body,
        None => {
            return Ok(MempoolListOutcome {
                repo: mount.repo,
                branch: mount.branch,
                entries: Vec::new(),
                warnings: Vec::new(),
            });
        }
    };

    let manifest: ContentManifestDocument =
        serde_json::from_str(&manifest_body).context("parse mempool manifest")?;
    let file_entries: Vec<&ContentManifestEntry> = manifest
        .entries
        .iter()
        .filter(|e| !e.metadata.kind.is_directory_like())
        .collect();
    let entries = file_entries
        .iter()
        .map(|entry| format_entry(entry))
        .collect();
    let warnings = match drift_warnings(&mount, &file_entries) {
        Ok(warnings) => warnings,
        Err(error) => vec![format!("drift check failed: {error}")],
    };

    Ok(MempoolListOutcome {
        repo: mount.repo,
        branch: mount.branch,
        entries,
        warnings,
    })
}

fn fetch_manifest_body(mount: &MempoolMountInfo) -> CliResult<Option<String>> {
    let manifest_repo_path = file_in_repo(&mount.root_prefix, "manifest.json");
    let manifest_url = format!(
        "repos/{}/contents/{}?ref={}",
        mount.repo, manifest_repo_path, mount.branch,
    );

    match gh_capture_status([
        "api",
        "-H",
        "Accept: application/vnd.github.raw",
        manifest_url.as_str(),
    ])? {
        GhApiOutput::Success(body) => Ok(Some(body)),
        GhApiOutput::Missing => Ok(None),
    }
}

fn format_entry(entry: &ContentManifestEntry) -> MempoolListEntry {
    let status = entry
        .mempool
        .as_ref()
        .map(|m| match m.status {
            MempoolStatus::Draft => "draft",
            MempoolStatus::Review => "review",
        })
        .unwrap_or("?")
        .to_string();
    let modified = entry
        .metadata
        .authored
        .date
        .as_deref()
        .unwrap_or("-")
        .to_string();
    let size_hint = if entry.path.ends_with(".md") {
        entry
            .metadata
            .derived
            .word_count
            .map(|w| format!("~{w} words"))
            .unwrap_or_default()
    } else {
        entry
            .metadata
            .derived
            .size_bytes
            .map(|n| format!("{n}B"))
            .unwrap_or_default()
    };

    MempoolListEntry {
        status,
        path: entry.path.clone(),
        size_hint,
        modified,
    }
}

fn drift_warnings(
    mount: &MempoolMountInfo,
    manifest_files: &[&ContentManifestEntry],
) -> CliResult<Vec<String>> {
    #[derive(Deserialize)]
    struct TreeResp {
        tree: Vec<TreeEntry>,
    }
    #[derive(Deserialize)]
    struct TreeEntry {
        path: String,
        #[serde(rename = "type")]
        kind: String,
    }

    let url = format!(
        "repos/{}/git/trees/{}?recursive=1",
        mount.repo, mount.branch
    );
    let body = gh_capture(["api", url.as_str()])?;
    let resp: TreeResp = serde_json::from_str(&body).context("parse git/trees response")?;

    let prefix = mount.root_prefix.trim_matches('/');
    let strip = |full: &str| -> Option<String> {
        if prefix.is_empty() {
            Some(full.to_string())
        } else {
            full.strip_prefix(prefix)
                .and_then(|s| s.strip_prefix('/'))
                .map(|s| s.to_string())
        }
    };

    let repo_md: BTreeSet<String> = resp
        .tree
        .into_iter()
        .filter(|e| e.kind == "blob" && e.path.ends_with(".md"))
        .filter_map(|e| strip(&e.path))
        .collect();
    let manifest_paths: BTreeSet<String> = manifest_files
        .iter()
        .map(|e| e.path.clone())
        .filter(|p| p.ends_with(".md"))
        .collect();

    let mut warnings = Vec::new();
    warnings.extend(
        repo_md
            .difference(&manifest_paths)
            .map(|orphan| format!("file in repo not in manifest: {orphan}")),
    );
    warnings.extend(
        manifest_paths
            .difference(&repo_md)
            .map(|missing| format!("manifest entry not in repo: {missing}")),
    );
    Ok(warnings)
}

fn file_in_repo(root_prefix: &str, file_path: &str) -> String {
    let prefix = root_prefix.trim_matches('/');
    if prefix.is_empty() {
        file_path.to_string()
    } else {
        format!("{prefix}/{file_path}")
    }
}

#[cfg(test)]
mod tests {
    use websh_core::domain::{
        Fields, MempoolFields, MempoolStatus, NodeKind, NodeMetadata, SCHEMA_VERSION,
    };

    use super::*;

    fn manifest_entry(path: &str, kind: NodeKind) -> ContentManifestEntry {
        ContentManifestEntry {
            path: path.to_string(),
            metadata: NodeMetadata {
                schema: SCHEMA_VERSION,
                kind,
                bundle: None,
                authored: Fields {
                    date: Some("2026-05-05".to_string()),
                    ..Fields::default()
                },
                derived: Fields {
                    word_count: Some(42),
                    ..Fields::default()
                },
            },
            mempool: Some(MempoolFields {
                status: MempoolStatus::Review,
                priority: None,
                category: Some("writing".to_string()),
            }),
        }
    }

    #[test]
    fn format_entry_uses_manifest_metadata() {
        let entry = manifest_entry("writing/a.md", NodeKind::Page);
        let formatted = format_entry(&entry);
        assert_eq!(formatted.status, "review");
        assert_eq!(formatted.path, "writing/a.md");
        assert_eq!(formatted.size_hint, "~42 words");
        assert_eq!(formatted.modified, "2026-05-05");
    }

    #[test]
    fn file_in_repo_handles_empty_and_nested_prefixes() {
        assert_eq!(file_in_repo("", "manifest.json"), "manifest.json");
        assert_eq!(
            file_in_repo("content/mempool", "manifest.json"),
            "content/mempool/manifest.json"
        );
    }
}
