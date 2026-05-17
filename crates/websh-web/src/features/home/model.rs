use serde::Deserialize;

use websh_core::attestation::artifact::AttestationArtifact;
use websh_core::domain::VirtualPath;
use websh_core::filesystem::{GlobalFs, content_href_for_path};

pub(super) const TOC_ITEMS: &[TocItem] = &[
    TocItem {
        num: "1",
        name: "about",
        href: "#/about",
        meta: "bio · cv",
        count_root: None,
    },
    TocItem {
        num: "2",
        name: "writing",
        href: "#/writing",
        meta: "",
        count_root: Some("/writing"),
    },
    TocItem {
        num: "3",
        name: "projects",
        href: "#/projects",
        meta: "",
        count_root: Some("/projects"),
    },
    TocItem {
        num: "4",
        name: "papers",
        href: "#/papers",
        meta: "",
        count_root: Some("/papers"),
    },
    TocItem {
        num: "5",
        name: "talks",
        href: "#/talks",
        meta: "",
        count_root: Some("/talks"),
    },
    TocItem {
        num: "6",
        name: "misc",
        href: "#/misc",
        meta: "",
        count_root: Some("/misc"),
    },
];

#[derive(Clone, Copy)]
pub(super) struct TocItem {
    pub(super) num: &'static str,
    pub(super) name: &'static str,
    pub(super) href: &'static str,
    pub(super) meta: &'static str,
    count_root: Option<&'static str>,
}

impl TocItem {
    pub(super) fn is_count_backed(&self) -> bool {
        self.count_root.is_some()
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct NowDocument {
    pub(super) items: Vec<NowItem>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct NowItem {
    date: String,
    pub(super) text: String,
}

#[derive(Clone, Debug, thiserror::Error)]
pub(super) enum NowParseError {
    #[error("parse now.toml: {message}")]
    Toml { message: String },
    #[error("now.toml must contain at least one item")]
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RecentItem {
    pub(super) kind: String,
    pub(super) date: String,
    pub(super) title: String,
    pub(super) href: String,
    pub(super) tag: String,
}

pub(super) fn site_last_revised_at() -> Option<String> {
    websh_site::attestation_artifact()
        .ok()
        .and_then(|artifact| latest_attestation_issued_at(&artifact))
}

fn latest_attestation_issued_at(artifact: &AttestationArtifact) -> Option<String> {
    artifact
        .subjects
        .iter()
        .map(|subject| subject.issued_at())
        .max()
        .map(str::to_string)
}

pub(super) fn current_homepage_date() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let date = js_sys::Date::new_0();
        format!(
            "{:04}-{:02}-{:02}",
            date.get_full_year(),
            date.get_month() + 1,
            date.get_date()
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        websh_core::support::format::format_date_iso(crate::platform::current_timestamp() / 1000)
    }
}

pub(super) fn compact_homepage_date(date: &str) -> String {
    let mut parts = date.split('-');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(year), Some(month), Some(day), None)
            if year.len() == 4
                && month.len() == 2
                && day.len() == 2
                && year.chars().all(|ch| ch.is_ascii_digit())
                && month.chars().all(|ch| ch.is_ascii_digit())
                && day.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            format!("{year}/{month}{day}")
        }
        _ => date.to_string(),
    }
}

pub(super) fn parse_now_toml(body: &str) -> Result<NowDocument, NowParseError> {
    let mut doc: NowDocument = toml::from_str(body).map_err(|error| NowParseError::Toml {
        message: error.to_string(),
    })?;

    doc.items = doc
        .items
        .into_iter()
        .map(|mut item| {
            item.date = item.date.trim().to_string();
            item.text = item.text.trim().to_string();
            item
        })
        .filter(|item| !item.date.is_empty() && !item.text.is_empty())
        .collect();

    if doc.items.is_empty() {
        return Err(NowParseError::Empty);
    }

    Ok(doc)
}

pub(super) fn latest_now_date(items: &[NowItem]) -> Option<String> {
    items
        .iter()
        .map(|item| item.date.as_str())
        .max()
        .map(str::to_string)
}

pub(super) fn toc_item_meta(fs: &GlobalFs, item: &TocItem) -> String {
    let Some(root) = item.count_root else {
        return item.meta.to_string();
    };
    let Ok(path) = VirtualPath::from_absolute(root) else {
        return "0".to_string();
    };
    count_toc_entries(fs, &path).to_string()
}

