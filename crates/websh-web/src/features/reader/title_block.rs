//! `Ident` strip + `TitleBlock` (h1 + per-intent `MetaTable`).
//!
//! The `Ident` strip sits above the title and shows two single-line
//! summaries:
//!
//! - **Left**: friendly kind label + compact date, e.g. `"Note 2026/0314"`.
//! - **Right**: a kind-specific size chip — words+min for prose, page
//!   count for PDFs, pixel dimensions for images.
//!
//! The `MetaTable` below the title is the verbose breakdown
//! (Type / Size / Date / Tags / Caption) and is unrelated to the strip.

use leptos::prelude::*;

use crate::shared::components::{IdentifierStrip, MetaRow, MetaTable};
use websh_core::domain::{FileType, LinkRef, NodeKind};
use websh_core::support::format::format_date_compact;

use super::actions::{ReaderActionsBindings, ReaderActionsMenu};
use super::css;
use super::intent::ReaderIntent;
use super::meta::{ReaderMeta, ReaderVariantLink, type_tag_for_intent};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowSpec {
    Type {
        tag: String,
        hint: Option<&'static str>,
    },
    Size {
        value: String,
    },
    Date {
        value: String,
    },
    Tags {
        items: Vec<String>,
    },
    Links {
        items: Vec<LinkRef>,
    },
    Variants {
        items: Vec<ReaderVariantLink>,
    },
    Caption {
        text: String,
    },
}

pub fn rows_for(intent: &ReaderIntent, meta: &ReaderMeta) -> Vec<RowSpec> {
    let mut rows = Vec::new();

    let type_tag = type_tag_for_intent(intent);

    if let Some(tag) = type_tag {
        rows.push(RowSpec::Type {
            tag,
            hint: meta.media_type_hint,
        });
    }

    if meta.variants.len() > 1 {
        rows.push(RowSpec::Variants {
            items: meta.variants.clone(),
        });
    }

    let wants_size =
        !intent_is_markdown(intent) && !intent_is_html(intent) && !intent_is_redirect(intent);
    if wants_size && let Some(size) = meta.size_pretty.clone() {
        rows.push(RowSpec::Size { value: size });
    }

    if !matches!(intent, ReaderIntent::Redirect { .. })
        && let Some(date) = meta.display_date()
    {
        rows.push(RowSpec::Date { value: date });
    }

    let wants_tags = intent_is_markdown(intent) || intent_is_pdf(intent);
    if wants_tags && !meta.tags.is_empty() {
        rows.push(RowSpec::Tags {
            items: meta.tags.clone(),
        });
    }

    if !meta.links.is_empty() {
        rows.push(RowSpec::Links {
            items: meta.links.clone(),
        });
    }

    let wants_caption = intent_is_image(intent);
    if wants_caption && !meta.description.is_empty() {
        rows.push(RowSpec::Caption {
            text: meta.description.clone(),
        });
    }

    rows
}

fn intent_is_markdown(intent: &ReaderIntent) -> bool {
    matches!(intent, ReaderIntent::Markdown { .. })
        || matches!(
            intent,
            ReaderIntent::BundleVariant { variant_path, .. }
                if FileType::from_path(variant_path.as_str()) == FileType::Markdown
        )
}

fn intent_is_html(intent: &ReaderIntent) -> bool {
    matches!(intent, ReaderIntent::Html { .. })
        || matches!(
            intent,
            ReaderIntent::BundleVariant { variant_path, .. }
                if FileType::from_path(variant_path.as_str()) == FileType::Html
        )
}

fn intent_is_redirect(intent: &ReaderIntent) -> bool {
    matches!(intent, ReaderIntent::Redirect { .. })
        || matches!(
            intent,
            ReaderIntent::BundleVariant { variant_path, .. }
                if FileType::from_path(variant_path.as_str()) == FileType::Link
        )
}

fn intent_is_pdf(intent: &ReaderIntent) -> bool {
    matches!(intent, ReaderIntent::Asset { media_type, .. } if media_type == "application/pdf")
        || matches!(
            intent,
            ReaderIntent::BundleVariant { variant_path, .. }
                if FileType::from_path(variant_path.as_str()) == FileType::Pdf
        )
}

fn intent_is_image(intent: &ReaderIntent) -> bool {
    matches!(intent, ReaderIntent::Asset { media_type, .. } if media_type.starts_with("image/"))
        || matches!(
            intent,
            ReaderIntent::BundleVariant { variant_path, .. }
                if FileType::from_path(variant_path.as_str()) == FileType::Image
        )
}

/// Friendly display label for a [`NodeKind`]. Shorter than the enum
/// variant name and closer to how authors talk about their content
/// ("Note" rather than "Page", "Doc" rather than "Document").
fn kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Page => "Note",
        NodeKind::Document => "Doc",
        NodeKind::App => "App",
        NodeKind::Asset => "Asset",
        NodeKind::Redirect => "Link",
        NodeKind::Data => "Data",
        NodeKind::Directory => "Folder",
        NodeKind::Bundle => "Bundle",
    }
}

