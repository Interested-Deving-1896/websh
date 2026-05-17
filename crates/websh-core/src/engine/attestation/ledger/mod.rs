//! Canonical generated content ledger hash chain.

use serde::{Deserialize, Serialize};

use crate::engine::attestation::artifact::{ContentFile, compute_content_sha256};
use crate::engine::attestation::subject::SubjectCanonicalError;

mod build;
#[cfg(test)]
mod tests;
mod validate;

pub use build::compute_block_sha256;
pub use validate::{LedgerValidationError, LedgerValidationResult};

pub const CONTENT_LEDGER_SCHEME: &str = "websh.content-ledger.v1";
pub const CONTENT_LEDGER_PATH: &str = "content/.websh/ledger.json";
pub const CONTENT_LEDGER_CONTENT_PATH: &str = ".websh/ledger.json";
pub const CONTENT_LEDGER_ROUTE: &str = "/ledger";
pub const CONTENT_LEDGER_GENESIS_HASH: &str =
    "0x0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentLedger {
    pub version: u32,
    pub scheme: String,
    pub hash: String,
    pub genesis_hash: String,
    pub blocks: Vec<ContentLedgerBlock>,
    pub block_count: usize,
    pub chain_head: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentLedgerBlock {
    pub height: u64,
    pub sort_key: ContentLedgerSortKey,
    pub prev_block_sha256: String,
    pub block_sha256: String,
    pub entry: ContentLedgerEntry,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentLedgerEntry {
    pub id: String,
    pub route: String,
    pub path: String,
    pub category: ContentLedgerCategory,
    pub content_files: Vec<ContentFile>,
    pub content_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentLedgerSortKey {
    pub date: Option<String>,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentLedgerCategory {
    Writing,
    Projects,
    Papers,
    Talks,
    Misc,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentLedgerInput {
    pub sort_key: ContentLedgerSortKey,
    pub entry: ContentLedgerEntry,
}

#[derive(Serialize)]
struct ContentLedgerBlockForHash<'a> {
    height: u64,
    sort_key: &'a ContentLedgerSortKey,
    prev_block_sha256: &'a str,
    entry: &'a ContentLedgerEntry,
}

impl ContentLedgerBlock {
    pub fn refresh_hash(&mut self) -> Result<(), LedgerHashError> {
        self.block_sha256 = compute_block_sha256(self)?;
        Ok(())
    }
}

impl ContentLedgerEntry {
    pub fn new(
        id: String,
        route: String,
        path: String,
        category: ContentLedgerCategory,
        content_files: Vec<ContentFile>,
    ) -> Result<Self, LedgerHashError> {
        let content_sha256 = compute_content_sha256(&content_files)?;
        Ok(Self {
            id,
            route,
            path,
            category,
            content_files,
            content_sha256,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LedgerHashError {
    #[error(transparent)]
    Content(#[from] SubjectCanonicalError),
    #[error("serialize ledger block: {source}")]
    Block {
        #[source]
        source: serde_json::Error,
    },
}

impl ContentLedgerSortKey {
    pub fn new(date: Option<String>, path: String) -> Self {
        Self { date, path }
    }
}

impl ContentLedgerInput {
    pub fn new(sort_key: ContentLedgerSortKey, entry: ContentLedgerEntry) -> Self {
        Self { sort_key, entry }
    }
}

impl ContentLedgerCategory {
    pub fn for_path(path: &str) -> Self {
        match path.trim_start_matches('/').split('/').next().unwrap_or("") {
            "writing" => Self::Writing,
            "projects" => Self::Projects,
            "papers" => Self::Papers,
            "talks" => Self::Talks,
            _ => Self::Misc,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Writing => "writing",
            Self::Projects => "projects",
            Self::Papers => "papers",
            Self::Talks => "talks",
            Self::Misc => "misc",
        }
    }
}