fn count_toc_entries(fs: &GlobalFs, root: &VirtualPath) -> usize {
    let Some(entries) = fs.list_dir(root) else {
        return 0;
    };

    entries
        .into_iter()
        .map(|entry| {
            if entry.name.starts_with('.') || entry.name.starts_with('_') {
                return 0;
            }
            if entry.is_dir {
                if fs
                    .node_metadata(&entry.path)
                    .is_some_and(|meta| meta.is_bundle())
                {
                    1
                } else {
                    count_toc_entries(fs, &entry.path)
                }
            } else if toc_countable_file(&entry.path) {
                1
            } else {
                0
            }
        })
        .sum()
}

fn toc_countable_file(path: &VirtualPath) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    if name.ends_with(".meta.json") || name == "manifest.json" || name.starts_with('_') {
        return false;
    }
    matches!(
        name.rsplit_once('.').map(|(_, ext)| ext),
        Some("md" | "html" | "pdf" | "link" | "app")
    )
}

pub(super) fn recent_items_from_fs(fs: &GlobalFs) -> Vec<RecentItem> {
    let mut items = Vec::new();

    for root in ["papers", "projects", "writing", "talks"] {
        let path = VirtualPath::from_absolute(format!("/{root}")).expect("constant category path");
        collect_recent_items(fs, &path, &mut items);
    }

    items.sort_by(|left, right| {
        right
            .date
            .cmp(&left.date)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.title.cmp(&right.title))
    });
    items.truncate(6);
    items
}

fn collect_recent_items(fs: &GlobalFs, path: &VirtualPath, out: &mut Vec<RecentItem>) {
    let Some(entries) = fs.list_dir(path) else {
        return;
    };

    for entry in entries {
        if entry.is_dir {
            if fs
                .node_metadata(&entry.path)
                .is_some_and(|meta| meta.is_bundle())
            {
                collect_recent_bundle(fs, &entry.path, out);
            } else {
                collect_recent_items(fs, &entry.path, out);
            }
            continue;
        }

        let node_meta = fs.node_metadata(&entry.path);
        let Some(date) = non_empty_text(node_meta.and_then(|meta| meta.date()).map(str::to_string))
        else {
            continue;
        };
        let Some(kind) = category_label_for_path(entry.path.as_str()) else {
            continue;
        };
        let title = non_empty_text(node_meta.and_then(|meta| meta.title()).map(str::to_string))
            .unwrap_or(entry.title);
        let tag = node_meta
            .and_then(|meta| meta.tags())
            .and_then(first_tag)
            .unwrap_or_default();

        out.push(RecentItem {
            kind,
            date,
            title,
            href: content_href_for_path(entry.path.as_str()),
            tag,
        });
    }
}

fn collect_recent_bundle(fs: &GlobalFs, path: &VirtualPath, out: &mut Vec<RecentItem>) {
    let node_meta = fs.node_metadata(path);
    let Some(date) = non_empty_text(node_meta.and_then(|meta| meta.date()).map(str::to_string))
    else {
        return;
    };
    let Some(kind) = category_label_for_path(path.as_str()) else {
        return;
    };
    let fallback_title = path
        .file_name()
        .map(str::to_string)
        .unwrap_or_else(|| path.as_str().trim_matches('/').to_string());
    let title = non_empty_text(node_meta.and_then(|meta| meta.title()).map(str::to_string))
        .unwrap_or(fallback_title);
    let tag = node_meta
        .and_then(|meta| meta.tags())
        .and_then(first_tag)
        .unwrap_or_default();

    out.push(RecentItem {
        kind,
        date,
        title,
        href: content_href_for_path(path.as_str()),
        tag,
    });
}

fn non_empty_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn first_tag(tags: &[String]) -> Option<String> {
    tags.iter()
        .map(|tag| tag.trim().to_string())
        .find(|tag| !tag.is_empty())
}

