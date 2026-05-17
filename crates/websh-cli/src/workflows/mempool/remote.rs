use anyhow::{Context, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Deserialize;

use websh_core::domain::VirtualPath;
use websh_core::domain::{ContentManifestDocument, ContentManifestEntry};
use websh_core::mempool::{MempoolManifestState, build_mempool_manifest_state};

use crate::CliResult;
use crate::infra::gh::{
    GhApiOutput, GhResourceStatus, gh_capture, gh_capture_status, gh_resource_status, gh_status,
};

use super::mount::MempoolMountInfo;
use super::path::MempoolEntryPath;

#[derive(Deserialize)]
struct ContentsApiResponse {
    content: String,
    sha: String,
}

struct RemoteManifest {
    document: ContentManifestDocument,
    sha: String,
}

struct GithubContentsClient<'a> {
    mount: &'a MempoolMountInfo,
}

impl<'a> GithubContentsClient<'a> {
    fn new(mount: &'a MempoolMountInfo) -> Self {
        Self { mount }
    }

    fn repo_path(&self, file_path: &str) -> String {
        file_in_repo(&self.mount.root_prefix, file_path)
    }

    fn contents_url(&self, repo_path: &str) -> String {
        format!("repos/{}/contents/{}", self.mount.repo, repo_path)
    }

    fn contents_url_at_ref(&self, repo_path: &str) -> String {
        format!("{}?ref={}", self.contents_url(repo_path), self.mount.branch)
    }

    fn path_status(&self, repo_path: &str) -> CliResult<GhResourceStatus> {
        let url = self.contents_url_at_ref(&self.repo_path(repo_path));
        gh_resource_status(["api", "--silent", url.as_str()])
    }

    fn fetch_raw(&self, repo_path: &str) -> CliResult<String> {
        let url = self.contents_url_at_ref(&self.repo_path(repo_path));
        gh_capture([
            "api",
            "-H",
            "Accept: application/vnd.github.raw",
            url.as_str(),
        ])
    }

    fn get_manifest(&self) -> CliResult<RemoteManifest> {
        let url = self.contents_url_at_ref(&self.repo_path("manifest.json"));
        let response_json = gh_capture(["api", url.as_str()])?;
        let response: ContentsApiResponse =
            serde_json::from_str(&response_json).context("parse manifest GET response")?;
        let manifest_bytes = BASE64_STANDARD
            .decode(response.content.replace('\n', ""))
            .context("base64-decode manifest")?;
        let document: ContentManifestDocument =
            serde_json::from_slice(&manifest_bytes).context("parse mempool manifest")?;
        Ok(RemoteManifest {
            document,
            sha: response.sha,
        })
    }

    fn put_blob(&self, repo_path: &str, file_body: &str, message: &str) -> CliResult<bool> {
        let file_b64 = BASE64_STANDARD.encode(file_body.as_bytes());
        let url = self.contents_url(&self.repo_path(repo_path));
        gh_status([
            "api",
            url.as_str(),
            "-X",
            "PUT",
            "-f",
            &format!("message={message}"),
            "-f",
            &format!("content={file_b64}"),
            "-f",
            &format!("branch={}", self.mount.branch),
        ])
    }

    fn put_manifest(&self, body: &str, sha: &str, message: &str) -> CliResult<bool> {
        let body_b64 = BASE64_STANDARD.encode(body.as_bytes());
        let url = self.contents_url(&self.repo_path("manifest.json"));
        gh_status([
            "api",
            url.as_str(),
            "-X",
            "PUT",
            "-f",
            &format!("message={message}"),
            "-f",
            &format!("content={body_b64}"),
            "-f",
            &format!("sha={sha}"),
            "-f",
            &format!("branch={}", self.mount.branch),
        ])
    }

    fn delete_blob(&self, repo_path: &str) -> CliResult<bool> {
        let Some(file_sha) = self.blob_sha(repo_path)? else {
            return Ok(false);
        };
        let url = self.contents_url(&self.repo_path(repo_path));
        let deleted = gh_status([
            "api",
            url.as_str(),
            "-X",
            "DELETE",
            "-f",
            &format!("message=mempool: drop {repo_path} (blob)"),
            "-f",
            &format!("sha={file_sha}"),
            "-f",
            &format!("branch={}", self.mount.branch),
        ])?;
        if !deleted {
            bail!(
                "blob delete failed for {} (manifest already updated; orphan blob remains - \
                 re-run `websh-cli mempool drop --path {}` later to retry the blob delete)",
                repo_path,
                repo_path
            );
        }
        Ok(true)
    }

