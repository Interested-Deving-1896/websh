//! Mempool data model projected from manifest entries for web UI consumers.

use std::collections::BTreeMap;

use websh_core::domain::{MempoolFields, MempoolStatus, NodeMetadata, Priority, VirtualPath};
use websh_core::mempool::{LEDGER_CATEGORIES, category_for_mempool_path};
use websh_core::support::format::{format_size, format_thousands_u32, iso_date_prefix};

const DEFAULT_TITLE_FALLBACK: &str = "untitled";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MempoolModel {
    pub filter: LedgerFilterShape,
    pub entries: Vec<MempoolEntry>,
    pub total_count: usize,
    pub counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MempoolEntry {
    pub path: VirtualPath,
    pub title: String,
    pub desc: String,
    pub status: MempoolStatus,
    pub priority: Option<Priority>,
    pub kind: String,
    pub category: String,
    pub modified: String,
    pub sort_key: Option<String>,
    pub gas: String,
    pub tags: Vec<String>,
}

/// Filter shape mirrored by the ledger page without depending on web UI state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerFilterShape {
    All,
    Category(String),
}

impl LedgerFilterShape {
    pub fn includes(&self, entry: &MempoolEntry) -> bool {
        match self {
            Self::All => true,
            Self::Category(category) if LEDGER_CATEGORIES.contains(&category.as_str()) => {
                entry.category == *category
            }
            Self::Category(category) => entry.path.as_str().contains(&format!("/{category}/")),
        }
    }
}

/// One mempool file projected from its manifest entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedMempoolFile {
    pub path: VirtualPath,
    pub meta: NodeMetadata,
    pub mempool: MempoolFields,
}

pub fn build_mempool_model(
    mempool_root: &VirtualPath,
    files: Vec<LoadedMempoolFile>,
    filter: &LedgerFilterShape,
) -> MempoolModel {
    let mut all = files
        .into_iter()
        .map(|file| build_entry(mempool_root, file))
        .collect::<Vec<_>>();

    let mut counts = BTreeMap::new();
    for entry in &all {
        *counts.entry(entry.category.clone()).or_default() += 1;
    }
    let total_count = all.len();

    sort_entries(&mut all);

    let entries = all
        .iter()
        .filter(|entry| filter.includes(entry))
        .cloned()
        .collect::<Vec<_>>();

    MempoolModel {
        filter: filter.clone(),
        entries,
        total_count,
        counts,
    }
}

fn build_entry(mempool_root: &VirtualPath, file: LoadedMempoolFile) -> MempoolEntry {
    let LoadedMempoolFile {
        path,
        meta,
        mempool,
    } = file;

    let title = meta
        .title()
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_TITLE_FALLBACK)
        .to_string();
    let status = mempool.status;
    let priority = mempool.priority;
    let date = meta.date().map(str::to_string);
    let modified = date.clone().unwrap_or_else(|| "undated".to_string());
    let sort_key = date
        .as_deref()
        .and_then(|raw| iso_date_prefix(raw).map(|prefix| prefix.to_string()));
    let category = mempool
        .category
        .clone()
        .unwrap_or_else(|| category_for_mempool_path(&path, mempool_root));
    let kind = kind_for_category(&category);
    let desc = meta.description().unwrap_or("").to_string();
    let is_markdown = path.as_str().ends_with(".md");
    let gas = if is_markdown {
        meta.word_count()
            .map(|w| format!("~{} words", format_thousands_u32(w)))
            .unwrap_or_default()
    } else {
        meta.size_bytes()
            .map(|n| format_size(Some(n), false))
            .unwrap_or_default()
    };
    let tags = meta.tags_owned();

    MempoolEntry {
        path,
        title,
        desc,
        status,
        priority,
        kind,
        category,
        modified,
        sort_key,
        gas,
        tags,
    }
}

fn kind_for_category(category: &str) -> String {
    match category {
        "writing" => "writing",
        "projects" => "project",
        "papers" => "paper",
        "talks" => "talk",
        _ => "note",
    }
    .to_string()
}

