//! ChangeSet — unified tracker for in-progress filesystem edits.
//!
//! ChangeSet paths are canonical absolute paths in the global filesystem.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::EntryExtensions;
use crate::domain::{NodeMetadata, VirtualPath};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    CreateFile {
        content: String,
        meta: NodeMetadata,
        extensions: EntryExtensions,
    },
    CreateBinary {
        blob_id: String,
        mime: String,
        meta: NodeMetadata,
        extensions: EntryExtensions,
    },
    UpdateFile {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        meta: Option<NodeMetadata>,
        #[serde(skip_serializing_if = "Option::is_none")]
        extensions: Option<EntryExtensions>,
    },
    DeleteFile,
    CreateDirectory {
        meta: NodeMetadata,
    },
    DeleteDirectory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub change: ChangeType,
    pub staged: bool,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    entries: BTreeMap<VirtualPath, Entry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub creates_staged: usize,
    pub creates_unstaged: usize,
    pub updates_staged: usize,
    pub updates_unstaged: usize,
    pub deletes_staged: usize,
    pub deletes_unstaged: usize,
}

impl Summary {
    pub fn total(&self) -> usize {
        self.creates_staged
            + self.creates_unstaged
            + self.updates_staged
            + self.updates_unstaged
            + self.deletes_staged
            + self.deletes_unstaged
    }
    pub fn total_staged(&self) -> usize {
        self.creates_staged + self.updates_staged + self.deletes_staged
    }
}

