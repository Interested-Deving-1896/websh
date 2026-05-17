//! Content manifest schema. The manifest is the bundled projection of
//! every sidecar in a mount's content tree. The runtime fetches it once
//! and reads metadata directly — no per-file sidecar fetches at runtime.

use serde::{Deserialize, Serialize};

use super::mempool::MempoolFields;
use super::metadata::NodeMetadata;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentManifestDocument {
    pub entries: Vec<ContentManifestEntry>,
}

/// `mempool` is a domain-extension sibling block; new domains slot in
/// here. No `deny_unknown_fields` so older runtimes ignore newer
/// domain blocks instead of rejecting the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ContentManifestEntry {
    pub path: String,
    pub metadata: NodeMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mempool: Option<MempoolFields>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_manifest_document_round_trips_existing_shape() {
        let body = include_str!("../../../../tests/fixtures/manifest_golden.json");
        let manifest: ContentManifestDocument = serde_json::from_str(body).expect("parse");
        let encoded = serde_json::to_string_pretty(&manifest).expect("serialize");
        assert_eq!(encoded.trim_end(), body.trim_end());
    }

    #[test]
    fn content_manifest_requires_entries() {
        let parsed = serde_json::from_str::<ContentManifestDocument>("{}");
        assert!(parsed.is_err());
    }
}