    fn blob_sha(&self, repo_path: &str) -> CliResult<Option<String>> {
        let url = self.contents_url_at_ref(&self.repo_path(repo_path));
        match gh_capture_status(["api", "--jq", ".sha", url.as_str()])? {
            GhApiOutput::Success(file_sha_raw) => {
                let file_sha = file_sha_raw.trim().trim_matches('"').to_string();
                if file_sha.is_empty() {
                    bail!("could not extract sha for {repo_path}");
                }
                Ok(Some(file_sha))
            }
            GhApiOutput::Missing => Ok(None),
        }
    }
}

/// Compose `<prefix>/<path>` for the GitHub Contents API URL, handling the
/// empty-prefix case so we don't emit a leading slash.
pub(crate) fn file_in_repo(root_prefix: &str, file_path: &str) -> String {
    let prefix = root_prefix.trim_matches('/');
    if prefix.is_empty() {
        file_path.to_string()
    } else {
        format!("{prefix}/{file_path}")
    }
}

/// Generic existence check for any path inside the mempool repo. Only GitHub
/// 404/not-found responses are classified as missing; all other `gh` failures
/// are returned as errors.
pub(crate) fn gh_path_status(
    mount: &MempoolMountInfo,
    repo_path: &str,
) -> CliResult<GhResourceStatus> {
    GithubContentsClient::new(mount).path_status(repo_path)
}

/// Two-step add: PUT the file blob, then PUT the rewritten manifest with
/// the new entry inserted. File-first so a step-2 failure leaves the runtime
/// view consistent (manifest is the truth; the orphan file is invisible).
pub(crate) fn add_to_mempool_via_gh(
    mount: &MempoolMountInfo,
    repo_path: &str,
    file_body: &str,
) -> CliResult {
    let entry_path = MempoolEntryPath::parse(repo_path)
        .with_context(|| format!("invalid mempool entry path `{repo_path}`"))?;
    let repo_path = entry_path.as_str();
    let client = GithubContentsClient::new(mount);

    // Step 1: PUT the new file (no sha — it's a create, not an update).
    if !client.put_blob(repo_path, file_body, &format!("mempool: add {repo_path}"))? {
        bail!(
            "file PUT failed for {repo_path} on {}@{}; nothing changed",
            mount.repo,
            mount.branch
        );
    }

    // Step 2: read manifest, insert entry, PUT.
    let remote_manifest = client.get_manifest()?;
    let new_manifest = manifest_with_added_entry(remote_manifest.document, repo_path, file_body)?;
    let new_body = manifest_to_pretty_json(&new_manifest)?;
    if !client.put_manifest(
        &new_body,
        &remote_manifest.sha,
        &format!("mempool: add {repo_path} (manifest)"),
    )? {
        bail!(
            "manifest PUT failed; the file {} is on {}@{} but the manifest doesn't reference \
             it (runtime won't see it). Re-run `websh-cli mempool add` after deleting the file \
             via the GitHub web UI, or manually edit manifest.json.",
            repo_path,
            mount.repo,
            mount.branch
        );
    }

    Ok(())
}

pub(crate) fn fetch_mempool_body(mount: &MempoolMountInfo, repo_path: &str) -> CliResult<String> {
    let entry_path = MempoolEntryPath::parse(repo_path)
        .with_context(|| format!("invalid mempool entry path `{repo_path}`"))?;
    GithubContentsClient::new(mount).fetch_raw(entry_path.as_str())
}

/// Drop a mempool entry via two sequential GitHub Contents API calls:
///
/// 1. Fetch + parse the mempool repo's `manifest.json`, remove the entry,
///    PUT the rewritten manifest (atomically replaces it on the branch).
/// 2. DELETE the file blob.
///
/// Manifest-first order means a step-2 failure leaves the repo in a
/// "dangling blob" state — the manifest no longer references the file but
/// the file still lives in the git tree. The runtime scan reads the
/// manifest, so the user-facing mempool view is correct. The orphan blob
/// is harmless and will be cleaned up the next time the file is committed
/// to (or by `git gc`).
pub(crate) fn drop_via_gh(mount: &MempoolMountInfo, path_in_repo: &str) -> CliResult<DropOutcome> {
    let entry_path = MempoolEntryPath::parse(path_in_repo)
        .with_context(|| format!("invalid mempool entry path `{path_in_repo}`"))?;
    let path_in_repo = entry_path.as_str();
    let client = GithubContentsClient::new(mount);

    // Step 1: rewrite manifest (skip if entry isn't present).
    let remote_manifest = client.get_manifest()?;
    let (manifest, manifest_changed) =
        manifest_without_entry(remote_manifest.document, path_in_repo);

    if manifest_changed {
        let new_body = manifest_to_pretty_json(&manifest)?;
        if !client.put_manifest(
            &new_body,
            &remote_manifest.sha,
            &format!("mempool: drop {path_in_repo}"),
        )? {
            bail!("manifest update failed when dropping {path_in_repo}; nothing else changed");
        }
    }

    // Step 2: delete the file blob (skip cleanly if already absent).
    let blob_deleted = client.delete_blob(path_in_repo)?;

    if !manifest_changed && !blob_deleted {
        Ok(DropOutcome::Absent)
    } else {
        Ok(DropOutcome::Removed {
            manifest: manifest_changed,
            blob: blob_deleted,
        })
    }
}

