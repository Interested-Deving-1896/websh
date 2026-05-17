use std::path::Path;

use websh_core::domain::NodeKind;

use crate::CliResult;
use crate::workflows::content::matching_file_sidecar;
use crate::workflows::content::{
    collect_files_recursive, discover_bundle_content_units, kind_for_content_path,
    path_is_inside_bundle, relative_path_from, resolve_path, route_for_content_path,
    should_skip_primary_content_file,
};

use super::subject::{SubjectKind, SubjectSpec};

pub(super) fn discover_subject_specs(
    root: &Path,
    content_dir: &Path,
) -> CliResult<Vec<SubjectSpec>> {
    let content_root = resolve_path(root, content_dir);
    let mut files = Vec::new();
    collect_files_recursive(&content_root, &mut files)?;

    let mut specs = Vec::new();
    let bundles = discover_bundle_content_units(&content_root, &files)?;
    for bundle in &bundles {
        specs.push(SubjectSpec {
            route: route_for_content_path(&bundle.rel_path),
            kind: SubjectKind::Bundle,
            content_paths: bundle.content_paths.clone(),
        });
    }

    for file_path in files {
        let rel_path = relative_path_from(&content_root, &file_path)?;
        if should_skip_primary_content_file(&rel_path) {
            continue;
        }
        if path_is_inside_bundle(&rel_path, &bundles) {
            continue;
        }
        let mut content_paths = vec![file_path.clone()];
        if let Some(sidecar) = matching_file_sidecar(&content_root, &rel_path) {
            content_paths.push(sidecar);
        }
        let kind = subject_kind_for_node_kind(kind_for_content_path(&rel_path));
        specs.push(SubjectSpec {
            route: route_for_content_path(&rel_path),
            kind,
            content_paths,
        });
    }
    specs.sort_by(|left, right| left.route.cmp(&right.route));
    Ok(specs)
}

fn subject_kind_for_node_kind(kind: NodeKind) -> SubjectKind {
    match kind {
        NodeKind::Bundle => SubjectKind::Bundle,
        NodeKind::Page => SubjectKind::Page,
        _ => SubjectKind::Document,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root(name: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        root.push(format!(
            "websh-attest-discover-test-{name}-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn discovers_bundle_support_assets_as_bundle_subject_content() {
        let root = temp_root("bundle-assets");
        let content = root.join("content");
        fs::create_dir_all(content.join("writing/foo")).unwrap();
        fs::write(
            content.join("writing/foo/_index.dir.json"),
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
        fs::write(content.join("writing/foo/en.md"), b"english").unwrap();
        fs::write(content.join("writing/foo/cover.png"), b"png").unwrap();
        fs::write(
            content.join("writing/foo/cover.meta.json"),
            b"{\"schema\":1,\"authored\":{\"title\":\"Cover\"}}",
        )
        .unwrap();

        let specs = discover_subject_specs(&root, Path::new("content")).unwrap();

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].route, "/writing/foo");
        assert_eq!(specs[0].kind, SubjectKind::Bundle);
        let paths = specs[0]
            .content_paths
            .iter()
            .map(|path| {
                path.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert!(paths.contains(&"content/writing/foo/_index.dir.json".to_string()));
        assert!(paths.contains(&"content/writing/foo/en.md".to_string()));
        assert!(paths.contains(&"content/writing/foo/cover.png".to_string()));
        assert!(paths.contains(&"content/writing/foo/cover.meta.json".to_string()));

        fs::remove_dir_all(root).unwrap();
    }
}
