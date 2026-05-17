use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use sha2::{Digest, Sha256};

use websh_core::domain::{
    Fields, NodeKind, NodeMetadata, RendererKind, SCHEMA_VERSION,
    validate_bundle_metadata_with_targets,
};
use websh_core::ports::ManifestSnapshotError;

use crate::CliResult;
use crate::infra::json::write_json;

use super::files::{kind_for_content_path, should_skip_content_file};
use super::frontmatter::{merge_authored, parse_yaml_frontmatter};
use super::media::derived_for_path;

/// Refresh the sidecar JSON for a primary file. Reads the file's
/// extension/contents to compute derived fields; for markdown files,
/// reads YAML frontmatter and stores it in the sidecar's `authored`
/// section. The previously authored fields are preserved when no
/// frontmatter exists (and for non-markdown files in general).
pub(crate) fn sync_file_sidecar(
    content_root: &Path,
    file_path: &Path,
    rel_path: &str,
) -> CliResult {
    let metadata = fs::metadata(file_path)
        .with_context(|| format!("read metadata {}", file_path.display()))?;
    let bytes = fs::read(file_path).with_context(|| format!("read {}", file_path.display()))?;

    let kind = kind_for_content_path(rel_path);
    let mut derived = derived_for_path(file_path, rel_path, &bytes)?;
    derived.title = Some(fallback_file_title(rel_path));
    derived.kind = Some(kind);
    derived.renderer = derived
        .renderer
        .or_else(|| default_renderer_for_kind(kind, rel_path));
    derived.size_bytes = Some(metadata.len());
    // `modified_at` is deliberately omitted for files: filesystem mtime
    // is the checkout wall-clock under git, so it diverges across clones
    // and breaks byte-stability of the sidecar (which feeds into signed
    // attestations). `content_sha256` is the canonical change-detection
    // signal.
    derived.content_sha256 = Some(format!("0x{}", hex::encode(Sha256::digest(&bytes))));

    let sidecar_path = sidecar_path_for(content_root, rel_path);
    let existing = read_sidecar_metadata(&sidecar_path)?;
    let prior_authored = existing
        .as_ref()
        .map(|m| m.authored.clone())
        .unwrap_or_default();

    // For markdown files, frontmatter is the authoring source — but it
    // wins per-field, not whole-cloth. Sidecar-only fields (e.g. `access`,
    // `route`, `trust`) that the frontmatter doesn't mention are
    // preserved.
    let authored = if rel_path.ends_with(".md") {
        match parse_yaml_frontmatter(std::str::from_utf8(&bytes).unwrap_or_default())? {
            Some(frontmatter) => merge_authored(prior_authored, frontmatter),
            None => prior_authored,
        }
    } else {
        prior_authored
    };

    let new_meta = NodeMetadata {
        schema: SCHEMA_VERSION,
        kind,
        bundle: None,
        authored,
        derived,
    };
    write_json(&sidecar_path, &new_meta)
}

pub(crate) fn sync_directory_sidecar(content_root: &Path, dir_rel: &str) -> CliResult {
    let sidecar_path = directory_sidecar_path_for(content_root, dir_rel);
    let existing = read_sidecar_metadata(&sidecar_path)?;
    let dir_path = if dir_rel.is_empty() {
        content_root.to_path_buf()
    } else {
        content_root.join(dir_rel)
    };

    // Directory mtime is deliberately omitted. Writing sidecars during a
    // sync bumps the directory's mtime, so storing it would make sidecars
    // non-byte-stable across consecutive sync runs (and would invalidate
    // attestations that signed the previous canonical content). The
    // `child_count` field is the cheap "did membership change" indicator.
    let directory_kind = match existing.as_ref().map(|metadata| metadata.kind) {
        Some(kind) if kind.is_directory_like() => kind,
        Some(kind) => {
            bail!("directory sidecar {dir_rel} has non-directory top-level kind `{kind:?}`");
        }
        None => NodeKind::Directory,
    };
    if existing
        .as_ref()
        .is_some_and(|metadata| metadata.bundle.is_some() && directory_kind != NodeKind::Bundle)
    {
        bail!("directory {dir_rel} has bundle metadata but kind is not `bundle`");
    }

    let bundle = if directory_kind == NodeKind::Bundle {
        let bundle = existing
            .as_ref()
            .and_then(|metadata| metadata.bundle.clone())
            .ok_or_else(|| {
                anyhow!("bundle directory {dir_rel} requires a bundle metadata block")
            })?;
        validate_bundle_metadata_with_targets(dir_rel, &bundle, |variant| {
            let target = dir_path.join(&variant.path);
            if !target.is_file() {
                return Err(ManifestSnapshotError::MissingBundleVariantTarget {
                    bundle_path: dir_rel.to_string(),
                    variant_id: variant.id.clone(),
                    path: variant.path.clone(),
                });
            }
            Ok(())
        })?;
        Some(bundle)
    } else {
        None
    };

    let derived = Fields {
        title: Some(dir_title_fallback(dir_rel)),
        kind: Some(directory_kind),
        child_count: Some(count_children(&dir_path)?),
        ..Fields::default()
    };

    let authored = existing
        .as_ref()
        .map(|m| m.authored.clone())
        .unwrap_or_default();

    let new_meta = NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: directory_kind,
        bundle,
        authored,
        derived,
    };
    write_json(&sidecar_path, &new_meta)
}

