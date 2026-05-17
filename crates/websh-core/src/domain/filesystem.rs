use std::collections::HashMap;
use std::fmt;

use super::mempool::MempoolFields;
use super::metadata::{NodeKind, NodeMetadata};

/// Domain-extension sibling blocks on a file entry — one optional
/// typed field per domain, populated from the manifest entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EntryExtensions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mempool: Option<MempoolFields>,
}

/// Unix-style permission display (computed at runtime).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayPermissions {
    pub is_dir: bool,
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl fmt::Display for DisplayPermissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}{}",
            if self.is_dir { 'd' } else { '-' },
            if self.read { 'r' } else { '-' },
            if self.write { 'w' } else { '-' },
            if self.execute { 'x' } else { '-' },
        )
    }
}

/// Directory entry returned by canonical filesystem directory listings.
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub path: crate::domain::VirtualPath,
    pub is_dir: bool,
    pub title: String,
    pub meta: Option<NodeMetadata>,
}

/// Supported file types for the reader
#[derive(Clone, Debug, PartialEq)]
pub enum FileType {
    Html,
    Markdown,
    Pdf,
    Image,
    Link,
    Unknown,
}

impl FileType {
    /// Detect file type from path extension
    pub fn from_path(path: &str) -> Self {
        match path.rsplit('.').next().map(|s| s.to_lowercase()).as_deref() {
            Some("html" | "htm") => Self::Html,
            Some("md") => Self::Markdown,
            Some("pdf") => Self::Pdf,
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg") => Self::Image,
            Some("link") => Self::Link,
            _ => Self::Unknown,
        }
    }
}

/// Represents an entry in the canonical filesystem tree. Each entry now
/// carries a single [`NodeMetadata`] record covering both authored and
/// derived fields.
#[derive(Clone, Debug)]
pub enum FsEntry {
    Directory {
        children: HashMap<String, FsEntry>,
        meta: NodeMetadata,
    },
    File {
        content_path: Option<String>,
        meta: NodeMetadata,
        extensions: EntryExtensions,
    },
}

impl FsEntry {
    /// Create a file without content path (static file).
    pub fn file() -> Self {
        FsEntry::File {
            content_path: None,
            meta: NodeMetadata {
                kind: NodeKind::Asset,
                bundle: None,
                ..NodeMetadata::default()
            },
            extensions: EntryExtensions::default(),
        }
    }

    /// Create a file with full metadata and domain extensions.
    pub fn content_file_with_meta(
        path: &str,
        meta: NodeMetadata,
        extensions: EntryExtensions,
    ) -> Self {
        FsEntry::File {
            content_path: Some(path.to_string()),
            meta,
            extensions,
        }
    }

    pub fn is_directory(&self) -> bool {
        matches!(self, FsEntry::Directory { .. })
    }

    pub fn is_restricted(&self) -> bool {
        match self {
            FsEntry::File { meta, .. } | FsEntry::Directory { meta, .. } => meta.is_restricted(),
        }
    }

    /// Get the metadata regardless of file/directory.
    pub fn meta(&self) -> &NodeMetadata {
        match self {
            FsEntry::File { meta, .. } | FsEntry::Directory { meta, .. } => meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut NodeMetadata {
        match self {
            FsEntry::File { meta, .. } | FsEntry::Directory { meta, .. } => meta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_type_detection() {
        assert_eq!(FileType::from_path("index.html"), FileType::Html);
        assert_eq!(FileType::from_path("blog/hello.md"), FileType::Markdown);
        assert_eq!(FileType::from_path("papers/research.pdf"), FileType::Pdf);
        assert_eq!(FileType::from_path("images/photo.png"), FileType::Image);
        assert_eq!(FileType::from_path("images/photo.JPG"), FileType::Image);
        assert_eq!(FileType::from_path("links/github.link"), FileType::Link);
        assert_eq!(FileType::from_path("unknown/file.xyz"), FileType::Unknown);
    }
}
