//! Manifest-driven file metadata projection.
//!
//! `FileMeta` lives here, in `shared`, so reader and other surface UIs can
//! consume the same projection. The struct mirrors the subset of
//! `FsEntry::File` fields that those UIs care about.
//!
//! Note: the similarly named `FileMetaStrip` (in `shared/file_meta_strip`)
//! is a render component, not a data type.

use websh_core::domain::{FsEntry, ImageDim, LinkRef, NodeKind, PageSize, VirtualPath};
use websh_core::filesystem::GlobalFs;
use websh_core::support::format::{format_thousands_u32, reading_time_minutes};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMeta {
    pub title: String,
    pub description: Option<String>,
    pub size: Option<u64>,
    pub modified: Option<u64>,
    pub date: Option<String>,
    pub tags: Vec<String>,
    pub links: Vec<LinkRef>,
    /// Effective node kind. Defaults to [`NodeKind::Asset`] when no entry
    /// is found (matches the engine's fallback for unknown files).
    pub kind: NodeKind,
    /// PDF page geometry (PostScript points). Set only for `.pdf` files
    /// where `lopdf` could parse a MediaBox.
    pub page_size: Option<PageSize>,
    /// Total PDF page count.
    pub page_count: Option<u32>,
    /// Pixel dimensions for raster images (`.png` / `.jpg` / `.gif` /
    /// `.webp`).
    pub image_dimensions: Option<ImageDim>,
    /// Whitespace-tokenized word count for markdown bodies (frontmatter
    /// excluded).
    pub word_count: Option<u32>,
}

impl FileMeta {
    pub fn has_display_meta(&self) -> bool {
        self.date
            .as_ref()
            .is_some_and(|date| !date.trim().is_empty())
            || self.tags.iter().any(|tag| !tag.trim().is_empty())
    }

    pub fn clean_date(&self) -> Option<String> {
        self.date
            .as_ref()
            .map(|date| date.trim().to_string())
            .filter(|date| !date.is_empty())
    }

    pub fn clean_tags(&self) -> Vec<String> {
        self.tags
            .iter()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect()
    }

    pub fn clean_links(&self) -> Vec<LinkRef> {
        self.links
            .iter()
            .filter_map(|link| {
                let label = link.label.trim();
                let url = link.url.trim();
                if label.is_empty() || url.is_empty() {
                    return None;
                }
                Some(LinkRef {
                    label: label.to_string(),
                    url: url.to_string(),
                    kind: link
                        .kind
                        .as_deref()
                        .map(str::trim)
                        .filter(|kind| !kind.is_empty())
                        .map(str::to_string),
                })
            })
            .collect()
    }

    /// Kind-aware size chunks: words+min for `Page`, pages for
    /// `Document`, dimensions for `Asset`. Each chunk renders as its
    /// own sibling `<span>` so the host container's `·` separator CSS
    /// drives spacing.
    pub fn size_summary_parts(&self) -> Vec<String> {
        size_summary_parts(
            self.kind,
            self.word_count,
            self.page_count,
            self.image_dimensions.as_ref(),
        )
    }
}

/// Free-function entry point for callers holding a `NodeMetadata`
/// directly (skip the `FileMeta` projection).
pub fn size_summary_parts(
    kind: NodeKind,
    word_count: Option<u32>,
    page_count: Option<u32>,
    image_dimensions: Option<&ImageDim>,
) -> Vec<String> {
    match kind {
        NodeKind::Page => word_count
            .map(|words| {
                vec![
                    format!("{} words", format_thousands_u32(words)),
                    format!("{} min", reading_time_minutes(words)),
                ]
            })
            .unwrap_or_default(),
        NodeKind::Document => page_count
            .map(|pages| {
                vec![format!(
                    "{pages} {}",
                    if pages == 1 { "page" } else { "pages" }
                )]
            })
            .unwrap_or_default(),
        NodeKind::Asset => image_dimensions
            .map(|dim| vec![format!("{}×{}", dim.width, dim.height)])
            .unwrap_or_default(),
        NodeKind::App
        | NodeKind::Redirect
        | NodeKind::Data
        | NodeKind::Directory
        | NodeKind::Bundle => Vec::new(),
    }
}