#[component]
pub fn Ident(meta: Memo<ReaderMeta>) -> impl IntoView {
    view! {
        {move || {
            let m = meta.get();
            let kind = kind_label(m.kind).to_string();
            // Compact date on the left side (next to kind). Falls back to
            // the ISO `modified_iso` if no authored date is set; the strip
            // omits the date when neither is present.
            let date = m
                .display_date()
                .as_deref()
                .and_then(format_date_compact);
            // Each metric chunk renders as its own sibling `<span>`; the
            // `.identMetric > span + span::before` rule in the module
            // CSS draws the `·` separator between chunks at the same
            // spacing token as the ledger card meta line.
            let parts = m.size_summary_parts();
            view! {
                <IdentifierStrip muted=true>
                    <span class=css::identLeft>
                        <span>{kind}</span>
                        {date.map(|value| view! { <span>{value}</span> })}
                    </span>
                    {(!parts.is_empty()).then(|| view! {
                        <span class=css::identMetric>
                            {parts.into_iter().map(|chunk| view! {
                                <span>{chunk}</span>
                            }).collect_view()}
                        </span>
                    })}
                </IdentifierStrip>
            }
        }}
    }
}

#[component]
pub fn TitleBlock(
    intent: Memo<ReaderIntent>,
    meta: Memo<ReaderMeta>,
    actions: ReaderActionsBindings,
    variants_disabled: Signal<bool>,
    set_preferred_locale: Callback<String>,
) -> impl IntoView {
    view! {
        <div class=css::titleBlock>
            <div class=css::titleRow>
                <h1 class=css::title>{move || meta.get().title.clone()}</h1>
                <ReaderActionsMenu actions=actions />
            </div>
            {move || {
                let i = intent.get();
                let m = meta.get();
                let rows = rows_for(&i, &m);
                let disabled = variants_disabled.get();
                if rows.is_empty() {
                    None
                } else {
                    Some(view! {
                        <MetaTable class=css::metaTable aria_label="file metadata">
                            {rows.into_iter()
                                .map(|row| render_row(row, disabled, set_preferred_locale))
                                .collect_view()}
                        </MetaTable>
                    })
                }
            }}
        </div>
    }
}

fn render_row(
    spec: RowSpec,
    variants_disabled: bool,
    set_preferred_locale: Callback<String>,
) -> AnyView {
    match spec {
        RowSpec::Type { tag, hint } => view! {
            <MetaRow
                label="Type"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                <span class=css::metaTag>{tag}</span>
                {hint.map(|h| view! { <span class=css::metaDim>{h}</span> })}
            </MetaRow>
        }
        .into_any(),
        RowSpec::Size { value } => view! {
            <MetaRow
                label="Size"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                {value}
            </MetaRow>
        }
        .into_any(),
        RowSpec::Date { value } => view! {
            <MetaRow
                label="Date"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                {value}
            </MetaRow>
        }
        .into_any(),
        RowSpec::Tags { items } => view! {
            <MetaRow
                label="Tags"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                {items.into_iter().map(|tag| view! {
                    <span class=css::metaTag>{tag}</span>
                }).collect_view()}
            </MetaRow>
        }
        .into_any(),
        RowSpec::Links { items } => view! {
            <MetaRow
                label="Links"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                <span class=css::linkList>
                    {items.into_iter().map(render_metadata_link).collect_view()}
                </span>
            </MetaRow>
        }
        .into_any(),
        RowSpec::Variants { items } => view! {
            <MetaRow
                label="Variants"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                <span class=css::variantList>
                    {items.into_iter()
                        .map(|variant| render_variant_link(
                            variant,
                            variants_disabled,
                            set_preferred_locale,
                        ))
                        .collect_view()}
                </span>
            </MetaRow>
        }
        .into_any(),
        RowSpec::Caption { text } => view! {
            <MetaRow
                label="Caption"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                {text}
            </MetaRow>
        }
        .into_any(),
    }
}

fn render_variant_link(
    variant: ReaderVariantLink,
    disabled: bool,
    set_preferred_locale: Callback<String>,
) -> AnyView {
    if variant.active {
        return view! {
            <span class=format!("{} {}", css::variantChip, css::variantActive) aria-current="true">
                {variant.label}
            </span>
        }
        .into_any();
    }
    if disabled {
        return view! {
            <span class=format!("{} {}", css::variantChip, css::variantDisabled) aria-disabled="true">
                {variant.label}
            </span>
        }
        .into_any();
    }
    let locale = variant.locale.clone();
    let persist_locale = move |_| {
        if let Some(locale) = locale.clone() {
            set_preferred_locale.run(locale);
        }
    };
    view! {
        <a class=css::variantChip href=variant.href on:click=persist_locale>
            {variant.label}
        </a>
    }
    .into_any()
}