pub(crate) fn read_file_sidecar(
    content_root: &Path,
    rel_path: &str,
) -> CliResult<Option<NodeMetadata>> {
    read_sidecar_metadata(&sidecar_path_for(content_root, rel_path))
}

pub(crate) fn read_directory_sidecar(
    content_root: &Path,
    dir_rel: &str,
) -> CliResult<Option<NodeMetadata>> {
    read_sidecar_metadata(&directory_sidecar_path_for(content_root, dir_rel))
}

fn read_sidecar_metadata(sidecar_path: &Path) -> CliResult<Option<NodeMetadata>> {
    if !sidecar_path.exists() {
        return Ok(None);
    }
    let body = fs::read_to_string(sidecar_path)
        .with_context(|| format!("read {}", sidecar_path.display()))?;
    let metadata: NodeMetadata =
        serde_json::from_str(&body).with_context(|| format!("parse {}", sidecar_path.display()))?;
    Ok(Some(metadata))
}

fn sidecar_path_for(content_root: &Path, rel_path: &str) -> PathBuf {
    let stem_path = Path::new(rel_path);
    let parent = stem_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = stem_path
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    content_root.join(parent).join(format!("{stem}.meta.json"))
}

fn directory_sidecar_path_for(content_root: &Path, dir_rel: &str) -> PathBuf {
    if dir_rel.is_empty() {
        content_root.join("_index.dir.json")
    } else {
        content_root.join(dir_rel).join("_index.dir.json")
    }
}

fn count_children(dir: &Path) -> CliResult<u32> {
    let mut count = 0u32;
    for entry in fs::read_dir(dir).with_context(|| format!("read directory {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read directory entry in {}", dir.display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Filter list mirrors `should_skip_content_file` so the count
        // matches the manifest entry count for this directory.
        if name == ".git" || should_skip_content_file(&name) {
            continue;
        }
        count += 1;
    }
    Ok(count)
}

fn dir_title_fallback(dir_rel: &str) -> String {
    if dir_rel.is_empty() {
        "Home".to_string()
    } else {
        Path::new(dir_rel)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| dir_rel.to_string())
    }
}

pub(crate) fn default_file_metadata(file_path: &Path, rel_path: &str) -> NodeMetadata {
    let kind = kind_for_content_path(rel_path);
    let size = fs::metadata(file_path).ok().map(|m| m.len());

    NodeMetadata {
        schema: SCHEMA_VERSION,
        kind,
        bundle: None,
        authored: Fields::default(),
        derived: Fields {
            title: Some(fallback_file_title(rel_path)),
            kind: Some(kind),
            renderer: default_renderer_for_kind(kind, rel_path),
            size_bytes: size,
            ..Fields::default()
        },
    }
}

pub(crate) fn default_directory_metadata(dir_rel: &str) -> NodeMetadata {
    NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: NodeKind::Directory,
        bundle: None,
        authored: Fields::default(),
        derived: Fields {
            title: Some(dir_title_fallback(dir_rel)),
            kind: Some(NodeKind::Directory),
            ..Fields::default()
        },
    }
}

fn default_renderer_for_kind(kind: NodeKind, rel_path: &str) -> Option<RendererKind> {
    let ext = Path::new(rel_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_lowercase());
    match (kind, ext.as_deref()) {
        (NodeKind::Page, Some("md")) => Some(RendererKind::MarkdownPage),
        (NodeKind::Page, Some("html" | "htm")) => Some(RendererKind::HtmlPage),
        (NodeKind::Document, Some("pdf")) => Some(RendererKind::Pdf),
        (NodeKind::Asset, Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg")) => {
            Some(RendererKind::Image)
        }
        (NodeKind::Redirect, _) => Some(RendererKind::Redirect),
        (NodeKind::App, _) => Some(RendererKind::TerminalApp),
        (NodeKind::Directory, _) => Some(RendererKind::DirectoryListing),
        (NodeKind::Bundle, _) => None,
        _ => None,
    }
}

fn fallback_file_title(rel_path: &str) -> String {
    Path::new(rel_path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| rel_path.to_string())
}

/// Resolve the `<rel_path>.meta.json` sidecar for a primary file, if it
/// exists. Returns `None` for `.meta.json` paths themselves and for
/// `_index.dir.json`.
pub(crate) fn matching_file_sidecar(content_root: &Path, rel_path: &str) -> Option<PathBuf> {
    let path = Path::new(rel_path);
    let name = path.file_name()?.to_string_lossy();
    if name.ends_with(".meta.json") || name == "_index.dir.json" {
        return None;
    }
    let sidecar = sidecar_path_for(content_root, rel_path);
    sidecar.exists().then_some(sidecar)
}