/// Project the `FsEntry` at `path` into a `FileMeta`. Returns `None` for
/// missing entries.
pub fn file_meta_for_path(fs: &GlobalFs, path: &VirtualPath) -> Option<FileMeta> {
    fs.get_entry(path).and_then(file_meta_for_entry)
}

pub fn file_meta_for_entry(entry: &FsEntry) -> Option<FileMeta> {
    match entry {
        FsEntry::File { meta, .. } | FsEntry::Directory { meta, .. } => Some(FileMeta {
            title: meta.title().unwrap_or("").to_string(),
            description: meta.description().map(str::to_string),
            size: meta.size_bytes(),
            modified: meta.modified_at(),
            date: meta.date().map(str::to_string),
            tags: meta.tags_owned(),
            links: meta.links_owned(),
            kind: meta.effective_kind(),
            page_size: meta.page_size().copied(),
            page_count: meta.page_count(),
            image_dimensions: meta.image_dimensions().copied(),
            word_count: meta.word_count(),
        }),
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    fn meta_with_kind(kind: NodeKind) -> FileMeta {
        FileMeta {
            kind,
            ..FileMeta::default()
        }
    }

    #[wasm_bindgen_test]
    fn size_summary_parts_splits_words_and_minutes_for_page() {
        let mut m = meta_with_kind(NodeKind::Page);
        m.word_count = Some(2_140);
        // Words and minutes are separate chunks so the host container's
        // `·` separator CSS draws the dot between them — keeps spacing
        // identical to the dot between a chunk and a tag downstream.
        assert_eq!(m.size_summary_parts(), vec!["2,140 words", "9 min"]);
    }

    #[wasm_bindgen_test]
    fn size_summary_parts_single_chunk_for_pdf_with_singular_plural() {
        let mut m = meta_with_kind(NodeKind::Document);
        m.page_count = Some(1);
        assert_eq!(m.size_summary_parts(), vec!["1 page"]);
        m.page_count = Some(12);
        assert_eq!(m.size_summary_parts(), vec!["12 pages"]);
    }

    #[wasm_bindgen_test]
    fn size_summary_parts_single_chunk_for_image() {
        let mut m = meta_with_kind(NodeKind::Asset);
        m.image_dimensions = Some(ImageDim {
            width: 1920,
            height: 1080,
        });
        // `×` is a glyph inside the dimension chunk, not a separator.
        assert_eq!(m.size_summary_parts(), vec!["1920×1080"]);
    }

    #[wasm_bindgen_test]
    fn size_summary_parts_empty_when_metric_field_absent() {
        // Markdown page with no word_count → no chunks.
        assert!(
            meta_with_kind(NodeKind::Page)
                .size_summary_parts()
                .is_empty()
        );
        // App / Redirect / Data / Directory have no natural metric.
        for kind in [
            NodeKind::App,
            NodeKind::Redirect,
            NodeKind::Data,
            NodeKind::Directory,
            NodeKind::Bundle,
        ] {
            assert!(
                meta_with_kind(kind).size_summary_parts().is_empty(),
                "expected empty summary for {kind:?}",
            );
        }
    }

    #[wasm_bindgen_test]
    fn size_summary_parts_free_function_matches_method() {
        // Pin the free function to the same output as the FileMeta
        // method so the two never drift apart.
        let dim = ImageDim {
            width: 800,
            height: 600,
        };
        assert_eq!(
            size_summary_parts(NodeKind::Asset, None, None, Some(&dim)),
            vec!["800×600"],
        );
        assert_eq!(
            size_summary_parts(NodeKind::Page, Some(345), None, None),
            vec!["345 words", "2 min"],
        );
    }
}
