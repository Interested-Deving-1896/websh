use std::collections::BTreeMap;
use std::sync::Arc;

use crate::app::AppContext;
use crate::shared::components::size_summary_parts;
use websh_core::attestation::ledger::{
    CONTENT_LEDGER_CONTENT_PATH, ContentLedger, ContentLedgerBlock, LedgerValidationError,
};
use websh_core::domain::{NodeMetadata, VirtualPath};
use websh_core::filesystem::{ContentReadError, GlobalFs, content_href_for_path};
use websh_core::mempool::LEDGER_CATEGORIES;
use websh_core::support::format::{format_date_iso, format_size, iso_date_prefix};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LedgerModel {
    pub(super) filter: LedgerFilter,
    pub(super) entries: Vec<LedgerEntry>,
    pub(super) counts: BTreeMap<String, usize>,
    pub(super) total_count: usize,
    pub(super) encrypted_count: usize,
    pub(super) head_hash: String,
    pub(super) genesis_date: String,
    pub(super) latest_date: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum LedgerFilter {
    All,
    Category(String),
}

#[derive(Clone, Debug, thiserror::Error)]
pub(super) enum LedgerLoadError {
    #[error("root mount failed: {message}")]
    RootMountFailed { message: String },
    #[error("read {path}: {source}")]
    Read {
        path: VirtualPath,
        #[source]
        source: ContentReadError,
    },
    #[error("parse ledger json: {source}")]
    Parse {
        #[source]
        source: Arc<serde_json::Error>,
    },
    #[error("validate ledger: {source}")]
    Validate {
        #[source]
        source: Arc<LedgerValidationError>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LedgerEntry {
    pub(super) block_number: String,
    pub(super) block_height: u64,
    pub(super) path: String,
    pub(super) href: String,
    pub(super) title: String,
    pub(super) description: Option<String>,
    pub(super) date: String,
    pub(super) category: String,
    pub(super) kind: String,
    pub(super) meta_line: Vec<String>,
    pub(super) variants: Vec<String>,
    pub(super) encrypted: bool,
    pub(super) hash: String,
    pub(super) previous_hash: String,
}

pub(super) fn ledger_filter_for_route(request_path: &str, node_path: &VirtualPath) -> LedgerFilter {
    if request_path.trim_matches('/') == "ledger" {
        return LedgerFilter::All;
    }
    node_path
        .segments()
        .next()
        .map(|segment| LedgerFilter::Category(segment.to_string()))
        .unwrap_or(LedgerFilter::All)
}

pub(super) async fn load_content_ledger(ctx: AppContext) -> Result<ContentLedger, LedgerLoadError> {
    let path = VirtualPath::from_absolute(format!("/{CONTENT_LEDGER_CONTENT_PATH}"))
        .expect("ledger path is absolute");
    let body = ctx
        .read_text(&path)
        .await
        .map_err(|source| LedgerLoadError::Read { path, source })?;
    let ledger: ContentLedger =
        serde_json::from_str(&body).map_err(|source| LedgerLoadError::Parse {
            source: Arc::new(source),
        })?;
    ledger
        .validate()
        .map_err(|source| LedgerLoadError::Validate {
            source: Arc::new(source),
        })?;
    Ok(ledger)
}

pub(super) fn build_ledger_model(
    fs: &GlobalFs,
    ledger: &ContentLedger,
    filter: &LedgerFilter,
) -> LedgerModel {
    let all_entries = ledger
        .blocks
        .iter()
        .rev()
        .filter_map(|block| ledger_entry_for_block(fs, block))
        .collect::<Vec<_>>();
    let total_count = all_entries.len();

    let mut counts = BTreeMap::new();
    for category in LEDGER_CATEGORIES {
        counts.insert((*category).to_string(), 0usize);
    }
    for entry in &all_entries {
        *counts.entry(entry.category.clone()).or_default() += 1;
    }

    let entries = all_entries
        .iter()
        .filter(|entry| filter.includes(entry))
        .cloned()
        .collect::<Vec<_>>();
    let encrypted_count = entries.iter().filter(|entry| entry.encrypted).count();
    let head_hash = ledger.chain_head.clone();
    let latest_date = entries
        .first()
        .map(|entry| entry.date.clone())
        .unwrap_or_else(|| "—".to_string());
    let genesis_date = all_entries
        .iter()
        .filter_map(|entry| iso_date_prefix(&entry.date).map(str::to_string))
        .min()
        .unwrap_or_else(|| "—".to_string());

    LedgerModel {
        filter: filter.clone(),
        entries,
        counts,
        total_count,
        encrypted_count,
        head_hash,
        genesis_date,
        latest_date,
    }
}

fn ledger_entry_for_block(fs: &GlobalFs, block: &ContentLedgerBlock) -> Option<LedgerEntry> {
    let entry = &block.entry;
    let node_path = VirtualPath::from_absolute(format!("/{}", entry.path)).ok()?;
    let node_meta = fs.node_metadata(&node_path);
    let fallback_title = fallback_file_title(&entry.path);
    let title = node_meta
        .and_then(|meta| meta.title())
        .map(str::to_string)
        .unwrap_or(fallback_title);
    let description = node_meta
        .and_then(|meta| meta.description())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    let date = node_meta
        .and_then(|meta| meta.date())
        .map(str::to_string)
        .or_else(|| node_meta.and_then(|meta| meta.modified_at().map(format_date_iso)))
        .unwrap_or_else(|| "undated".to_string());
    let category = entry.category.as_str().to_string();
    let kind = if node_meta.is_some_and(NodeMetadata::is_bundle) {
        "bundle".to_string()
    } else {
        kind_for_entry(&category, &entry.path)
    };
    let tags = node_meta.map(NodeMetadata::tags_owned).unwrap_or_default();
    let size = node_meta
        .and_then(|meta| meta.size_bytes())
        .or_else(|| Some(entry.content_files.iter().map(|file| file.bytes).sum()));
    let summary_parts = node_meta
        .map(|meta| {
            size_summary_parts(
                meta.effective_kind(),
                meta.word_count(),
                meta.page_count(),
                meta.image_dimensions(),
            )
        })
        .unwrap_or_default();
    let encrypted = node_meta.and_then(|meta| meta.access()).is_some();
    let variants = node_meta
        .and_then(|meta| meta.bundle.as_ref())
        .map(|bundle| {
            bundle
                .variants
                .iter()
                .map(|variant| variant.label.clone())
                .collect()
        })
        .unwrap_or_default();

    Some(LedgerEntry {
        block_number: format!("{:04}", block.height),
        block_height: block.height,
        path: entry.path.clone(),
        href: content_href_for_path(&entry.path),
        title,
        description,
        date,
        category,
        kind,
        meta_line: meta_line_for_entry(summary_parts, size, &tags),
        variants,
        encrypted,
        hash: block.block_sha256.clone(),
        previous_hash: block.prev_block_sha256.clone(),
    })
}

fn meta_line_for_entry(
    summary_parts: Vec<String>,
    size: Option<u64>,
    tags: &[String],
) -> Vec<String> {
    let mut out = summary_parts;
    if out.is_empty()
        && let Some(bytes) = size
    {
        out.push(format_size(Some(bytes), false));
    }
    out.extend(tags.iter().take(3).cloned());
    if out.is_empty() {
        out.push("content".to_string());
    }
    out
}

fn fallback_file_title(path: &str) -> String {
    path.rsplit('/')
        .next()
        .and_then(|name| name.split('.').next())
        .filter(|stem| !stem.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn kind_for_entry(category: &str, path: &str) -> String {
    match category {
        "papers" => "paper",
        "projects" => "project",
        "talks" => "talk",
        "writing" => "writing",
        _ if path.ends_with(".asc") => "key",
        _ if path.ends_with(".toml") || path.ends_with(".json") => "data",
        _ => "note",
    }
    .to_string()
}

impl LedgerFilter {
    pub(super) fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    pub(super) fn matches(&self, category: &str) -> bool {
        matches!(self, Self::Category(active) if active == category)
    }

    fn includes(&self, entry: &LedgerEntry) -> bool {
        match self {
            Self::All => true,
            Self::Category(category) if LEDGER_CATEGORIES.contains(&category.as_str()) => {
                entry.category == *category
            }
            Self::Category(category) => entry.path.starts_with(&format!("{category}/")),
        }
    }
}
