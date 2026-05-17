//! Single source of truth for the manifest shape of a mempool entry.
//!
//! Every write path (browser New/Edit, CLI `mempool add`) routes through
//! `build_mempool_manifest_state` so the resulting `(NodeMetadata,
//! EntryExtensions)` is identical given identical inputs. `derived` fields
//! (size, sha256, word_count) are computed from the raw bytes that go on
//! the wire — sha and size against `raw_body` directly, word_count
//! against the post-frontmatter body.
//!
//! `derived.modified_at` is deliberately omitted for byte-stability under
//! signed attestations (mirrors `cli::manifest::sync_content`).

use std::str::FromStr;

use sha2::{Digest, Sha256};

use crate::domain::{
    EntryExtensions, Fields, MempoolFields, MempoolStatus, NodeKind, NodeMetadata, Priority,
    SCHEMA_VERSION, VirtualPath,
};

use super::parse::{category_for_mempool_path, parse_mempool_frontmatter, strip_frontmatter_block};
use super::path::mempool_root;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MempoolManifestState {
    pub meta: NodeMetadata,
    pub extensions: EntryExtensions,
}

/// Build the manifest state for a mempool file. `raw_body` is the markdown
/// bytes we will commit (frontmatter included). `path` is the canonical
/// `/mempool/<cat>/<slug>.md` absolute path.
///
/// Status / priority / category come from the frontmatter (fall back:
/// status=Draft, priority=None, category derived from path). Authored
/// fields (title / date / tags) likewise come from frontmatter.
pub fn build_mempool_manifest_state(raw_body: &str, path: &VirtualPath) -> MempoolManifestState {
    let raw = parse_mempool_frontmatter(raw_body).unwrap_or_default();
    let category_from_path = category_for_mempool_path(path, mempool_root());
    let status = raw
        .status
        .as_deref()
        .and_then(|s| MempoolStatus::from_str(s).ok())
        .unwrap_or(MempoolStatus::Draft);
    let priority = raw
        .priority
        .as_deref()
        .and_then(|s| Priority::from_str(s).ok());
    let sha = format!("0x{}", hex::encode(Sha256::digest(raw_body.as_bytes())));
    let word_count = u32::try_from(content_word_count(raw_body)).unwrap_or(u32::MAX);

    let category = raw
        .category
        .filter(|s| !s.is_empty())
        .unwrap_or(category_from_path);

    MempoolManifestState {
        meta: NodeMetadata {
            schema: SCHEMA_VERSION,
            kind: NodeKind::Page,
            bundle: None,
            authored: Fields {
                title: raw.title.filter(|s| !s.is_empty()),
                date: raw.modified.filter(|s| !s.is_empty()),
                tags: (!raw.tags.is_empty()).then_some(raw.tags),
                ..Fields::default()
            },
            derived: Fields {
                size_bytes: Some(raw_body.len() as u64),
                content_sha256: Some(sha),
                word_count: Some(word_count),
                ..Fields::default()
            },
        },
        extensions: EntryExtensions {
            mempool: Some(MempoolFields {
                status,
                priority,
                category: Some(category).filter(|s| !s.is_empty() && s != "misc"),
            }),
        },
    }
}

fn content_word_count(raw_body: &str) -> usize {
    strip_frontmatter_block(raw_body).split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> VirtualPath {
        VirtualPath::from_absolute(s).unwrap()
    }

    #[test]
    fn extracts_authored_from_frontmatter() {
        let body = "---\n\
                    title: \"On writing slow\"\n\
                    status: review\n\
                    priority: med\n\
                    modified: \"2026-04-25\"\n\
                    tags: [essay, slow]\n\
                    ---\n\nHello world.\n";
        let state = build_mempool_manifest_state(body, &path("/mempool/writing/on-slow.md"));
        assert_eq!(
            state.meta.authored.title.as_deref(),
            Some("On writing slow")
        );
        assert_eq!(state.meta.authored.date.as_deref(), Some("2026-04-25"));
        assert_eq!(
            state.meta.authored.tags.as_deref(),
            Some(&["essay".to_string(), "slow".to_string()][..])
        );
    }

    #[test]
    fn computes_derived_from_raw_bytes() {
        let body = "---\ntitle: x\nstatus: draft\n---\n\nHello world hello.\n";
        let state = build_mempool_manifest_state(body, &path("/mempool/writing/x.md"));
        assert_eq!(state.meta.derived.size_bytes, Some(body.len() as u64));
        assert_eq!(state.meta.derived.word_count, Some(3));
        let sha = state.meta.derived.content_sha256.as_deref().unwrap();
        assert!(sha.starts_with("0x"));
        assert_eq!(sha.len(), 2 + 64);
    }

    #[test]
    fn includes_mempool_block() {
        let body = "---\ntitle: x\nstatus: review\npriority: high\n---\n";
        let state = build_mempool_manifest_state(body, &path("/mempool/papers/x.md"));
        let mp = state.extensions.mempool.expect("mempool block");
        assert_eq!(mp.status, MempoolStatus::Review);
        assert_eq!(mp.priority, Some(Priority::High));
        assert_eq!(mp.category.as_deref(), Some("papers"));
    }

    #[test]
    fn falls_back_to_draft_when_status_missing() {
        let body = "---\ntitle: x\n---\n";
        let state = build_mempool_manifest_state(body, &path("/mempool/writing/x.md"));
        let mp = state.extensions.mempool.expect("mempool block");
        assert_eq!(mp.status, MempoolStatus::Draft);
        assert!(mp.priority.is_none());
    }

    #[test]
    fn drops_misc_category_when_loose_path() {
        let body = "---\ntitle: x\nstatus: draft\n---\n";
        let state = build_mempool_manifest_state(body, &path("/mempool/loose.md"));
        let mp = state.extensions.mempool.expect("mempool block");
        assert!(mp.category.is_none(), "got {:?}", mp.category);
    }

    #[test]
    fn category_from_frontmatter_overrides_path_when_present() {
        let body = "---\ntitle: x\nstatus: draft\ncategory: papers\n---\n";
        let state = build_mempool_manifest_state(body, &path("/mempool/writing/x.md"));
        let mp = state.extensions.mempool.expect("mempool block");
        assert_eq!(mp.category.as_deref(), Some("papers"));
    }

    #[test]
    fn word_count_excludes_frontmatter() {
        let body =
            "---\ntitle: \"big title with many words\"\ntags: [a, b, c]\n---\n\nbody one two\n";
        let state = build_mempool_manifest_state(body, &path("/mempool/writing/x.md"));
        assert_eq!(state.meta.derived.word_count, Some(3));
    }

    #[test]
    fn empty_authored_tags_omitted() {
        let body = "---\ntitle: x\nstatus: draft\n---\n";
        let state = build_mempool_manifest_state(body, &path("/mempool/writing/x.md"));
        assert!(state.meta.authored.tags.is_none());
    }
}