impl ChangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert-or-replace a change at `path`. New entries default to staged so
    /// write commands are immediately eligible for `sync commit`.
    pub fn upsert_at(&mut self, path: VirtualPath, change: ChangeType, timestamp_ms: u64) {
        let entry = Entry {
            change,
            staged: true,
            timestamp: timestamp_ms,
        };
        self.entries.insert(path, entry);
    }

    pub fn stage(&mut self, path: &VirtualPath) {
        if let Some(e) = self.entries.get_mut(path) {
            e.staged = true;
        }
    }

    pub fn unstage(&mut self, path: &VirtualPath) {
        if let Some(e) = self.entries.get_mut(path) {
            e.staged = false;
        }
    }

    pub fn discard(&mut self, path: &VirtualPath) {
        self.entries.remove(path);
    }

    pub fn stage_all(&mut self) {
        for e in self.entries.values_mut() {
            e.staged = true;
        }
    }

    pub fn unstage_all(&mut self) {
        for e in self.entries.values_mut() {
            e.staged = false;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn get(&self, path: &VirtualPath) -> Option<&Entry> {
        self.entries.get(path)
    }

    pub fn is_staged(&self, path: &VirtualPath) -> bool {
        self.entries.get(path).is_some_and(|e| e.staged)
    }

    pub fn is_deleted(&self, path: &VirtualPath) -> bool {
        matches!(
            self.entries.get(path).map(|e| &e.change),
            Some(ChangeType::DeleteFile | ChangeType::DeleteDirectory)
        )
    }

    pub fn iter_all(&self) -> impl Iterator<Item = (&VirtualPath, &Entry)> {
        self.entries.iter()
    }

    pub fn iter_staged(&self) -> impl Iterator<Item = (&VirtualPath, &Entry)> {
        self.entries.iter().filter(|(_, e)| e.staged)
    }

    pub fn staged_subset(&self) -> Self {
        Self {
            entries: self
                .entries
                .iter()
                .filter(|(_, entry)| entry.staged)
                .map(|(path, entry)| (path.clone(), entry.clone()))
                .collect(),
        }
    }

    pub fn iter_unstaged(&self) -> impl Iterator<Item = (&VirtualPath, &Entry)> {
        self.entries.iter().filter(|(_, e)| !e.staged)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn summary(&self) -> Summary {
        let mut s = Summary::default();
        for (_, e) in self.iter_all() {
            let bucket = match &e.change {
                ChangeType::CreateFile { .. }
                | ChangeType::CreateBinary { .. }
                | ChangeType::CreateDirectory { .. } => {
                    if e.staged {
                        &mut s.creates_staged
                    } else {
                        &mut s.creates_unstaged
                    }
                }
                ChangeType::UpdateFile { .. } => {
                    if e.staged {
                        &mut s.updates_staged
                    } else {
                        &mut s.updates_unstaged
                    }
                }
                ChangeType::DeleteFile | ChangeType::DeleteDirectory => {
                    if e.staged {
                        &mut s.deletes_staged
                    } else {
                        &mut s.deletes_unstaged
                    }
                }
            };
            *bucket += 1;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> VirtualPath {
        VirtualPath::from_absolute(s).unwrap()
    }

    fn create_file(content: &str) -> ChangeType {
        use crate::domain::{Fields, NodeKind, SCHEMA_VERSION};
        ChangeType::CreateFile {
            content: content.to_string(),
            meta: NodeMetadata {
                schema: SCHEMA_VERSION,
                kind: NodeKind::Page,
                bundle: None,
                authored: Fields::default(),
                derived: Fields::default(),
            },
            extensions: EntryExtensions::default(),
        }
    }

    fn upsert(cs: &mut ChangeSet, path: &str, change: ChangeType) {
        cs.upsert_at(p(path), change, 1234);
    }

    #[test]
    fn upsert_defaults_staged_true() {
        let mut cs = ChangeSet::new();
        upsert(&mut cs, "/a.md", create_file("hi"));
        assert!(cs.is_staged(&p("/a.md")));
        assert_eq!(cs.len(), 1);
        assert_eq!(cs.get(&p("/a.md")).unwrap().timestamp, 1234);
    }

    #[test]
    fn unstage_then_stage_roundtrip() {
        let mut cs = ChangeSet::new();
        upsert(&mut cs, "/a.md", create_file("hi"));
        cs.unstage(&p("/a.md"));
        assert!(!cs.is_staged(&p("/a.md")));
        cs.stage(&p("/a.md"));
        assert!(cs.is_staged(&p("/a.md")));
    }

    #[test]
    fn discard_removes_entry() {
        let mut cs = ChangeSet::new();
        upsert(&mut cs, "/a.md", create_file("hi"));
        cs.discard(&p("/a.md"));
        assert!(cs.get(&p("/a.md")).is_none());
    }

    #[test]
    fn is_deleted_matches_delete_variants() {
        let mut cs = ChangeSet::new();
        upsert(&mut cs, "/gone.md", ChangeType::DeleteFile);
        upsert(&mut cs, "/keep.md", create_file("x"));
        assert!(cs.is_deleted(&p("/gone.md")));
        assert!(!cs.is_deleted(&p("/keep.md")));
    }

    #[test]
    fn iter_all_yields_sorted_order() {
        let mut cs = ChangeSet::new();
        upsert(&mut cs, "/z.md", create_file("z"));
        upsert(&mut cs, "/a.md", create_file("a"));
        upsert(&mut cs, "/m.md", create_file("m"));
        let paths: Vec<_> = cs.iter_all().map(|(p, _)| p.as_str().to_string()).collect();
        assert_eq!(paths, vec!["/a.md", "/m.md", "/z.md"]);
    }

    #[test]
    fn iter_staged_filters_unstaged() {
        let mut cs = ChangeSet::new();
        upsert(&mut cs, "/a.md", create_file("a"));
        upsert(&mut cs, "/b.md", create_file("b"));
        cs.unstage(&p("/b.md"));
        let staged: Vec<_> = cs
            .iter_staged()
            .map(|(p, _)| p.as_str().to_string())
            .collect();
        assert_eq!(staged, vec!["/a.md"]);
    }

    #[test]
    fn summary_counts_buckets() {
        let mut cs = ChangeSet::new();
        upsert(&mut cs, "/new.md", create_file("x"));
        upsert(
            &mut cs,
            "/upd.md",
            ChangeType::UpdateFile {
                content: "y".into(),
                meta: None,
                extensions: None,
            },
        );
        upsert(&mut cs, "/del.md", ChangeType::DeleteFile);
        cs.unstage(&p("/del.md"));
        let s = cs.summary();
        assert_eq!(s.creates_staged, 1);
        assert_eq!(s.updates_staged, 1);
        assert_eq!(s.deletes_unstaged, 1);
        assert_eq!(s.total(), 3);
        assert_eq!(s.total_staged(), 2);
    }

    #[test]
    fn create_file_roundtrip_serializes_extensions_explicitly() {
        let mut cs = ChangeSet::new();
        upsert(&mut cs, "/a.md", create_file("x"));

        let json = serde_json::to_string(&cs).unwrap();
        assert!(json.contains(r#""extensions":{}"#));

        let back = serde_json::from_str::<ChangeSet>(&json).unwrap();
        assert_eq!(back, cs);
    }

    #[test]
    fn create_file_deserialization_requires_extensions() {
        let json = r#"{
            "entries": {
                "/a.md": {
                    "change": {
                        "CreateFile": {
                            "content": "x",
                            "meta": {
                                "schema": 1,
                                "kind": "page",
                                "authored": {},
                                "derived": {}
                            }
                        }
                    },
                    "staged": true,
                    "timestamp": 1234
                }
            }
        }"#;

        let err = serde_json::from_str::<ChangeSet>(json).unwrap_err();
        assert!(
            err.to_string().contains("missing field `extensions`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn deserialization_rejects_non_canonical_entry_paths() {
        let json = r#"{
            "entries": {
                "/a/../b.md": {
                    "change": "DeleteFile",
                    "staged": true,
                    "timestamp": 1234
                }
            }
        }"#;

        let err = serde_json::from_str::<ChangeSet>(json).unwrap_err();
        assert!(
            err.to_string().contains("parent segment"),
            "unexpected error: {err}"
        );
    }
}