fn category_label_for_path(path: &str) -> Option<String> {
    let folder = path.trim_start_matches('/').split('/').next()?;
    let label = match folder {
        "papers" => "paper",
        "projects" => "project",
        "talks" => "talk",
        "writing" => "writing",
        _ => return None,
    };
    Some(label.to_string())
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn parse_now_toml_trims_and_filters_items() {
        let doc = parse_now_toml(
            r#"
[[items]]
date = " 2026-04-25 "
text = " content-backed now section "

[[items]]
date = "2026-04-26"
text = "newer content-backed item"

[[items]]
date = ""
text = "also ignored"
"#,
        )
        .expect("valid now.toml");

        assert_eq!(doc.items.len(), 2);
        assert_eq!(doc.items[0].date, "2026-04-25");
        assert_eq!(doc.items[0].text, "content-backed now section");
        assert_eq!(latest_now_date(&doc.items).as_deref(), Some("2026-04-26"));
    }

    #[wasm_bindgen_test]
    fn parse_now_toml_rejects_empty_items() {
        assert!(parse_now_toml("[[items]]\ndate = \"\"\ntext = \"\"").is_err());
    }

    #[wasm_bindgen_test]
    fn compact_homepage_date_formats_iso_date() {
        assert_eq!(compact_homepage_date("2026-04-26"), "2026/0426");
        assert_eq!(compact_homepage_date("not-a-date"), "not-a-date");
    }

    #[wasm_bindgen_test]
    fn site_last_revised_at_uses_latest_attestation_subject() {
        let artifact: AttestationArtifact = serde_json::from_str(
            r#"
{
  "version": 1,
  "scheme": "websh.attestations.v1",
  "subjects": [
    {
      "kind": "homepage",
      "route": "/",
      "issued_at": "2026-04-30",
      "content_files": [],
      "attestations": [],
      "ack_combined_root": "0xack"
    },
    {
      "kind": "page",
      "route": "/writing/newer",
      "issued_at": "2026-05-17",
      "content_files": [],
      "attestations": []
    }
  ]
}
"#,
        )
        .expect("valid artifact");

        assert_eq!(
            latest_attestation_issued_at(&artifact).as_deref(),
            Some("2026-05-17")
        );
        assert!(site_last_revised_at().is_some());
    }

    #[wasm_bindgen_test]
    fn recent_items_use_folder_category_metadata_and_content_route() {
        use websh_core::domain::{EntryExtensions, Fields, NodeKind, NodeMetadata, SCHEMA_VERSION};
        use websh_core::ports::{ScannedFile, ScannedSubtree};

        let make_meta = |date: &str, tags: &[&str]| NodeMetadata {
            schema: SCHEMA_VERSION,
            kind: NodeKind::Page,
            bundle: None,
            authored: Fields {
                date: Some(date.to_string()),
                tags: Some(tags.iter().map(|t| t.to_string()).collect()),
                ..Fields::default()
            },
            derived: Fields::default(),
        };

        let snapshot = ScannedSubtree {
            files: vec![
                ScannedFile {
                    path: "projects/websh.md".to_string(),
                    meta: make_meta("2026-04-22", &["local app", "rust"]),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "papers/tabula.md".to_string(),
                    meta: make_meta("2026-04-26", &["EuroSys 2027", "systems"]),
                    extensions: EntryExtensions::default(),
                },
            ],
            directories: Vec::new(),
        };
        let mut fs = GlobalFs::empty();
        fs.mount_scanned_subtree(VirtualPath::root(), &snapshot)
            .expect("mount snapshot");

        let items = recent_items_from_fs(&fs);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, "paper");
        assert_eq!(items[0].href, "#/papers/tabula");
        assert_eq!(items[0].tag, "EuroSys 2027");
        assert_eq!(items[1].kind, "project");
    }

    #[wasm_bindgen_test]
    fn toc_counts_visible_content_files_under_each_directory() {
        use websh_core::domain::{EntryExtensions, Fields, NodeKind, NodeMetadata, SCHEMA_VERSION};
        use websh_core::ports::{ScannedFile, ScannedSubtree};

        let blank = || NodeMetadata {
            schema: SCHEMA_VERSION,
            kind: NodeKind::Page,
            bundle: None,
            authored: Fields::default(),
            derived: Fields::default(),
        };

        let snapshot = ScannedSubtree {
            files: vec![
                ScannedFile {
                    path: "writing/hello.md".to_string(),
                    meta: blank(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "writing/deep/post.html".to_string(),
                    meta: blank(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "writing/hello.meta.json".to_string(),
                    meta: blank(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "writing/notes.toml".to_string(),
                    meta: blank(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "projects/websh.md".to_string(),
                    meta: blank(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "papers/tabula.pdf".to_string(),
                    meta: blank(),
                    extensions: EntryExtensions::default(),
                },
            ],
            directories: Vec::new(),
        };
        let mut fs = GlobalFs::empty();
        fs.mount_scanned_subtree(VirtualPath::root(), &snapshot)
            .expect("mount snapshot");

        assert_eq!(
            count_toc_entries(&fs, &VirtualPath::from_absolute("/writing").unwrap()),
            2
        );
        assert_eq!(
            toc_item_meta(
                &fs,
                TOC_ITEMS
                    .iter()
                    .find(|item| item.name == "projects")
                    .unwrap()
            ),
            "1"
        );
        assert_eq!(
            toc_item_meta(
                &fs,
                TOC_ITEMS.iter().find(|item| item.name == "about").unwrap()
            ),
            "bio · cv"
        );
    }
}
