use std::fs;
use std::path::Path;

use anyhow::Context;
use websh_core::attestation::artifact::subject_id_for_route;
use websh_core::attestation::ledger::{
    CONTENT_LEDGER_CONTENT_PATH, ContentLedger, ContentLedgerCategory, ContentLedgerEntry,
    ContentLedgerInput, ContentLedgerSortKey,
};
use websh_core::support::format::iso_date_prefix;

use super::files::{
    BundleContentUnit, build_content_files, collect_files_recursive, discover_bundle_content_units,
    path_is_inside_bundle, relative_path_from, resolve_path, route_for_content_path,
    should_skip_primary_content_file,
};
use super::frontmatter::content_entry_raw_date;
use super::sidecar::matching_file_sidecar;
use crate::CliResult;
use crate::infra::json::write_json;

pub(crate) fn generate_content_ledger(root: &Path, content_dir: &Path) -> CliResult<ContentLedger> {
    let content_root = resolve_path(root, content_dir);
    fs::create_dir_all(&content_root)
        .with_context(|| format!("create directory {}", content_root.display()))?;

    let mut files = Vec::new();
    collect_files_recursive(&content_root, &mut files)?;

    let mut staged: Vec<ContentLedgerInput> = Vec::new();
    let bundles = discover_bundle_content_units(&content_root, &files)?;
    for bundle in &bundles {
        let route = route_for_content_path(&bundle.rel_path);
        let content_files = build_content_files(root, &bundle.content_paths)?;
        let sort_date = sort_date_for_bundle(bundle);
        let category = ContentLedgerCategory::for_path(&bundle.rel_path);
        staged.push(ContentLedgerInput::new(
            ContentLedgerSortKey::new(sort_date, bundle.rel_path.clone()),
            ContentLedgerEntry::new(
                subject_id_for_route(&route),
                route,
                bundle.rel_path.clone(),
                category,
                content_files,
            )?,
        ));
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

        let route = route_for_content_path(&rel_path);
        let content_files = build_content_files(root, &content_paths)?;
        let sort_date = sort_date_for_entry(&content_root, &file_path, &rel_path);
        let category = ContentLedgerCategory::for_path(&rel_path);
        staged.push(ContentLedgerInput::new(
            ContentLedgerSortKey::new(sort_date, rel_path.clone()),
            ContentLedgerEntry::new(
                subject_id_for_route(&route),
                route,
                rel_path,
                category,
                content_files,
            )?,
        ));
    }
    // `ContentLedger::new` owns canonical block ordering and hash chaining:
    // `(sort_key.date asc with None first, sort_key.path asc)`.
    let ledger = ContentLedger::new(staged)?;
    ledger.validate()?;

    let ledger_path = content_root.join(CONTENT_LEDGER_CONTENT_PATH);
    if let Some(parent) = ledger_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    write_json(&ledger_path, &ledger)?;

    Ok(ledger)
}

fn sort_date_for_entry(content_root: &Path, file_path: &Path, rel_path: &str) -> Option<String> {
    content_entry_raw_date(content_root, file_path, rel_path)
        .as_deref()
        .and_then(iso_date_prefix)
        .map(|date| date.to_string())
}

