use std::collections::BTreeSet;

use crate::engine::attestation::artifact::{CONTENT_HASH, compute_content_sha256};

use super::{
    CONTENT_LEDGER_GENESIS_HASH, CONTENT_LEDGER_SCHEME, ContentLedger, ContentLedgerCategory,
    ContentLedgerEntry, ContentLedgerSortKey, LedgerHashError, compute_block_sha256,
};

pub type LedgerValidationResult<T = ()> = Result<T, LedgerValidationError>;

#[derive(Debug, thiserror::Error)]
pub enum LedgerValidationError {
    #[error("unsupported ledger version {version}")]
    UnsupportedVersion { version: u32 },
    #[error("unsupported ledger scheme {scheme}")]
    UnsupportedScheme { scheme: String },
    #[error("unsupported ledger hash {hash}")]
    UnsupportedHash { hash: String },
    #[error("ledger genesis_hash mismatch")]
    GenesisHashMismatch,
    #[error("ledger block_count does not match blocks")]
    BlockCountMismatch { declared: usize, actual: usize },
    #[error("ledger block height mismatch at index {index}")]
    BlockHeightMismatch {
        index: usize,
        height: u64,
        expected: u64,
    },
    #[error("ledger blocks are not sorted canonically")]
    BlocksNotSorted,
    #[error("prev_block_sha256 mismatch at block {height}")]
    PrevBlockHashMismatch { height: u64 },
    #[error("ledger sort_key path mismatch for {id}")]
    SortKeyPathMismatch { id: String },
    #[error("duplicate ledger id {id}")]
    DuplicateId { id: String },
    #[error("duplicate ledger route {route}")]
    DuplicateRoute { route: String },
    #[error("duplicate ledger path {path}")]
    DuplicatePath { path: String },
    #[error("content hash mismatch for {id}")]
    ContentHashMismatch { id: String },
    #[error("block hash mismatch for {id}")]
    BlockHashMismatch { id: String },
    #[error("ledger chain_head mismatch")]
    ChainHeadMismatch,
    #[error("ledger id mismatch for {path}")]
    IdMismatch { path: String },
    #[error("ledger category mismatch for {path}")]
    CategoryMismatch { path: String },
    #[error("ledger content has no files for {id}")]
    EmptyContent { id: String },
    #[error("empty content file {path}")]
    EmptyContentFile { path: String },
    #[error("content files must be strictly sorted for {id}")]
    ContentFilesNotSorted { id: String },
    #[error("missing primary content file {path}")]
    MissingPrimaryContentFile { path: String },
    #[error("ledger sort_key date is invalid: {date}")]
    InvalidSortKeyDate { date: String },
    #[error("{field} must be normalized 0x-prefixed sha256")]
    InvalidSha256Field { field: &'static str, value: String },
    #[error("ledger route must be absolute: {route}")]
    InvalidRoute { route: String },
    #[error("ledger path must be content-root-relative: {path}")]
    InvalidContentPath { path: String },
    #[error("ledger content file path must be under content/: {path}")]
    InvalidArtifactFilePath { path: String },
    #[error("{label} contains an invalid segment: {path}")]
    InvalidPathSegment { label: &'static str, path: String },
    #[error(transparent)]
    Hash(#[from] LedgerHashError),
}

impl ContentLedger {
    pub fn validate(&self) -> LedgerValidationResult {
        if self.version != 1 {
            return Err(LedgerValidationError::UnsupportedVersion {
                version: self.version,
            });
        }
        if self.scheme != CONTENT_LEDGER_SCHEME {
            return Err(LedgerValidationError::UnsupportedScheme {
                scheme: self.scheme.clone(),
            });
        }
        if self.hash != CONTENT_HASH {
            return Err(LedgerValidationError::UnsupportedHash {
                hash: self.hash.clone(),
            });
        }
        if self.genesis_hash != CONTENT_LEDGER_GENESIS_HASH {
            return Err(LedgerValidationError::GenesisHashMismatch);
        }
        validate_sha256_field("genesis_hash", &self.genesis_hash)?;
        validate_sha256_field("chain_head", &self.chain_head)?;
        if self.block_count != self.blocks.len() {
            return Err(LedgerValidationError::BlockCountMismatch {
                declared: self.block_count,
                actual: self.blocks.len(),
            });
        }

        let mut ids = BTreeSet::new();
        let mut routes = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut previous_sort_key: Option<&ContentLedgerSortKey> = None;
        let mut expected_prev_block_sha256 = self.genesis_hash.clone();

        for (index, block) in self.blocks.iter().enumerate() {
            let expected_height = index as u64 + 1;
            if block.height != expected_height {
                return Err(LedgerValidationError::BlockHeightMismatch {
                    index: index + 1,
                    height: block.height,
                    expected: expected_height,
                });
            }
            validate_sort_key(&block.sort_key)?;
            if let Some(previous_sort_key) = previous_sort_key
                && previous_sort_key > &block.sort_key
            {
                return Err(LedgerValidationError::BlocksNotSorted);
            }
            previous_sort_key = Some(&block.sort_key);

            validate_sha256_field("prev_block_sha256", &block.prev_block_sha256)?;
            validate_sha256_field("block_sha256", &block.block_sha256)?;
            if block.prev_block_sha256 != expected_prev_block_sha256 {
                return Err(LedgerValidationError::PrevBlockHashMismatch {
                    height: block.height,
                });
            }

            let entry = &block.entry;
            if block.sort_key.path != entry.path {
                return Err(LedgerValidationError::SortKeyPathMismatch {
                    id: entry.id.clone(),
                });
            }
            if !ids.insert(entry.id.as_str()) {
                return Err(LedgerValidationError::DuplicateId {
                    id: entry.id.clone(),
                });
            }
            if !routes.insert(entry.route.as_str()) {
                return Err(LedgerValidationError::DuplicateRoute {
                    route: entry.route.clone(),
                });
            }
            if !paths.insert(entry.path.as_str()) {
                return Err(LedgerValidationError::DuplicatePath {
                    path: entry.path.clone(),
                });
            }

            validate_entry(entry)?;
            let content_sha256 =
                compute_content_sha256(&entry.content_files).map_err(LedgerHashError::from)?;
            if content_sha256 != entry.content_sha256 {
                return Err(LedgerValidationError::ContentHashMismatch {
                    id: entry.id.clone(),
                });
            }

            let block_sha256 = compute_block_sha256(block)?;
            if block_sha256 != block.block_sha256 {
                return Err(LedgerValidationError::BlockHashMismatch {
                    id: entry.id.clone(),
                });
            }
            expected_prev_block_sha256 = block.block_sha256.clone();
        }

        let expected_chain_head = self
            .blocks
            .last()
            .map(|block| block.block_sha256.as_str())
            .unwrap_or(&self.genesis_hash);
        if self.chain_head != expected_chain_head {
            return Err(LedgerValidationError::ChainHeadMismatch);
        }

        Ok(())
    }
}

fn validate_entry(entry: &ContentLedgerEntry) -> LedgerValidationResult {
    validate_absolute_route(&entry.route)?;
    validate_content_path(&entry.path)?;
    let expected_id = format!("route:{}", entry.route);
    if entry.id != expected_id {
        return Err(LedgerValidationError::IdMismatch {
            path: entry.path.clone(),
        });
    }
    if entry.category != ContentLedgerCategory::for_path(&entry.path) {
        return Err(LedgerValidationError::CategoryMismatch {
            path: entry.path.clone(),
        });
    }
    validate_entry_content(entry)?;
    validate_sha256_field("content_sha256", &entry.content_sha256)
}

fn validate_entry_content(entry: &ContentLedgerEntry) -> LedgerValidationResult {
    if entry.content_files.is_empty() {
        return Err(LedgerValidationError::EmptyContent {
            id: entry.id.clone(),
        });
    }

    let primary_file = format!("content/{}", entry.path);
    let primary_bundle_sidecar = format!("content/{}/_index.dir.json", entry.path);
    let mut previous_path: Option<&str> = None;
    let mut has_primary_file = false;
    for file in &entry.content_files {
        validate_artifact_file_path(&file.path)?;
        validate_sha256_field("content file sha256", &file.sha256)?;
        if file.bytes == 0 {
            return Err(LedgerValidationError::EmptyContentFile {
                path: file.path.clone(),
            });
        }
        if file.path == primary_file || file.path == primary_bundle_sidecar {
            has_primary_file = true;
        }
        if let Some(previous_path) = previous_path
            && previous_path >= file.path.as_str()
        {
            return Err(LedgerValidationError::ContentFilesNotSorted {
                id: entry.id.clone(),
            });
        }
        previous_path = Some(&file.path);
    }
    if !has_primary_file {
        return Err(LedgerValidationError::MissingPrimaryContentFile { path: primary_file });
    }
    Ok(())
}

fn validate_sort_key(sort_key: &ContentLedgerSortKey) -> LedgerValidationResult {
    if let Some(date) = &sort_key.date {
        validate_sort_key_date(date)?;
    }
    validate_content_path(&sort_key.path)
}

fn validate_sort_key_date(date: &str) -> LedgerValidationResult {
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes[..4].iter().all(|byte| byte.is_ascii_digit())
        || !bytes[5..7].iter().all(|byte| byte.is_ascii_digit())
        || !bytes[8..10].iter().all(|byte| byte.is_ascii_digit())
        || date.chars().any(char::is_control)
    {
        return Err(LedgerValidationError::InvalidSortKeyDate {
            date: date.to_string(),
        });
    }

    let Ok(year) = date[0..4].parse::<u32>() else {
        return Err(LedgerValidationError::InvalidSortKeyDate {
            date: date.to_string(),
        });
    };
    let Ok(month) = date[5..7].parse::<u32>() else {
        return Err(LedgerValidationError::InvalidSortKeyDate {
            date: date.to_string(),
        });
    };
    let Ok(day) = date[8..10].parse::<u32>() else {
        return Err(LedgerValidationError::InvalidSortKeyDate {
            date: date.to_string(),
        });
    };

    let Some(max_day) = days_in_month(year, month) else {
        return Err(LedgerValidationError::InvalidSortKeyDate {
            date: date.to_string(),
        });
    };
    if day == 0 || day > max_day {
        return Err(LedgerValidationError::InvalidSortKeyDate {
            date: date.to_string(),
        });
    }

    Ok(())
}

fn days_in_month(year: u32, month: u32) -> Option<u32> {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    };
    Some(days)
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn validate_sha256_field(field: &'static str, value: &str) -> LedgerValidationResult {
    if value.len() != 66
        || !value.starts_with("0x")
        || !value[2..]
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(LedgerValidationError::InvalidSha256Field {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn validate_absolute_route(route: &str) -> LedgerValidationResult {
    if !route.starts_with('/') || route.contains('\\') || route.chars().any(char::is_control) {
        return Err(LedgerValidationError::InvalidRoute {
            route: route.to_string(),
        });
    }
    if route != "/" {
        validate_path_segments(route.trim_start_matches('/'), "ledger route")?;
    }
    Ok(())
}

fn validate_content_path(path: &str) -> LedgerValidationResult {
    if path.starts_with('/') || path.contains('\\') || path.chars().any(char::is_control) {
        return Err(LedgerValidationError::InvalidContentPath {
            path: path.to_string(),
        });
    }
    validate_path_segments(path, "ledger path")
}

fn validate_artifact_file_path(path: &str) -> LedgerValidationResult {
    if !path.starts_with("content/")
        || path.starts_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
    {
        return Err(LedgerValidationError::InvalidArtifactFilePath {
            path: path.to_string(),
        });
    }
    validate_path_segments(path, "ledger content file path")
}

fn validate_path_segments(path: &str, label: &'static str) -> LedgerValidationResult {
    if path.is_empty()
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(LedgerValidationError::InvalidPathSegment {
            label,
            path: path.to_string(),
        });
    }
    Ok(())
}
