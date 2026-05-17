use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use websh_core::domain::{ContentManifestDocument, ContentManifestEntry};
use websh_core::ports::parse_manifest_snapshot;

use crate::CliResult;
use crate::infra::json::write_json;

use super::files::{
    CONTENT_MANIFEST_FILE, collect_files_recursive, relative_path_from, resolve_path,
    should_skip_content_file, should_skip_primary_content_file,
};
use super::sidecar::{
    default_directory_metadata, default_file_metadata, read_directory_sidecar, read_file_sidecar,
    sync_directory_sidecar, sync_file_sidecar,
};

pub(crate) const DEFAULT_CONTENT_DIR: &str = "content";

/// Canonical entry point: walk the content tree, refresh every node's
/// sidecar (recompute `derived` fields and merge frontmatter into
/// `authored` for markdown files), then fold the sidecars into
/// `manifest.json`.
///
/// This is the only function callers should reach for when they need
/// the manifest to reflect the current on-disk content. The CLI's
/// `content manifest` subcommand and Trunk's pre-build hook both end
/// here. Internal callers that have *just* sync'd and only need to
/// re-fold the manifest after touching `.websh/ledger.json` may use
/// [`build_manifest_from_sidecars`] for the projection-only path.
pub(crate) fn sync_content(root: &Path, content_dir: &Path) -> CliResult<ContentManifestDocument> {
    let content_root = resolve_path(root, content_dir);
    fs::create_dir_all(&content_root)
        .with_context(|| format!("create directory {}", content_root.display()))?;

    let mut all_files = Vec::new();
    collect_files_recursive(&content_root, &mut all_files)?;

    // First pass: refresh every primary file's sidecar.
    for file_path in &all_files {
        let rel_path = relative_path_from(&content_root, file_path)?;
        if should_skip_primary_content_file(&rel_path) {
            continue;
        }
        sync_file_sidecar(&content_root, file_path, &rel_path)?;
    }

    // Second pass: refresh directory sidecars.
    let directories = enumerate_directories_from_files(&content_root, &all_files)?;
    for dir_rel in &directories {
        sync_directory_sidecar(&content_root, dir_rel)?;
    }

    // Third pass: build manifest from current sidecars + the file list
    // we already have on hand.
    bundle_manifest(&content_root, &all_files, &directories)
}

/// Internal-only: re-fold `manifest.json` from existing sidecars without
/// refreshing them. The caller is responsible for ensuring sidecars are
/// already current — this is intended for narrow situations like
/// "rewrote `.websh/ledger.json`, now re-bundle the manifest so the new
/// ledger hash propagates" where doing a full [`sync_content`] would be
/// wasted work.
///
/// Not exposed as a CLI subcommand: external invocations should always
/// go through `content manifest` (i.e. [`sync_content`]) so the manifest
/// is never ahead of the sidecars.
pub(crate) fn build_manifest_from_sidecars(
    root: &Path,
    content_dir: &Path,
) -> CliResult<ContentManifestDocument> {
    let content_root = resolve_path(root, content_dir);
    fs::create_dir_all(&content_root)
        .with_context(|| format!("create directory {}", content_root.display()))?;

    let mut all_files = Vec::new();
    collect_files_recursive(&content_root, &mut all_files)?;
    let directories = enumerate_directories_from_files(&content_root, &all_files)?;
    bundle_manifest(&content_root, &all_files, &directories)
}