fn sort_date_for_bundle(bundle: &BundleContentUnit) -> Option<String> {
    bundle
        .metadata
        .date()
        .and_then(iso_date_prefix)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use websh_core::domain::BundleValidationError;

    fn temp_root(name: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        root.push(format!("websh-ledger-test-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn ledger_groups_sidecars_and_excludes_generated_files() {
        let root = temp_root("sidecar");
        let content = root.join("content");
        fs::create_dir_all(content.join("talks")).unwrap();
        fs::create_dir_all(content.join(".websh")).unwrap();
        fs::write(content.join("manifest.json"), "{}").unwrap();
        fs::write(content.join(".websh/old.json"), "{}").unwrap();
        fs::write(content.join("talks/a.pdf"), b"pdf").unwrap();
        fs::write(
            content.join("talks/a.meta.json"),
            r#"{"schema":1,"kind":"document","authored":{"title":"Talk","tags":["zk"],"date":"2026-04-01"},"derived":{}}"#,
        )
        .unwrap();

        let ledger = generate_content_ledger(&root, Path::new("content")).unwrap();
        assert_eq!(ledger.blocks.len(), 1);
        let entry = &ledger.blocks[0].entry;
        assert_eq!(entry.path, "talks/a.pdf");
        assert_eq!(entry.route, "/talks/a.pdf");
        assert_eq!(entry.category, ContentLedgerCategory::Talks);
        assert_eq!(
            entry
                .content_files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["content/talks/a.meta.json", "content/talks/a.pdf"]
        );

        let encoded = serde_json::to_string(&ledger).unwrap();
        assert!(!encoded.contains("\"title\""));
        assert!(!encoded.contains("\"tags\""));
        assert!(root.join("content/.websh/ledger.json").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ledger_sorts_entries_by_date_with_path_tiebreaker() {
        let root = temp_root("date-sort");
        let content = root.join("content");
        fs::create_dir_all(content.join("writing")).unwrap();
        fs::create_dir_all(content.join("papers")).unwrap();
        fs::create_dir_all(content.join("misc")).unwrap();

        // Frontmatter-dated markdown.
        fs::write(
            content.join("writing/old.md"),
            "---\ndate: \"2026-01-15\"\n---\nold writing\n",
        )
        .unwrap();
        fs::write(
            content.join("writing/new.md"),
            "---\ndate: \"2026-04-01\"\n---\nnew writing\n",
        )
        .unwrap();
        // Sidecar-dated binary.
        fs::write(content.join("papers/p.pdf"), b"pdf").unwrap();
        fs::write(
            content.join("papers/p.meta.json"),
            r#"{"schema":1,"kind":"document","authored":{"date":"2026-03-10"},"derived":{}}"#,
        )
        .unwrap();
        // Undated entries have `None` sort dates, so they sort first with
        // the path as a tiebreaker.
        fs::write(content.join("misc/b.txt"), b"b").unwrap();
        fs::write(content.join("misc/a.txt"), b"a").unwrap();

        let ledger = generate_content_ledger(&root, Path::new("content")).unwrap();
        let order: Vec<&str> = ledger
            .blocks
            .iter()
            .map(|block| block.entry.path.as_str())
            .collect();

        assert_eq!(
            order,
            vec![
                // Undated first (path-asc tiebreaker), then dated asc.
                "misc/a.txt",
                "misc/b.txt",
                "writing/old.md",
                "papers/p.pdf",
                "writing/new.md",
            ]
        );
        let heights = ledger
            .blocks
            .iter()
            .map(|block| block.height)
            .collect::<Vec<_>>();
        assert_eq!(heights, vec![1, 2, 3, 4, 5]);
        assert_eq!(
            ledger.blocks[0].prev_block_sha256, ledger.genesis_hash,
            "first block points to genesis"
        );
        for pair in ledger.blocks.windows(2) {
            assert_eq!(pair[1].prev_block_sha256, pair[0].block_sha256);
        }
        assert_eq!(
            ledger.chain_head,
            ledger.blocks.last().unwrap().block_sha256
        );
        ledger.validate().unwrap();

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ledger_groups_bundle_support_assets_without_standalone_blocks() {
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
                "variants":[
                  {"id":"en","path":"en.md","label":"English"},
                  {"id":"ko","path":"ko.md","label":"Korean"}
                ]
              },
              "authored":{"title":"Foo","date":"2026-05-15"},
              "derived":{"kind":"bundle"}
            }"#,
        )
        .unwrap();
        fs::write(content.join("writing/foo/en.md"), b"english").unwrap();
        fs::write(content.join("writing/foo/en.meta.json"), b"{\"schema\":1}").unwrap();
        fs::write(content.join("writing/foo/ko.md"), b"korean").unwrap();
        fs::write(content.join("writing/foo/cover.png"), b"png").unwrap();
        fs::write(
            content.join("writing/foo/cover.meta.json"),
            b"{\"schema\":1,\"authored\":{\"title\":\"Cover\"}}",
        )
        .unwrap();

        let ledger = generate_content_ledger(&root, Path::new("content")).unwrap();

        assert_eq!(ledger.blocks.len(), 1);
        let entry = &ledger.blocks[0].entry;
        assert_eq!(entry.path, "writing/foo");
        assert_eq!(entry.route, "/writing/foo");
        let paths = entry
            .content_files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"content/writing/foo/_index.dir.json"));
        assert!(paths.contains(&"content/writing/foo/en.md"));
        assert!(paths.contains(&"content/writing/foo/en.meta.json"));
        assert!(paths.contains(&"content/writing/foo/ko.md"));
        assert!(paths.contains(&"content/writing/foo/cover.png"));
        assert!(paths.contains(&"content/writing/foo/cover.meta.json"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ledger_rejects_bundle_root_route_collision() {
        let root = temp_root("bundle-collision");
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
        fs::write(content.join("writing/foo.md"), b"collision").unwrap();

        let err = generate_content_ledger(&root, Path::new("content")).unwrap_err();
        let bundle_error = err
            .downcast_ref::<BundleValidationError>()
            .expect("bundle route collision error");
        assert!(matches!(
            bundle_error,
            BundleValidationError::RootRouteCollision { file_path, .. }
                if file_path == "writing/foo.md"
        ));

        fs::remove_dir_all(root).unwrap();
    }
}
