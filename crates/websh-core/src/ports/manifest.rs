use std::collections::BTreeMap;

use crate::domain::{
    BundleValidationError, ContentManifestDocument, ContentManifestEntry, EntryExtensions,
    NodeKind, NodeMetadata, validate_bundle_metadata_with_targets,
    validate_bundle_route_collisions,
};
use crate::filesystem::content_route_for_path;

use super::{ScannedDirectory, ScannedFile, ScannedSubtree};

pub type ManifestSnapshotResult<T> = Result<T, ManifestSnapshotError>;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ManifestPathError {
    #[error("path must not be empty")]
    EmptyFilePath,
    #[error("path must be repo-relative: {path}")]
    Absolute { path: String },
    #[error("path must use forward slashes only: {path}")]
    Backslash { path: String },
    #[error("path contains an empty segment: {path}")]
    EmptySegment { path: String },
    #[error("path contains traversal segment: {path}")]
    TraversalSegment { path: String },
    #[error("path contains a control character: {path}")]
    ControlCharacter { path: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestSnapshotError {
    #[error("manifest json is invalid: {source}")]
    Json {
        #[from]
        source: serde_json::Error,
    },
    #[error("duplicate manifest path: {path}")]
    DuplicatePath { path: String },
    #[error("invalid manifest path `{path}`: {reason}")]
    InvalidPath {
        path: String,
        reason: ManifestPathError,
    },
    #[error("path {path} has bundle metadata but kind is not `bundle`")]
    BundleMetadataOnNonBundleKind { path: String },
    #[error("bundle {path} has derived.kind that does not match top-level kind")]
    BundleDerivedKindMismatch { path: String },
    #[error("bundle {path} requires a bundle metadata block")]
    MissingBundleMetadata { path: String },
    #[error("bundle {bundle_path} variant `{variant_id}` points to directory `{path}`")]
    BundleVariantTargetIsDirectory {
        bundle_path: String,
        variant_id: String,
        path: String,
    },
    #[error("bundle {bundle_path} variant `{variant_id}` points to missing manifest file `{path}`")]
    MissingBundleVariantTarget {
        bundle_path: String,
        variant_id: String,
        path: String,
    },
    #[error(transparent)]
    Bundle(#[from] BundleValidationError),
}

pub fn parse_manifest_snapshot(body: &str) -> ManifestSnapshotResult<ScannedSubtree> {
    let manifest: ContentManifestDocument = serde_json::from_str(body)?;

    let mut files = Vec::new();
    let mut directories = Vec::new();

    let entry_kinds = manifest_entry_kinds(&manifest.entries)?;

    for entry in manifest.entries {
        let is_dir = entry.metadata.kind.is_directory_like();
        validate_manifest_path(&entry.path, is_dir)?;
        validate_manifest_metadata(&entry.path, &entry.metadata, &entry_kinds)?;
        if is_dir {
            directories.push(ScannedDirectory {
                path: entry.path,
                meta: entry.metadata,
            });
        } else {
            files.push(ScannedFile {
                path: entry.path,
                meta: entry.metadata,
                extensions: EntryExtensions {
                    mempool: entry.mempool,
                },
            });
        }
    }

    Ok(ScannedSubtree { files, directories })
}

pub fn serialize_manifest_snapshot(snapshot: &ScannedSubtree) -> ManifestSnapshotResult<String> {
    let mut entries = Vec::with_capacity(snapshot.files.len() + snapshot.directories.len());

    for dir in &snapshot.directories {
        validate_manifest_path(&dir.path, true)?;
        entries.push(ContentManifestEntry {
            path: dir.path.clone(),
            metadata: dir.meta.clone(),
            mempool: None,
        });
    }
    for file in &snapshot.files {
        validate_manifest_path(&file.path, false)?;
        entries.push(ContentManifestEntry {
            path: file.path.clone(),
            metadata: file.meta.clone(),
            mempool: file.extensions.mempool.clone(),
        });
    }

    let manifest = ContentManifestDocument { entries };
    serde_json::to_string_pretty(&manifest).map_err(Into::into)
}

fn manifest_entry_kinds(
    entries: &[ContentManifestEntry],
) -> ManifestSnapshotResult<BTreeMap<String, NodeKind>> {
    let mut paths = BTreeMap::new();
    for entry in entries {
        if paths
            .insert(entry.path.clone(), entry.metadata.kind)
            .is_some()
        {
            return Err(ManifestSnapshotError::DuplicatePath {
                path: entry.path.clone(),
            });
        }
    }
    Ok(paths)
}

fn validate_manifest_metadata(
    path: &str,
    metadata: &NodeMetadata,
    entry_kinds: &BTreeMap<String, NodeKind>,
) -> ManifestSnapshotResult<()> {
    if metadata.bundle.is_some() && metadata.kind != NodeKind::Bundle {
        return Err(ManifestSnapshotError::BundleMetadataOnNonBundleKind {
            path: display_manifest_path(path).to_string(),
        });
    }

    if metadata.kind == NodeKind::Bundle {
        if metadata
            .derived
            .kind
            .is_some_and(|kind| kind != NodeKind::Bundle)
        {
            return Err(ManifestSnapshotError::BundleDerivedKindMismatch {
                path: display_manifest_path(path).to_string(),
            });
        }
        let bundle = metadata.bundle.as_ref().ok_or_else(|| {
            ManifestSnapshotError::MissingBundleMetadata {
                path: display_manifest_path(path).to_string(),
            }
        })?;
        validate_bundle_metadata_with_targets(path, bundle, |variant| {
            let target = join_manifest_path(path, &variant.path);
            match entry_kinds.get(&target) {
                Some(kind) if !kind.is_directory_like() => Ok(()),
                Some(_) => Err(ManifestSnapshotError::BundleVariantTargetIsDirectory {
                    bundle_path: display_manifest_path(path).to_string(),
                    variant_id: variant.id.clone(),
                    path: variant.path.clone(),
                }),
                None => Err(ManifestSnapshotError::MissingBundleVariantTarget {
                    bundle_path: display_manifest_path(path).to_string(),
                    variant_id: variant.id.clone(),
                    path: variant.path.clone(),
                }),
            }
        })?;
        validate_bundle_route_collisions(
            path,
            bundle,
            entry_kinds
                .iter()
                .filter(|(_, kind)| !kind.is_directory_like())
                .map(|(entry_path, _)| entry_path.as_str()),
            content_route_for_path,
        )?;
    }

    Ok(())
}

fn join_manifest_path(base: &str, child: &str) -> String {
    if base.is_empty() {
        child.to_string()
    } else {
        format!("{base}/{child}")
    }
}

fn display_manifest_path(path: &str) -> &str {
    if path.is_empty() { "/" } else { path }
}

fn validate_manifest_path(path: &str, allow_empty: bool) -> ManifestSnapshotResult<()> {
    if path.is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(ManifestSnapshotError::InvalidPath {
                path: path.to_string(),
                reason: ManifestPathError::EmptyFilePath,
            })
        };
    }
    if path.starts_with('/') {
        return Err(ManifestSnapshotError::InvalidPath {
            path: path.to_string(),
            reason: ManifestPathError::Absolute {
                path: path.to_string(),
            },
        });
    }
    if path.contains('\\') {
        return Err(ManifestSnapshotError::InvalidPath {
            path: path.to_string(),
            reason: ManifestPathError::Backslash {
                path: path.to_string(),
            },
        });
    }
    for segment in path.split('/') {
        if segment.is_empty() {
            return Err(ManifestSnapshotError::InvalidPath {
                path: path.to_string(),
                reason: ManifestPathError::EmptySegment {
                    path: path.to_string(),
                },
            });
        }
        if segment == "." || segment == ".." {
            return Err(ManifestSnapshotError::InvalidPath {
                path: path.to_string(),
                reason: ManifestPathError::TraversalSegment {
                    path: path.to_string(),
                },
            });
        }
        if segment.chars().any(char::is_control) {
            return Err(ManifestSnapshotError::InvalidPath {
                path: path.to_string(),
                reason: ManifestPathError::ControlCharacter {
                    path: path.to_string(),
                },
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::domain::{Fields, NodeKind, NodeMetadata, SCHEMA_VERSION};

    use super::*;

    #[test]
    fn round_trips_manifest_document() {
        let snapshot = ScannedSubtree {
            files: vec![ScannedFile {
                path: "about.md".to_string(),
                meta: NodeMetadata {
                    schema: SCHEMA_VERSION,
                    kind: NodeKind::Page,
                    bundle: None,
                    authored: Fields {
                        title: Some("About".to_string()),
                        date: Some("2026-04-26".to_string()),
                        tags: Some(vec!["intro".to_string()]),
                        ..Fields::default()
                    },
                    derived: Fields {
                        size_bytes: Some(7),
                        modified_at: Some(42),
                        ..Fields::default()
                    },
                },
                extensions: EntryExtensions::default(),
            }],
            directories: vec![ScannedDirectory {
                path: String::new(),
                meta: NodeMetadata {
                    schema: SCHEMA_VERSION,
                    kind: NodeKind::Directory,
                    bundle: None,
                    authored: Fields {
                        title: Some("Home".to_string()),
                        tags: Some(vec!["root".to_string()]),
                        ..Fields::default()
                    },
                    derived: Fields::default(),
                },
            }],
        };

        let encoded = serialize_manifest_snapshot(&snapshot).expect("serialize");
        let decoded = parse_manifest_snapshot(&encoded).expect("parse");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn rejects_manifest_paths_with_traversal_segments() {
        let manifest = r#"{
            "entries": [
                {"path":"../secret.md","metadata":{"schema":1,"kind":"page","authored":{},"derived":{}}}
            ]
        }"#;

        let err = parse_manifest_snapshot(manifest).unwrap_err();
        assert!(matches!(
            err,
            ManifestSnapshotError::InvalidPath {
                reason: ManifestPathError::TraversalSegment { .. },
                ..
            }
        ));
    }

    #[test]
    fn parses_bundle_manifest_when_declared_variant_files_exist() {
        let manifest = r#"{
            "entries": [
                {
                    "path":"writing/foo",
                    "metadata":{
                        "schema":1,
                        "kind":"bundle",
                        "bundle":{
                            "default_variant":"en",
                            "variants":[
                                {"id":"en","path":"en.md","label":"English"},
                                {"id":"ko","path":"ko.md","label":"Korean"}
                            ]
                        },
                        "authored":{},
                        "derived":{"kind":"bundle"}
                    }
                },
                {"path":"writing/foo/en.md","metadata":{"schema":1,"kind":"page","authored":{},"derived":{}}},
                {"path":"writing/foo/ko.md","metadata":{"schema":1,"kind":"page","authored":{},"derived":{}}}
            ]
        }"#;

        let snapshot = parse_manifest_snapshot(manifest).expect("parse bundle manifest");
        assert_eq!(snapshot.directories.len(), 1);
        assert_eq!(snapshot.files.len(), 2);
        assert!(snapshot.directories[0].meta.is_bundle());
    }

    #[test]
    fn rejects_bundle_metadata_on_non_bundle_kind() {
        let manifest = r#"{
            "entries": [
                {
                    "path":"writing/foo",
                    "metadata":{
                        "schema":1,
                        "kind":"directory",
                        "bundle":{"default_variant":"en","variants":[]},
                        "authored":{},
                        "derived":{"kind":"directory"}
                    }
                }
            ]
        }"#;

        let err = parse_manifest_snapshot(manifest).unwrap_err();
        assert!(matches!(
            err,
            ManifestSnapshotError::BundleMetadataOnNonBundleKind { .. }
        ));
    }

    #[test]
    fn rejects_bundle_manifest_without_metadata_block() {
        let manifest = r#"{
            "entries": [
                {"path":"writing/foo","metadata":{"schema":1,"kind":"bundle","authored":{},"derived":{"kind":"bundle"}}}
            ]
        }"#;

        let err = parse_manifest_snapshot(manifest).unwrap_err();
        assert!(matches!(
            err,
            ManifestSnapshotError::MissingBundleMetadata { .. }
        ));
    }

    #[test]
    fn rejects_bundle_manifest_with_missing_variant_file() {
        let manifest = r#"{
            "entries": [
                {
                    "path":"writing/foo",
                    "metadata":{
                        "schema":1,
                        "kind":"bundle",
                        "bundle":{
                            "default_variant":"en",
                            "variants":[{"id":"en","path":"en.md","label":"English"}]
                        },
                        "authored":{},
                        "derived":{"kind":"bundle"}
                    }
                }
            ]
        }"#;

        let err = parse_manifest_snapshot(manifest).unwrap_err();
        assert!(matches!(
            err,
            ManifestSnapshotError::MissingBundleVariantTarget { .. }
        ));
    }

    #[test]
    fn rejects_bundle_manifest_with_route_unsafe_variant_id() {
        let manifest = r#"{
            "entries": [
                {
                    "path":"writing/foo",
                    "metadata":{
                        "schema":1,
                        "kind":"bundle",
                        "bundle":{
                            "default_variant":"ko.md",
                            "variants":[{"id":"ko.md","path":"ko.md","label":"Korean"}]
                        },
                        "authored":{},
                        "derived":{"kind":"bundle"}
                    }
                },
                {"path":"writing/foo/ko.md","metadata":{"schema":1,"kind":"page","authored":{},"derived":{}}}
            ]
        }"#;

        let err = parse_manifest_snapshot(manifest).unwrap_err();
        assert!(matches!(
            err,
            ManifestSnapshotError::Bundle(BundleValidationError::InvalidVariantId { .. })
        ));
    }

    #[test]
    fn rejects_bundle_manifest_with_root_route_collision() {
        let manifest = r#"{
            "entries": [
                {
                    "path":"writing/foo",
                    "metadata":{
                        "schema":1,
                        "kind":"bundle",
                        "bundle":{
                            "default_variant":"en",
                            "variants":[{"id":"en","path":"en.md","label":"English"}]
                        },
                        "authored":{},
                        "derived":{"kind":"bundle"}
                    }
                },
                {"path":"writing/foo/en.md","metadata":{"schema":1,"kind":"page","authored":{},"derived":{}}},
                {"path":"writing/foo.md","metadata":{"schema":1,"kind":"page","authored":{},"derived":{}}}
            ]
        }"#;

        let err = parse_manifest_snapshot(manifest).unwrap_err();
        assert!(matches!(
            err,
            ManifestSnapshotError::Bundle(BundleValidationError::RootRouteCollision { .. })
        ));
    }
}
