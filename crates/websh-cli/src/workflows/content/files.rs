use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use websh_core::attestation::artifact::{ContentFile, sha256_hex};
use websh_core::domain::{
    NodeKind, NodeMetadata, validate_bundle_metadata_with_targets, validate_bundle_route_collisions,
};
use websh_core::filesystem::content_route_for_path;
use websh_core::ports::ManifestSnapshotError;

use crate::CliResult;

pub(crate) const CONTENT_MANIFEST_FILE: &str = "manifest.json";

#[derive(Clone, Debug)]
pub(crate) struct BundleContentUnit {
    pub(crate) rel_path: String,
    pub(crate) metadata: NodeMetadata,
    pub(crate) content_paths: Vec<PathBuf>,
}

pub(crate) fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> CliResult {
    if !dir.exists() {
        return Ok(());
    }

    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("read directory {}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("read directory entry in {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type {}", path.display()))?;
        if file_type.is_dir() {
            if entry.file_name() == ".git" {
                continue;
            }
            collect_files_recursive(&path, out)?;
        } else if file_type.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn should_skip_content_file(rel_path: &str) -> bool {
    rel_path == CONTENT_MANIFEST_FILE
        || rel_path.ends_with(".meta.json")
        || rel_path.ends_with("_index.dir.json")
        || rel_path
            .split('/')
            .any(|part| matches!(part, ".DS_Store" | ".gitkeep"))
}

pub(crate) fn should_skip_primary_content_file(rel_path: &str) -> bool {
    should_skip_content_file(rel_path) || rel_path.split('/').any(|part| part == ".websh")
}

pub(crate) fn route_for_content_path(rel_path: &str) -> String {
    content_route_for_path(rel_path)
}

pub(crate) fn kind_for_content_path(rel_path: &str) -> NodeKind {
    match Path::new(rel_path).extension().and_then(|ext| ext.to_str()) {
        Some("md" | "html" | "htm") => NodeKind::Page,
        Some("link") => NodeKind::Redirect,
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg") => NodeKind::Asset,
        Some("pdf") => NodeKind::Document,
        Some("app") => NodeKind::App,
        Some("json") => NodeKind::Data,
        _ => NodeKind::Document,
    }
}

pub(crate) fn resolve_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

pub(crate) fn relative_path_from(root: &Path, path: &Path) -> CliResult<String> {
    let rel = path.strip_prefix(root).with_context(|| {
        format!(
            "path {} is not under root {}",
            path.display(),
            root.display()
        )
    })?;
    Ok(rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/"))
}

pub(crate) fn build_content_files(root: &Path, paths: &[PathBuf]) -> CliResult<Vec<ContentFile>> {
    let mut files = paths
        .iter()
        .map(|path| {
            let artifact_path = artifact_path(root, path)?;
            let resolved = resolve_path(root, path);
            let bytes =
                fs::read(&resolved).with_context(|| format!("read {}", resolved.display()))?;
            Ok(ContentFile {
                path: artifact_path,
                sha256: sha256_hex(&bytes),
                bytes: bytes.len() as u64,
            })
        })
        .collect::<CliResult<Vec<_>>>()?;

    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.windows(2).any(|pair| pair[0].path == pair[1].path) {
        bail!("duplicate content path");
    }

    Ok(files)
}

pub(crate) fn discover_bundle_content_units(
    content_root: &Path,
    all_files: &[PathBuf],
) -> CliResult<Vec<BundleContentUnit>> {
    let mut units = Vec::new();
    for file_path in all_files {
        let rel_path = relative_path_from(content_root, file_path)?;
        if !rel_path.ends_with("_index.dir.json") {
            continue;
        }
        let body = fs::read_to_string(file_path)
            .with_context(|| format!("read {}", file_path.display()))?;
        let metadata: NodeMetadata = serde_json::from_str(&body)
            .with_context(|| format!("parse {}", file_path.display()))?;
        if !metadata.is_bundle() {
            continue;
        }
        let rel_path = bundle_rel_from_sidecar_rel(&rel_path)?;
        let unit = bundle_content_unit(content_root, all_files, rel_path, metadata)?;
        units.push(unit);
    }
    units.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    Ok(units)
}

pub(crate) fn path_is_inside_bundle(rel_path: &str, bundles: &[BundleContentUnit]) -> bool {
    bundles.iter().any(|bundle| {
        bundle.rel_path.is_empty()
            || rel_path == bundle.rel_path
            || rel_path
                .strip_prefix(&bundle.rel_path)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn bundle_content_unit(
    content_root: &Path,
    all_files: &[PathBuf],
    rel_path: String,
    metadata: NodeMetadata,
) -> CliResult<BundleContentUnit> {
    let bundle = metadata
        .bundle
        .as_ref()
        .ok_or_else(|| anyhow!("bundle directory {rel_path} requires a bundle metadata block"))?;
    let mut declared_variant_rels = BTreeSet::new();
    validate_bundle_metadata_with_targets(&rel_path, bundle, |variant| {
        let variant_rel = join_rel_path(&rel_path, &variant.path);
        declared_variant_rels.insert(variant_rel.clone());
        let target = content_root.join(&variant_rel);
        if !target.is_file() {
            return Err(ManifestSnapshotError::MissingBundleVariantTarget {
                bundle_path: rel_path.clone(),
                variant_id: variant.id.clone(),
                path: variant.path.clone(),
            });
        }
        Ok(())
    })?;
    validate_bundle_content_route_collisions(content_root, &rel_path, bundle, all_files)?;

    let mut content_paths = vec![content_root.join(directory_sidecar_rel_path(&rel_path))];
    for variant in &bundle.variants {
        let variant_rel = join_rel_path(&rel_path, &variant.path);
        content_paths.push(content_root.join(&variant_rel));
        let sidecar = file_sidecar_rel_path(&variant_rel);
        let sidecar_path = content_root.join(sidecar);
        if sidecar_path.exists() {
            content_paths.push(sidecar_path);
        }
    }
    for file_path in all_files {
        let support_rel = relative_path_from(content_root, file_path)?;
        if !is_inside_rel_dir(&support_rel, &rel_path) {
            continue;
        }
        if declared_variant_rels.contains(&support_rel)
            || should_skip_bundle_signed_content_file(&support_rel)
        {
            continue;
        }
        content_paths.push(file_path.clone());
    }
    content_paths.sort();
    content_paths.dedup();

    Ok(BundleContentUnit {
        rel_path,
        metadata,
        content_paths,
    })
}

fn validate_bundle_content_route_collisions(
    content_root: &Path,
    rel_path: &str,
    bundle: &websh_core::domain::BundleMetadata,
    all_files: &[PathBuf],
) -> CliResult {
    let mut candidate_rels = Vec::new();
    for file_path in all_files {
        let file_rel = relative_path_from(content_root, file_path)?;
        if !should_skip_bundle_route_collision_file(&file_rel) {
            candidate_rels.push(file_rel);
        }
    }
    validate_bundle_route_collisions(
        rel_path,
        bundle,
        candidate_rels.iter().map(String::as_str),
        route_for_content_path,
    )
    .map_err(Into::into)
}

fn bundle_rel_from_sidecar_rel(sidecar_rel: &str) -> CliResult<String> {
    if sidecar_rel == "_index.dir.json" {
        return Ok(String::new());
    }
    sidecar_rel
        .strip_suffix("/_index.dir.json")
        .map(str::to_string)
        .ok_or_else(|| anyhow!("not a directory sidecar path: {sidecar_rel}"))
}

fn directory_sidecar_rel_path(dir_rel: &str) -> String {
    if dir_rel.is_empty() {
        "_index.dir.json".to_string()
    } else {
        format!("{dir_rel}/_index.dir.json")
    }
}

fn file_sidecar_rel_path(rel_path: &str) -> String {
    let path = Path::new(rel_path);
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let sidecar = format!("{stem}.meta.json");
    if parent.as_os_str().is_empty() {
        sidecar
    } else {
        format!("{}/{}", parent.to_string_lossy(), sidecar)
    }
}

fn join_rel_path(base: &str, child: &str) -> String {
    if base.is_empty() {
        child.to_string()
    } else {
        format!("{base}/{child}")
    }
}

fn is_inside_rel_dir(rel_path: &str, dir_rel: &str) -> bool {
    dir_rel.is_empty()
        || rel_path
            .strip_prefix(dir_rel)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn should_skip_bundle_route_collision_file(rel_path: &str) -> bool {
    should_skip_bundle_generated_or_system_file(rel_path)
        || Path::new(rel_path)
            .file_name()
            .map(|name| name.to_string_lossy().ends_with(".meta.json"))
            .unwrap_or(false)
}

fn should_skip_bundle_signed_content_file(rel_path: &str) -> bool {
    should_skip_bundle_generated_or_system_file(rel_path)
}

fn should_skip_bundle_generated_or_system_file(rel_path: &str) -> bool {
    let name = Path::new(rel_path)
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();

    name == CONTENT_MANIFEST_FILE
        || name == "_index.dir.json"
        || rel_path
            .split('/')
            .any(|part| matches!(part, ".websh" | ".git" | ".DS_Store" | ".gitkeep"))
}

pub(crate) fn artifact_path(root: &Path, path: &Path) -> CliResult<String> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root)
            .with_context(|| format!("path {} is outside root {}", path.display(), root.display()))?
            .to_path_buf()
    } else {
        path.to_path_buf()
    };

    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                bail!("path {} escapes the project root", path.display());
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("unsupported path {}", path.display());
            }
        }
    }

    if parts.is_empty() {
        bail!("empty content path");
    }
    Ok(parts.join("/"))
}