/// Project current sidecars + filesystem state into a `manifest.json`
/// document. Pure projection — does not modify sidecars.
fn bundle_manifest(
    content_root: &Path,
    all_files: &[PathBuf],
    directories: &[String],
) -> CliResult<ContentManifestDocument> {
    let mut entries = Vec::new();

    // Directory entries first (canonical order).
    for dir_rel in directories {
        let metadata = read_directory_sidecar(content_root, dir_rel)?
            .unwrap_or_else(|| default_directory_metadata(dir_rel));
        entries.push(ContentManifestEntry {
            path: dir_rel.clone(),
            metadata,
            mempool: None,
        });
    }

    // File entries. The manifest includes `.websh/*.json` artifacts (e.g.
    // ledger.json, attestations.json) so signed/derived data is reachable
    // through the same surface; only sidecars/manifest themselves are
    // skipped.
    let mut file_entries = Vec::new();
    for file_path in all_files {
        let rel_path = relative_path_from(content_root, file_path)?;
        if should_skip_content_file(&rel_path) {
            continue;
        }
        let metadata = read_file_sidecar(content_root, &rel_path)?
            .unwrap_or_else(|| default_file_metadata(file_path, &rel_path));
        file_entries.push(ContentManifestEntry {
            path: rel_path,
            metadata,
            mempool: None,
        });
    }
    file_entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries.extend(file_entries);

    let manifest = ContentManifestDocument { entries };
    validate_manifest(&manifest)?;
    write_json(&content_root.join(CONTENT_MANIFEST_FILE), &manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &ContentManifestDocument) -> CliResult {
    let body = serde_json::to_string(manifest).context("serialize manifest for validation")?;
    parse_manifest_snapshot(&body)?;
    Ok(())
}

fn content_parent_dirs(rel_path: &str) -> Vec<String> {
    let mut parts: Vec<&str> = rel_path.split('/').collect();
    parts.pop();
    let mut out = Vec::new();
    while !parts.is_empty() {
        out.push(parts.join("/"));
        parts.pop();
    }
    out
}

/// Build the sorted directory list from a pre-walked file list. Caller
/// passes `all_files` so the tree isn't walked twice during sync.
fn enumerate_directories_from_files(
    content_root: &Path,
    all_files: &[PathBuf],
) -> CliResult<Vec<String>> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    seen.insert(String::new());
    for file in all_files {
        let rel = relative_path_from(content_root, file)?;
        for parent in content_parent_dirs(&rel) {
            seen.insert(parent);
        }
    }
    Ok(seen.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use websh_core::domain::{
        AccessFilter, BundleValidationError, Fields, NodeKind, NodeMetadata, Recipient,
        SCHEMA_VERSION,
    };

    fn tempdir() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut d = std::env::temp_dir();
        d.push(format!("websh-manifest-test-{}-{}", std::process::id(), id));
        if d.exists() {
            fs::remove_dir_all(&d).unwrap();
        }
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn read_sidecar(path: &Path) -> NodeMetadata {
        let body = fs::read_to_string(path).expect("sidecar exists");
        serde_json::from_str(&body).expect("sidecar parses")
    }

    #[test]
    fn populates_authored_from_frontmatter() {
        let dir = tempdir();
        fs::write(
            dir.join("hello.md"),
            "---\ntitle: Greeting\ntags:\n  - intro\n  - sample\ndate: 2026-04-22\n---\n\nbody\n",
        )
        .unwrap();

        let manifest = sync_content(&dir, Path::new(".")).expect("sync ok");

        let sidecar = read_sidecar(&dir.join("hello.meta.json"));
        assert_eq!(sidecar.authored.title.as_deref(), Some("Greeting"));
        assert_eq!(sidecar.authored.date.as_deref(), Some("2026-04-22"));
        assert_eq!(
            sidecar.authored.tags.as_deref(),
            Some(&["intro".to_string(), "sample".to_string()][..]),
        );

        let entry = manifest
            .entries
            .iter()
            .find(|e| e.path == "hello.md")
            .expect("hello.md in manifest");
        assert_eq!(entry.metadata.authored.title.as_deref(), Some("Greeting"));
        assert_eq!(entry.metadata.kind, NodeKind::Page);
    }

    #[test]
    fn idempotent_across_repeated_runs() {
        let dir = tempdir();
        fs::write(dir.join("note.md"), "---\ntitle: Note\n---\n\ncontent\n").unwrap();

        sync_content(&dir, Path::new(".")).expect("first sync");
        let bytes_a = fs::read(dir.join("manifest.json")).unwrap();
        let sidecar_a = fs::read(dir.join("note.meta.json")).unwrap();

        sync_content(&dir, Path::new(".")).expect("second sync");
        let bytes_b = fs::read(dir.join("manifest.json")).unwrap();
        let sidecar_b = fs::read(dir.join("note.meta.json")).unwrap();

        assert_eq!(bytes_a, bytes_b, "manifest must be byte-equal across syncs");
        assert_eq!(
            sidecar_a, sidecar_b,
            "sidecar must be byte-equal across syncs"
        );
    }

    #[test]
    fn preserves_sidecar_only_authored_fields() {
        let dir = tempdir();

        // Pre-existing sidecar carries an `access` recipient list — the
        // sort of field a user authors directly in the JSON, not via
        // markdown frontmatter. Sync must not clobber it.
        let prior = NodeMetadata {
            schema: SCHEMA_VERSION,
            kind: NodeKind::Page,
            bundle: None,
            authored: Fields {
                access: Some(AccessFilter {
                    recipients: vec![Recipient {
                        address: "0xabc".to_string(),
                    }],
                }),
                ..Fields::default()
            },
            derived: Fields::default(),
        };
        fs::write(
            dir.join("scoped.meta.json"),
            format!("{}\n", serde_json::to_string_pretty(&prior).unwrap()),
        )
        .unwrap();

        // Frontmatter sets `title` only — no `access` key.
        fs::write(dir.join("scoped.md"), "---\ntitle: Scoped\n---\n\nbody\n").unwrap();

        sync_content(&dir, Path::new(".")).expect("sync ok");

        let after = read_sidecar(&dir.join("scoped.meta.json"));
        assert_eq!(after.authored.title.as_deref(), Some("Scoped"));
        let access = after.authored.access.expect("access preserved");
        assert_eq!(access.recipients.len(), 1);
        assert_eq!(access.recipients[0].address, "0xabc");
    }

    #[test]
    fn rejects_bundle_route_collisions_during_manifest_sync() {
        let dir = tempdir();
        fs::create_dir_all(dir.join("writing/foo")).unwrap();
        fs::write(
            dir.join("writing/foo/_index.dir.json"),
            r#"{
              "schema":1,
              "kind":"bundle",
              "bundle":{
                "default_variant":"en",
                "variants":[{"id":"en","path":"en.md","label":"English"}]
              },
              "authored":{"title":"Foo"},
              "derived":{"kind":"bundle"}
            }"#,
        )
        .unwrap();
        fs::write(dir.join("writing/foo/en.md"), b"english").unwrap();
        fs::write(dir.join("writing/foo.md"), b"collision").unwrap();

        let err = sync_content(&dir, Path::new(".")).unwrap_err();
        let manifest_error = err
            .downcast_ref::<websh_core::ports::ManifestSnapshotError>()
            .expect("bundle route collision error");
        assert!(matches!(
            manifest_error,
            websh_core::ports::ManifestSnapshotError::Bundle(
                BundleValidationError::RootRouteCollision { file_path, .. }
            )
                if file_path == "writing/foo.md"
        ));
    }
}