fn render_metadata_link(link: LinkRef) -> AnyView {
    let href = link.url.trim().to_string();
    let label = link.label.trim().to_string();
    if !is_safe_metadata_link(&href) {
        return view! {
            <span class=format!("{} {}", css::linkChip, css::linkDisabled) aria-disabled="true">
                {label}
            </span>
        }
        .into_any();
    }

    view! {
        <a class=css::linkChip href=href target="_blank" rel="noopener noreferrer">
            {label}
        </a>
    }
    .into_any()
}

fn is_safe_metadata_link(url: &str) -> bool {
    let url = url.trim_start().to_ascii_lowercase();
    url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with('/')
        || url.starts_with('#')
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;
    use websh_core::domain::VirtualPath;

    wasm_bindgen_test_configure!(run_in_browser);

    fn vp(path: &str) -> VirtualPath {
        VirtualPath::from_absolute(path).expect("test path")
    }

    fn meta_with(date: Option<&str>, modified_iso: Option<&str>) -> ReaderMeta {
        ReaderMeta {
            title: "x".to_string(),
            canonical_path: vp("/x.md"),
            modified_iso: modified_iso.map(String::from),
            date: date.map(String::from),
            size_pretty: None,
            tags: vec![],
            links: Vec::new(),
            description: String::new(),
            media_type_hint: Some("UTF-8 · CommonMark"),
            kind: websh_core::domain::NodeKind::Page,
            page_size: None,
            page_count: None,
            image_dimensions: None,
            word_count: None,
            variants: Vec::new(),
        }
    }

    #[wasm_bindgen_test]
    fn date_row_cases() {
        let intent = ReaderIntent::Markdown {
            node_path: vp("/x.md"),
        };
        let cases = [
            (Some("2026-04-22"), Some("2026-04-30"), Some("2026-04-22")),
            (None, Some("2026-04-30"), Some("2026-04-30")),
            (None, None, None),
        ];

        for (date, modified, expected) in cases {
            let m = meta_with(date, modified);
            let rows = rows_for(&intent, &m);
            let actual = rows.iter().find_map(|r| match r {
                RowSpec::Date { value } => Some(value.clone()),
                _ => None,
            });
            assert_eq!(actual.as_deref(), expected, "rows: {rows:?}");
        }
    }

    #[wasm_bindgen_test]
    fn plain_emits_size_row_when_present() {
        let intent = ReaderIntent::Plain {
            node_path: vp("/x.txt"),
        };
        let mut m = meta_with(None, None);
        m.size_pretty = Some("2 KB".to_string());
        let rows = rows_for(&intent, &m);
        assert!(
            rows.iter().any(|r| matches!(r, RowSpec::Size { .. })),
            "expected Size row, got {rows:?}"
        );
    }

    #[wasm_bindgen_test]
    fn redirect_emits_no_rows() {
        let intent = ReaderIntent::Redirect {
            node_path: vp("/x.link"),
        };
        let m = meta_with(Some("2026-04-22"), None);
        let rows = rows_for(&intent, &m);
        assert!(rows.is_empty(), "redirect rows should be empty: {rows:?}");
    }

    #[wasm_bindgen_test]
    fn image_caption_appears_only_when_description_set() {
        let intent = ReaderIntent::Asset {
            node_path: vp("/cover.png"),
            media_type: "image/png".to_string(),
        };
        let mut m = meta_with(None, None);
        m.description = "Sunrise.".to_string();
        let rows = rows_for(&intent, &m);
        assert!(
            rows.iter().any(|r| matches!(r, RowSpec::Caption { .. })),
            "expected Caption row, got {rows:?}"
        );

        m.description = String::new();
        let rows2 = rows_for(&intent, &m);
        assert!(
            rows2.iter().all(|r| !matches!(r, RowSpec::Caption { .. })),
            "Caption should be omitted, got {rows2:?}"
        );
    }

    #[wasm_bindgen_test]
    fn kind_labels_cover_every_variant() {
        // Sanity check that each enum variant maps to a non-empty label.
        // Drives the leftmost token of the ident strip; an empty string
        // would render as just a date with a leading space.
        for kind in [
            NodeKind::Page,
            NodeKind::Document,
            NodeKind::App,
            NodeKind::Asset,
            NodeKind::Redirect,
            NodeKind::Data,
            NodeKind::Directory,
            NodeKind::Bundle,
        ] {
            assert!(!kind_label(kind).is_empty(), "label missing for {kind:?}");
        }
    }

    #[wasm_bindgen_test]
    fn reader_meta_size_summary_parts_delegates_to_shared() {
        // Spot-check that the ReaderMeta wrapper produces the same
        // chunks as the shared free function. Full per-kind coverage
        // lives with `FileMeta::size_summary_parts` in shared/file_meta.rs.
        let mut m = meta_with(None, None);
        m.kind = NodeKind::Page;
        m.word_count = Some(2_140);
        assert_eq!(m.size_summary_parts(), vec!["2,140 words", "9 min"]);
    }
}
