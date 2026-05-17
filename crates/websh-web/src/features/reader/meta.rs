//! `ReaderMeta` — combined intent + manifest projection consumed by views.
//!
//! `reader_meta` is the public entry; `build_reader_meta` is the pure
//! inner combinator unit-tested below.

use leptos::prelude::With;

use crate::app::AppContext;
use crate::shared::components::{FileMeta, file_meta_for_path, size_summary_parts};
use websh_core::domain::{
    BundleVariant, FileType, ImageDim, LinkRef, NodeKind, PageSize, VirtualPath,
};
use websh_core::support::format::{format_date_iso, format_size};
use websh_core::support::media_type_for_path;

use super::intent::ReaderIntent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderMeta {
    pub title: String,
    pub canonical_path: VirtualPath,
    pub modified_iso: Option<String>,
    pub date: Option<String>,
    pub size_pretty: Option<String>,
    pub tags: Vec<String>,
    pub links: Vec<LinkRef>,
    pub description: String,
    pub media_type_hint: Option<&'static str>,
    /// Effective kind, used by the title strip to render a friendly label
    /// (e.g. `Page` → "Note") and by view dispatch to pick the right
    /// metric for the right-hand side of the strip.
    pub kind: NodeKind,
    /// PDF MediaBox geometry (points). Drives iframe `aspect-ratio` in
    /// [`super::views::pdf::PdfReaderView`] and the `· N pages` chip.
    pub page_size: Option<PageSize>,
    pub page_count: Option<u32>,
    /// Pixel dimensions for raster images. Drives `<img width/height>` in
    /// [`super::views::asset::AssetReaderView`] (preventing layout shift)
    /// and the `· W×H` chip in the title strip.
    pub image_dimensions: Option<ImageDim>,
    /// Markdown word count (frontmatter excluded). Drives the
    /// `N words · M min` chip on the right side of the title strip.
    pub word_count: Option<u32>,
    pub variants: Vec<ReaderVariantLink>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderVariantLink {
    pub id: String,
    pub label: String,
    pub href: String,
    pub locale: Option<String>,
    pub active: bool,
}

impl ReaderMeta {
    /// Display value for the single `Date` row — author-declared `date`
    /// preferred, mechanical `modified_iso` as fallback, `None` if neither.
    pub fn display_date(&self) -> Option<String> {
        self.date.clone().or_else(|| self.modified_iso.clone())
    }

    /// Kind-aware size chunks, sharing logic with
    /// [`FileMeta::size_summary_parts`] so the same file produces the
    /// same chunks in the title strip and the ledger entry meta line.
    pub fn size_summary_parts(&self) -> Vec<String> {
        size_summary_parts(
            self.kind,
            self.word_count,
            self.page_count,
            self.image_dimensions.as_ref(),
        )
    }
}

pub fn reader_meta(ctx: AppContext, intent: &ReaderIntent) -> ReaderMeta {
    ctx.view_global_fs.with(|fs| match intent {
        ReaderIntent::BundleVariant {
            bundle_path,
            variant_id,
            variant_path,
        } => {
            let bundle_meta = file_meta_for_path(fs, bundle_path).unwrap_or_default();
            let variant_meta = file_meta_for_path(fs, variant_path).unwrap_or_default();
            let variant_authored_title = fs
                .node_metadata(variant_path)
                .and_then(|meta| meta.authored.title.clone());
            let variant_links = fs
                .node_metadata(bundle_path)
                .and_then(|meta| meta.bundle.as_ref())
                .map(|bundle| variant_links(bundle_path, variant_id, &bundle.variants))
                .unwrap_or_default();
            build_bundle_reader_meta(
                intent,
                bundle_path,
                variant_id,
                bundle_meta,
                variant_meta,
                variant_authored_title,
                variant_links,
            )
        }
        _ => {
            let node_path = node_path_for(intent);
            let file_meta = file_meta_for_path(fs, &node_path).unwrap_or_default();
            build_reader_meta(intent, &node_path, file_meta)
        }
    })
}

fn node_path_for(intent: &ReaderIntent) -> VirtualPath {
    match intent {
        ReaderIntent::Markdown { node_path }
        | ReaderIntent::Html { node_path }
        | ReaderIntent::Plain { node_path }
        | ReaderIntent::Redirect { node_path }
        | ReaderIntent::Asset { node_path, .. } => node_path.clone(),
        ReaderIntent::BundleVariant { bundle_path, .. } => bundle_path.clone(),
    }
}

fn build_reader_meta(intent: &ReaderIntent, node_path: &VirtualPath, meta: FileMeta) -> ReaderMeta {
    let title = non_empty(meta.title.clone()).unwrap_or_else(|| fallback_title_for_path(node_path));

    let modified_iso = meta.modified.map(format_date_iso);
    let date = meta.clean_date();
    let size_pretty = meta.size.map(|size| format_size(Some(size), false));
    let tags = meta.clean_tags();
    let links = meta.clean_links();
    let description = meta.description.as_deref().unwrap_or("").trim().to_string();
    let media_type_hint = media_type_hint_for(intent);

    ReaderMeta {
        title,
        canonical_path: node_path.clone(),
        modified_iso,
        date,
        size_pretty,
        tags,
        links,
        description,
        media_type_hint,
        kind: meta.kind,
        page_size: meta.page_size,
        page_count: meta.page_count,
        image_dimensions: meta.image_dimensions,
        word_count: meta.word_count,
        variants: Vec::new(),
    }
}

fn build_bundle_reader_meta(
    intent: &ReaderIntent,
    bundle_path: &VirtualPath,
    variant_id: &str,
    bundle_meta: FileMeta,
    variant_meta: FileMeta,
    variant_authored_title: Option<String>,
    variants: Vec<ReaderVariantLink>,
) -> ReaderMeta {
    let fallback_title = fallback_title_for_path(bundle_path);
    let title = variant_authored_title
        .and_then(non_empty)
        .or_else(|| non_empty(bundle_meta.title.clone()))
        .or_else(|| non_empty(variant_meta.title.clone()))
        .unwrap_or(fallback_title);
    let modified_iso = variant_meta.modified.map(format_date_iso);
    let date = bundle_meta
        .clean_date()
        .or_else(|| variant_meta.clean_date());
    let size_pretty = variant_meta.size.map(|size| format_size(Some(size), false));
    let tags = {
        let bundle_tags = bundle_meta.clean_tags();
        if bundle_tags.is_empty() {
            variant_meta.clean_tags()
        } else {
            bundle_tags
        }
    };
    let links = {
        let mut links = bundle_meta.clean_links();
        links.extend(variant_meta.clean_links());
        links
    };
    let description = variant_meta
        .description
        .as_deref()
        .and_then(|text| non_empty(text.to_string()))
        .or_else(|| {
            bundle_meta
                .description
                .as_deref()
                .and_then(|text| non_empty(text.to_string()))
        })
        .unwrap_or_default();
    let media_type_hint = media_type_hint_for(intent);

    ReaderMeta {
        title,
        canonical_path: bundle_path.clone(),
        modified_iso,
        date,
        size_pretty,
        tags,
        links,
        description,
        media_type_hint,
        kind: variant_meta.kind,
        page_size: variant_meta.page_size,
        page_count: variant_meta.page_count,
        image_dimensions: variant_meta.image_dimensions,
        word_count: variant_meta.word_count,
        variants: variants
            .into_iter()
            .map(|mut variant| {
                variant.active = variant.id == variant_id;
                variant
            })
            .collect(),
    }
}

fn variant_links(
    bundle_path: &VirtualPath,
    active_id: &str,
    variants: &[BundleVariant],
) -> Vec<ReaderVariantLink> {
    variants
        .iter()
        .map(|variant| ReaderVariantLink {
            id: variant.id.clone(),
            label: variant.label.clone(),
            href: format!(
                "#{}",
                bundle_path.join(&variant.id).as_str().trim_end_matches('/')
            ),
            locale: variant.locale.clone(),
            active: variant.id == active_id,
        })
        .collect()
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn fallback_title_for_path(path: &VirtualPath) -> String {
    path.file_name()
        .map(title_from_file_name)
        .unwrap_or_else(|| path.as_str().trim_matches('/').to_string())
}

fn title_from_file_name(name: &str) -> String {
    name.rsplit_once('.')
        .and_then(|(stem, _ext)| (!stem.is_empty()).then_some(stem.to_string()))
        .unwrap_or_else(|| name.to_string())
}

fn media_type_hint_for(intent: &ReaderIntent) -> Option<&'static str> {
    match intent {
        ReaderIntent::Markdown { .. } => Some("UTF-8 · CommonMark"),
        ReaderIntent::Html { .. } => Some("UTF-8 · sanitized"),
        ReaderIntent::Plain { .. } => Some("UTF-8 · LF"),
        ReaderIntent::Asset { .. } | ReaderIntent::Redirect { .. } => None,
        ReaderIntent::BundleVariant { variant_path, .. } => {
            match FileType::from_path(variant_path.as_str()) {
                FileType::Markdown => Some("UTF-8 · CommonMark"),
                FileType::Html => Some("UTF-8 · sanitized"),
                FileType::Unknown => Some("UTF-8 · LF"),
                FileType::Pdf | FileType::Image | FileType::Link => None,
            }
        }
    }
}

pub fn type_tag_for_intent(intent: &ReaderIntent) -> Option<String> {
    match intent {
        ReaderIntent::Markdown { .. } => Some("markdown".to_string()),
        ReaderIntent::Html { .. } => Some("html".to_string()),
        ReaderIntent::Plain { .. } => Some("text".to_string()),
        ReaderIntent::Asset { media_type, .. } => Some(media_type.clone()),
        ReaderIntent::Redirect { .. } => None,
        ReaderIntent::BundleVariant { variant_path, .. } => {
            Some(match FileType::from_path(variant_path.as_str()) {
                FileType::Markdown => "markdown".to_string(),
                FileType::Html => "html".to_string(),
                FileType::Pdf | FileType::Image => {
                    media_type_for_path(variant_path.as_str()).to_string()
                }
                FileType::Link => "link".to_string(),
                FileType::Unknown => "text".to_string(),
            })
        }
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    fn vp(path: &str) -> VirtualPath {
        VirtualPath::from_absolute(path).expect("test path")
    }

    fn populated_meta() -> FileMeta {
        FileMeta {
            title: "Sample".to_string(),
            description: Some("An abstract.".to_string()),
            size: Some(1024),
            modified: Some(1_704_067_200),
            date: Some("2026-04-22".to_string()),
            tags: vec!["paper".to_string(), "draft".to_string()],
            ..FileMeta::default()
        }
    }

    #[wasm_bindgen_test]
    fn markdown_intent_with_full_meta() {
        let intent = ReaderIntent::Markdown {
            node_path: vp("/blog/hello.md"),
        };
        let meta = build_reader_meta(&intent, &vp("/blog/hello.md"), populated_meta());
        assert_eq!(meta.title, "Sample");
        assert_eq!(meta.media_type_hint, Some("UTF-8 · CommonMark"));
        assert_eq!(meta.date.as_deref(), Some("2026-04-22"));
        assert!(meta.modified_iso.is_some());
        assert_eq!(meta.tags, vec!["paper", "draft"]);
    }

    #[wasm_bindgen_test]
    fn plain_intent_with_size_only() {
        let intent = ReaderIntent::Plain {
            node_path: vp("/notes/x.txt"),
        };
        let meta = FileMeta {
            size: Some(2048),
            ..FileMeta::default()
        };
        let result = build_reader_meta(&intent, &vp("/notes/x.txt"), meta);
        assert_eq!(result.title, "x");
        assert_eq!(result.media_type_hint, Some("UTF-8 · LF"));
        assert!(result.size_pretty.is_some());
        assert!(result.date.is_none());
        assert!(result.modified_iso.is_none());
        assert!(result.tags.is_empty());
        assert_eq!(result.description, "");
    }

    #[wasm_bindgen_test]
    fn pdf_intent_preserves_description() {
        let intent = ReaderIntent::Asset {
            node_path: vp("/papers/x.pdf"),
            media_type: "application/pdf".to_string(),
        };
        let meta = FileMeta {
            title: "PDF Title".to_string(),
            description: Some("  We present a thing.  ".to_string()),
            ..FileMeta::default()
        };
        let result = build_reader_meta(&intent, &vp("/papers/x.pdf"), meta);
        assert_eq!(result.title, "PDF Title");
        assert_eq!(result.media_type_hint, None);
        assert_eq!(result.description, "We present a thing.");
    }

    #[wasm_bindgen_test]
    fn image_intent_with_empty_meta_has_no_description() {
        let intent = ReaderIntent::Asset {
            node_path: vp("/cover.png"),
            media_type: "image/png".to_string(),
        };
        let result = build_reader_meta(&intent, &vp("/cover.png"), FileMeta::default());
        assert_eq!(result.title, "cover");
        assert!(result.description.is_empty());
        assert!(result.size_pretty.is_none());
    }

    #[wasm_bindgen_test]
    fn redirect_intent_constructs() {
        let intent = ReaderIntent::Redirect {
            node_path: vp("/x.link"),
        };
        let result = build_reader_meta(&intent, &vp("/x.link"), FileMeta::default());
        assert_eq!(result.title, "x");
        assert_eq!(result.media_type_hint, None);
    }

    #[wasm_bindgen_test]
    fn bundle_title_uses_bundle_before_variant_derived_title() {
        let intent = ReaderIntent::BundleVariant {
            bundle_path: vp("/writing/foo"),
            variant_id: "ko".to_string(),
            variant_path: vp("/writing/foo/ko.md"),
        };
        let result = build_bundle_reader_meta(
            &intent,
            &vp("/writing/foo"),
            "ko",
            FileMeta {
                title: "Bundle Title".to_string(),
                ..FileMeta::default()
            },
            FileMeta {
                title: "ko".to_string(),
                kind: NodeKind::Page,
                ..FileMeta::default()
            },
            None,
            Vec::new(),
        );

        assert_eq!(result.title, "Bundle Title");
    }

    #[wasm_bindgen_test]
    fn bundle_title_prefers_variant_authored_title() {
        let intent = ReaderIntent::BundleVariant {
            bundle_path: vp("/writing/foo"),
            variant_id: "ko".to_string(),
            variant_path: vp("/writing/foo/ko.md"),
        };
        let result = build_bundle_reader_meta(
            &intent,
            &vp("/writing/foo"),
            "ko",
            FileMeta {
                title: "Bundle Title".to_string(),
                ..FileMeta::default()
            },
            FileMeta {
                title: "ko".to_string(),
                kind: NodeKind::Page,
                ..FileMeta::default()
            },
            Some("Korean Title".to_string()),
            Vec::new(),
        );

        assert_eq!(result.title, "Korean Title");
    }

    fn reader_meta_with(date: Option<&str>, modified_iso: Option<&str>) -> ReaderMeta {
        ReaderMeta {
            title: "x".to_string(),
            canonical_path: vp("/x"),
            modified_iso: modified_iso.map(String::from),
            date: date.map(String::from),
            size_pretty: None,
            tags: vec![],
            links: Vec::new(),
            description: String::new(),
            media_type_hint: None,
            kind: NodeKind::Page,
            page_size: None,
            page_count: None,
            image_dimensions: None,
            word_count: None,
            variants: Vec::new(),
        }
    }

    #[wasm_bindgen_test]
    fn display_date_cases() {
        let cases = [
            (Some("2026-04-22"), Some("2026-04-30"), Some("2026-04-22")),
            (None, Some("2026-04-30"), Some("2026-04-30")),
            (None, None, None),
        ];

        for (date, modified, expected) in cases {
            let m = reader_meta_with(date, modified);
            assert_eq!(m.display_date().as_deref(), expected);
        }
    }

    #[wasm_bindgen_test]
    fn title_strips_extension() {
        let intent = ReaderIntent::Markdown {
            node_path: vp("/blog/some.thing.md"),
        };
        let result = build_reader_meta(&intent, &vp("/blog/some.thing.md"), FileMeta::default());
        assert_eq!(result.title, "some.thing"); // rsplit only trims last extension
    }
}