fn sort_entries(entries: &mut [MempoolEntry]) {
    entries.sort_by(|left, right| match (&left.sort_key, &right.sort_key) {
        (Some(left_key), Some(right_key)) => right_key
            .cmp(left_key)
            .then_with(|| left.path.as_str().cmp(right.path.as_str())),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.path.as_str().cmp(right.path.as_str()),
    });
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;
    use websh_core::domain::{Fields, NodeKind, SCHEMA_VERSION};

    wasm_bindgen_test_configure!(run_in_browser);

    fn loaded(
        path: &str,
        title: &str,
        date: Option<&str>,
        status: MempoolStatus,
        priority: Option<Priority>,
    ) -> LoadedMempoolFile {
        LoadedMempoolFile {
            path: VirtualPath::from_absolute(path).unwrap(),
            meta: NodeMetadata {
                schema: SCHEMA_VERSION,
                kind: NodeKind::Page,
                bundle: None,
                authored: Fields {
                    title: Some(title.to_string()),
                    date: date.map(str::to_string),
                    ..Fields::default()
                },
                derived: Fields::default(),
            },
            mempool: MempoolFields {
                status,
                priority,
                category: None,
            },
        }
    }

    #[wasm_bindgen_test]
    fn build_model_orders_by_modified_desc() {
        let mempool_root = VirtualPath::from_absolute("/mempool").unwrap();
        let files = vec![
            loaded(
                "/mempool/writing/old.md",
                "old",
                Some("2026-03-01"),
                MempoolStatus::Draft,
                None,
            ),
            loaded(
                "/mempool/writing/new.md",
                "new",
                Some("2026-04-01"),
                MempoolStatus::Draft,
                None,
            ),
            loaded(
                "/mempool/writing/mid.md",
                "mid",
                Some("2026-03-15"),
                MempoolStatus::Review,
                Some(Priority::Med),
            ),
        ];
        let model = build_mempool_model(&mempool_root, files, &LedgerFilterShape::All);
        assert_eq!(model.entries.len(), 3);
        assert_eq!(model.entries[0].path.as_str(), "/mempool/writing/new.md");
        assert_eq!(model.entries[1].path.as_str(), "/mempool/writing/mid.md");
        assert_eq!(model.entries[2].path.as_str(), "/mempool/writing/old.md");
        assert_eq!(model.total_count, 3);
        assert_eq!(model.counts.get("writing").copied(), Some(3));
    }

    #[wasm_bindgen_test]
    fn build_model_filters_by_category() {
        let mempool_root = VirtualPath::from_absolute("/mempool").unwrap();
        let files = vec![
            loaded(
                "/mempool/writing/a.md",
                "a",
                Some("2026-04-01"),
                MempoolStatus::Draft,
                None,
            ),
            loaded(
                "/mempool/papers/b.md",
                "b",
                Some("2026-04-02"),
                MempoolStatus::Draft,
                None,
            ),
        ];
        let model = build_mempool_model(
            &mempool_root,
            files,
            &LedgerFilterShape::Category("writing".to_string()),
        );
        assert_eq!(model.entries.len(), 1);
        assert_eq!(model.entries[0].category, "writing");
        assert_eq!(model.total_count, 2);
        assert_eq!(model.counts.get("writing").copied(), Some(1));
        assert_eq!(model.counts.get("papers").copied(), Some(1));
    }

    #[wasm_bindgen_test]
    fn build_model_treats_undated_as_lowest_priority_sort() {
        let mempool_root = VirtualPath::from_absolute("/mempool").unwrap();
        let files = vec![
            loaded(
                "/mempool/writing/dated.md",
                "dated",
                Some("2026-04-01"),
                MempoolStatus::Draft,
                None,
            ),
            loaded(
                "/mempool/writing/undated.md",
                "undated",
                None,
                MempoolStatus::Draft,
                None,
            ),
        ];
        let model = build_mempool_model(&mempool_root, files, &LedgerFilterShape::All);
        assert_eq!(model.entries.len(), 2);
        assert_eq!(model.entries[0].path.as_str(), "/mempool/writing/dated.md");
        assert_eq!(
            model.entries[1].path.as_str(),
            "/mempool/writing/undated.md"
        );
    }

    #[wasm_bindgen_test]
    fn build_model_renders_mixed_categories() {
        let mempool_root = VirtualPath::from_absolute("/mempool").unwrap();
        let files = vec![
            loaded(
                "/mempool/writing/foo.md",
                "foo",
                Some("2026-04-01"),
                MempoolStatus::Draft,
                None,
            ),
            loaded(
                "/mempool/papers/bar.md",
                "bar",
                Some("2026-04-02"),
                MempoolStatus::Review,
                Some(Priority::High),
            ),
            loaded(
                "/mempool/talks/baz.md",
                "baz",
                Some("2026-03-10"),
                MempoolStatus::Draft,
                None,
            ),
        ];

        let model = build_mempool_model(&mempool_root, files, &LedgerFilterShape::All);
        assert_eq!(model.total_count, 3);
        assert_eq!(model.entries.len(), 3);
        assert_eq!(model.entries[0].path.as_str(), "/mempool/papers/bar.md");
        assert_eq!(model.entries[0].priority, Some(Priority::High));
        assert_eq!(model.entries[1].path.as_str(), "/mempool/writing/foo.md");
        assert_eq!(model.entries[2].path.as_str(), "/mempool/talks/baz.md");

        let writing_only = build_mempool_model(
            &mempool_root,
            vec![
                loaded(
                    "/mempool/writing/foo.md",
                    "foo",
                    Some("2026-04-01"),
                    MempoolStatus::Draft,
                    None,
                ),
                loaded(
                    "/mempool/papers/bar.md",
                    "bar",
                    Some("2026-04-02"),
                    MempoolStatus::Review,
                    None,
                ),
            ],
            &LedgerFilterShape::Category("writing".to_string()),
        );
        assert_eq!(writing_only.entries.len(), 1);
        assert_eq!(writing_only.total_count, 2);
    }
}