fn manifest_with_added_entry(
    mut manifest: ContentManifestDocument,
    repo_path: &str,
    file_body: &str,
) -> CliResult<ContentManifestDocument> {
    let canonical_path = VirtualPath::from_absolute(format!("/mempool/{repo_path}"))
        .with_context(|| format!("invalid mempool path /mempool/{repo_path}"))?;
    let MempoolManifestState { meta, extensions } =
        build_mempool_manifest_state(file_body, &canonical_path);
    let new_entry = ContentManifestEntry {
        path: repo_path.to_string(),
        metadata: meta,
        mempool: extensions.mempool,
    };

    manifest.entries.retain(|entry| entry.path != repo_path);
    manifest.entries.push(new_entry);
    manifest.entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(manifest)
}

fn manifest_without_entry(
    mut manifest: ContentManifestDocument,
    path_in_repo: &str,
) -> (ContentManifestDocument, bool) {
    let before = manifest.entries.len();
    manifest.entries.retain(|entry| entry.path != path_in_repo);
    let changed = manifest.entries.len() != before;
    (manifest, changed)
}

fn manifest_to_pretty_json(manifest: &ContentManifestDocument) -> CliResult<String> {
    Ok(serde_json::to_string_pretty(manifest).context("serialize mempool manifest")? + "\n")
}

pub(crate) enum DropOutcome {
    /// At least one of (manifest entry, file blob) was removed.
    Removed { manifest: bool, blob: bool },
    /// Neither manifest nor blob existed.
    Absent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use websh_core::domain::{MempoolStatus, NodeMetadata};

    fn entry(path: &str) -> ContentManifestEntry {
        ContentManifestEntry {
            path: path.to_string(),
            metadata: NodeMetadata::default(),
            mempool: None,
        }
    }

    #[test]
    fn file_in_repo_handles_empty_prefix() {
        assert_eq!(file_in_repo("", "writing/foo.md"), "writing/foo.md");
        assert_eq!(file_in_repo("/", "writing/foo.md"), "writing/foo.md");
    }

    #[test]
    fn file_in_repo_prepends_prefix() {
        assert_eq!(
            file_in_repo("content", "writing/foo.md"),
            "content/writing/foo.md"
        );
        assert_eq!(
            file_in_repo("/content/", "writing/foo.md"),
            "content/writing/foo.md"
        );
    }

    #[test]
    fn manifest_add_replaces_entry_and_sorts() {
        let manifest = ContentManifestDocument {
            entries: vec![
                entry("writing/z.md"),
                entry("writing/foo.md"),
                entry("writing/a.md"),
            ],
        };
        let body = "---\n\
                    title: Foo\n\
                    status: review\n\
                    priority: high\n\
                    modified: 2026-04-28\n\
                    tags: [draft]\n\
                    ---\n\nHello world.\n";

        let manifest = manifest_with_added_entry(manifest, "writing/foo.md", body).unwrap();

        assert_eq!(
            manifest
                .entries
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            vec!["writing/a.md", "writing/foo.md", "writing/z.md"]
        );
        let foo = manifest
            .entries
            .iter()
            .find(|entry| entry.path == "writing/foo.md")
            .unwrap();
        assert_eq!(foo.metadata.authored.title.as_deref(), Some("Foo"));
        assert_eq!(foo.metadata.authored.date.as_deref(), Some("2026-04-28"));
        assert_eq!(
            foo.mempool.as_ref().map(|mempool| mempool.status),
            Some(MempoolStatus::Review)
        );
    }

    #[test]
    fn manifest_drop_removes_entry_and_reports_change() {
        let manifest = ContentManifestDocument {
            entries: vec![entry("writing/a.md"), entry("writing/foo.md")],
        };

        let (manifest, changed) = manifest_without_entry(manifest, "writing/foo.md");

        assert!(changed);
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].path, "writing/a.md");
    }

    #[test]
    fn manifest_drop_absent_is_noop() {
        let manifest = ContentManifestDocument {
            entries: vec![entry("writing/a.md")],
        };

        let (manifest, changed) = manifest_without_entry(manifest, "writing/missing.md");

        assert!(!changed);
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].path, "writing/a.md");
    }

    #[test]
    fn drop_rejects_reserved_manifest_path_before_remote_delete() {
        let err = MempoolEntryPath::parse("manifest.json").unwrap_err();
        assert!(err.to_string().contains("reserved"));
    }
}
