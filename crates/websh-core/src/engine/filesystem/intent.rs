use crate::domain::{BundleVariant, FileType, VirtualPath};
use crate::support::{media_type_for_path, normalize_locale_tag};

use super::global_fs::GlobalFs;
use super::routing::{ResolvedKind, RouteResolution};

/// Renderer-neutral output produced by the engine and consumed by the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderIntent {
    DirectoryListing {
        node_path: VirtualPath,
    },
    TerminalApp {
        node_path: VirtualPath,
    },
    HtmlContent {
        node_path: VirtualPath,
    },
    MarkdownContent {
        node_path: VirtualPath,
    },
    PlainContent {
        node_path: VirtualPath,
    },
    Asset {
        node_path: VirtualPath,
        media_type: String,
    },
    BundleVariant {
        bundle_path: VirtualPath,
        variant_id: String,
        variant_path: VirtualPath,
    },
    Redirect {
        node_path: VirtualPath,
    },
}

pub fn build_render_intent(fs: &GlobalFs, resolution: &RouteResolution) -> Option<RenderIntent> {
    build_render_intent_with_preferred_locale(fs, resolution, None)
}

pub fn build_render_intent_with_preferred_locale(
    fs: &GlobalFs,
    resolution: &RouteResolution,
    preferred_locale: Option<&str>,
) -> Option<RenderIntent> {
    let path = &resolution.node_path;

    Some(match resolution.kind {
        ResolvedKind::Directory => RenderIntent::DirectoryListing {
            node_path: path.clone(),
        },
        ResolvedKind::App => RenderIntent::TerminalApp {
            node_path: path.clone(),
        },
        ResolvedKind::Redirect => RenderIntent::Redirect {
            node_path: path.clone(),
        },
        ResolvedKind::Asset => RenderIntent::Asset {
            node_path: path.clone(),
            media_type: media_type_for_path(path.as_str()).to_string(),
        },
        ResolvedKind::Bundle => bundle_intent_for_node(fs, resolution, preferred_locale)?,
        ResolvedKind::Page | ResolvedKind::Document => content_intent_for_node(path),
    })
}

fn bundle_intent_for_node(
    fs: &GlobalFs,
    resolution: &RouteResolution,
    preferred_locale: Option<&str>,
) -> Option<RenderIntent> {
    let bundle_path = &resolution.node_path;
    let bundle_meta = fs.node_metadata(bundle_path)?.bundle.as_ref()?;
    let variant_id = resolution
        .params
        .get("variant")
        .map(String::as_str)
        .or_else(|| preferred_variant_id(&bundle_meta.variants, preferred_locale))
        .unwrap_or(bundle_meta.default_variant.as_str());
    let variant = bundle_meta
        .variants
        .iter()
        .find(|variant| variant.id == variant_id)?;
    let variant_path = bundle_child_path(bundle_path, &variant.path)?;
    let entry = fs.get_entry(&variant_path)?;
    if entry.is_directory() {
        return None;
    }

    Some(RenderIntent::BundleVariant {
        bundle_path: bundle_path.clone(),
        variant_id: variant_id.to_string(),
        variant_path,
    })
}

fn preferred_variant_id<'a>(
    variants: &'a [BundleVariant],
    preferred_locale: Option<&str>,
) -> Option<&'a str> {
    let preferred = normalize_locale_tag(preferred_locale?)?;
    variants
        .iter()
        .find(|variant| {
            variant
                .locale
                .as_deref()
                .and_then(normalize_locale_tag)
                .as_deref()
                == Some(preferred.as_str())
        })
        .map(|variant| variant.id.as_str())
}

fn bundle_child_path(bundle_path: &VirtualPath, rel_path: &str) -> Option<VirtualPath> {
    if rel_path.is_empty()
        || rel_path.starts_with('/')
        || rel_path.contains('\\')
        || rel_path.chars().any(char::is_control)
    {
        return None;
    }
    if rel_path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }
    let path = bundle_path.join(rel_path);
    path.starts_with(bundle_path).then_some(path)
}

fn content_intent_for_node(path: &VirtualPath) -> RenderIntent {
    match FileType::from_path(path.as_str()) {
        FileType::Html => RenderIntent::HtmlContent {
            node_path: path.clone(),
        },
        FileType::Markdown => RenderIntent::MarkdownContent {
            node_path: path.clone(),
        },
        FileType::Pdf | FileType::Image => RenderIntent::Asset {
            node_path: path.clone(),
            media_type: media_type_for_path(path.as_str()).to_string(),
        },
        FileType::Link => RenderIntent::Redirect {
            node_path: path.clone(),
        },
        FileType::Unknown => RenderIntent::PlainContent {
            node_path: path.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        BundleMetadata, BundleVariant, EntryExtensions, Fields, NodeKind, NodeMetadata,
        SCHEMA_VERSION, VirtualPath,
    };
    use crate::engine::filesystem::{GlobalFs, RouteRequest, resolve_route};
    use crate::ports::{ScannedDirectory, ScannedFile, ScannedSubtree};

    use super::*;

    fn site(files: &[&str], directories: &[&str]) -> GlobalFs {
        let make_meta = |kind: NodeKind| NodeMetadata {
            schema: SCHEMA_VERSION,
            kind,
            bundle: None,
            authored: Fields::default(),
            derived: Fields::default(),
        };
        let make_dir_meta = |name: &str| NodeMetadata {
            schema: SCHEMA_VERSION,
            kind: NodeKind::Directory,
            bundle: None,
            authored: Fields {
                title: Some(name.to_string()),
                ..Fields::default()
            },
            derived: Fields::default(),
        };

        let snapshot = ScannedSubtree {
            files: files
                .iter()
                .map(|path| ScannedFile {
                    path: (*path).to_string(),
                    meta: make_meta(NodeKind::Page),
                    extensions: EntryExtensions::default(),
                })
                .collect(),
            directories: directories
                .iter()
                .map(|path| ScannedDirectory {
                    path: (*path).to_string(),
                    meta: make_dir_meta(path.rsplit('/').next().unwrap_or(path)),
                })
                .collect(),
        };

        let mut global = GlobalFs::empty();
        global
            .mount_scanned_subtree(VirtualPath::root(), &snapshot)
            .unwrap();
        global
    }

    fn bundle_site() -> GlobalFs {
        let snapshot = ScannedSubtree {
            files: vec![
                ScannedFile {
                    path: "writing/foo/en.md".to_string(),
                    meta: NodeMetadata {
                        schema: SCHEMA_VERSION,
                        kind: NodeKind::Page,
                        bundle: None,
                        authored: Fields::default(),
                        derived: Fields::default(),
                    },
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "writing/foo/ko.md".to_string(),
                    meta: NodeMetadata {
                        schema: SCHEMA_VERSION,
                        kind: NodeKind::Page,
                        bundle: None,
                        authored: Fields::default(),
                        derived: Fields::default(),
                    },
                    extensions: EntryExtensions::default(),
                },
            ],
            directories: vec![ScannedDirectory {
                path: "writing/foo".to_string(),
                meta: NodeMetadata {
                    schema: SCHEMA_VERSION,
                    kind: NodeKind::Bundle,
                    bundle: Some(BundleMetadata {
                        default_variant: "en".to_string(),
                        variants: vec![
                            BundleVariant {
                                id: "en".to_string(),
                                path: "en.md".to_string(),
                                label: "English".to_string(),
                                locale: Some("en".to_string()),
                                media_type: None,
                            },
                            BundleVariant {
                                id: "ko".to_string(),
                                path: "ko.md".to_string(),
                                label: "Korean".to_string(),
                                locale: Some("ko".to_string()),
                                media_type: None,
                            },
                        ],
                    }),
                    authored: Fields::default(),
                    derived: Fields {
                        kind: Some(NodeKind::Bundle),
                        ..Fields::default()
                    },
                },
            }],
        };
        let mut global = GlobalFs::empty();
        global
            .mount_scanned_subtree(VirtualPath::root(), &snapshot)
            .unwrap();
        global
    }

    #[test]
    fn builds_html_content_intent_for_root_index() {
        let fs = site(&["index.html"], &[]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::HtmlContent {
                node_path: VirtualPath::from_absolute("/index.html").unwrap(),
            }
        );
    }

    #[test]
    fn builds_markdown_content_intent_for_top_level_page() {
        let fs = site(&["about.md"], &[]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/about")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::MarkdownContent {
                node_path: VirtualPath::from_absolute("/about.md").unwrap(),
            }
        );
    }

    #[test]
    fn builds_terminal_app_intent() {
        let fs = site(&[], &[]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/websh")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::TerminalApp {
                node_path: VirtualPath::root(),
            }
        );
    }

    #[test]
    fn builds_directory_listing_intent() {
        let fs = site(&["blog/hello.md"], &["blog"]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/blog")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::DirectoryListing {
                node_path: VirtualPath::from_absolute("/blog").unwrap(),
            }
        );
    }

    #[test]
    fn builds_redirect_intent_with_source_node_path() {
        let fs = site(&["jump.link"], &[]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/jump")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::Redirect {
                node_path: VirtualPath::from_absolute("/jump.link").unwrap(),
            }
        );
    }

    #[test]
    fn builds_html_content_intent_for_html_document() {
        let fs = site(&["blog/hello.html"], &["blog"]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/blog/hello.html")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::HtmlContent {
                node_path: VirtualPath::from_absolute("/blog/hello.html").unwrap(),
            }
        );
    }

    #[test]
    fn builds_markdown_content_intent_for_md_document() {
        let fs = site(&["blog/hello.md"], &["blog"]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/blog/hello.md")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::MarkdownContent {
                node_path: VirtualPath::from_absolute("/blog/hello.md").unwrap(),
            }
        );
    }

    #[test]
    fn builds_asset_intent_for_pdf_document() {
        let fs = site(&["papers/draft.pdf"], &["papers"]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/papers/draft.pdf")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::Asset {
                node_path: VirtualPath::from_absolute("/papers/draft.pdf").unwrap(),
                media_type: "application/pdf".to_string(),
            }
        );
    }

    #[test]
    fn builds_asset_intent_for_image_document() {
        let fs = site(&["photos/cover.png"], &["photos"]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/photos/cover.png")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::Asset {
                node_path: VirtualPath::from_absolute("/photos/cover.png").unwrap(),
                media_type: "image/png".to_string(),
            }
        );
    }

    #[test]
    fn builds_redirect_intent_for_link_document() {
        let fs = site(&["links/x.link"], &["links"]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/links/x.link")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::Redirect {
                node_path: VirtualPath::from_absolute("/links/x.link").unwrap(),
            }
        );
    }

    #[test]
    fn builds_plain_content_intent_for_unknown_document() {
        let fs = site(&["notes/x.txt"], &["notes"]);
        let resolution = resolve_route(&fs, &RouteRequest::new("/notes/x.txt")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::PlainContent {
                node_path: VirtualPath::from_absolute("/notes/x.txt").unwrap(),
            }
        );
    }

    #[test]
    fn builds_default_bundle_variant_intent() {
        let fs = bundle_site();
        let resolution = resolve_route(&fs, &RouteRequest::new("/writing/foo")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::BundleVariant {
                bundle_path: VirtualPath::from_absolute("/writing/foo").unwrap(),
                variant_id: "en".to_string(),
                variant_path: VirtualPath::from_absolute("/writing/foo/en.md").unwrap(),
            }
        );
    }

    #[test]
    fn builds_preferred_locale_bundle_variant_intent_for_default_route() {
        let fs = bundle_site();
        let resolution = resolve_route(&fs, &RouteRequest::new("/writing/foo")).unwrap();
        let intent =
            build_render_intent_with_preferred_locale(&fs, &resolution, Some("ko-KR")).unwrap();

        assert_eq!(
            intent,
            RenderIntent::BundleVariant {
                bundle_path: VirtualPath::from_absolute("/writing/foo").unwrap(),
                variant_id: "ko".to_string(),
                variant_path: VirtualPath::from_absolute("/writing/foo/ko.md").unwrap(),
            }
        );
    }

    #[test]
    fn explicit_bundle_variant_ignores_preferred_locale() {
        let fs = bundle_site();
        let resolution = resolve_route(&fs, &RouteRequest::new("/writing/foo/en")).unwrap();
        let intent =
            build_render_intent_with_preferred_locale(&fs, &resolution, Some("ko")).unwrap();

        assert_eq!(
            intent,
            RenderIntent::BundleVariant {
                bundle_path: VirtualPath::from_absolute("/writing/foo").unwrap(),
                variant_id: "en".to_string(),
                variant_path: VirtualPath::from_absolute("/writing/foo/en.md").unwrap(),
            }
        );
    }

    #[test]
    fn preferred_locale_falls_back_to_default_when_unmatched() {
        let fs = bundle_site();
        let resolution = resolve_route(&fs, &RouteRequest::new("/writing/foo")).unwrap();
        let intent =
            build_render_intent_with_preferred_locale(&fs, &resolution, Some("fr-FR")).unwrap();

        assert_eq!(
            intent,
            RenderIntent::BundleVariant {
                bundle_path: VirtualPath::from_absolute("/writing/foo").unwrap(),
                variant_id: "en".to_string(),
                variant_path: VirtualPath::from_absolute("/writing/foo/en.md").unwrap(),
            }
        );
    }

    #[test]
    fn builds_explicit_bundle_variant_intent() {
        let fs = bundle_site();
        let resolution = resolve_route(&fs, &RouteRequest::new("/writing/foo/ko")).unwrap();
        let intent = build_render_intent(&fs, &resolution).unwrap();

        assert_eq!(
            intent,
            RenderIntent::BundleVariant {
                bundle_path: VirtualPath::from_absolute("/writing/foo").unwrap(),
                variant_id: "ko".to_string(),
                variant_path: VirtualPath::from_absolute("/writing/foo/ko.md").unwrap(),
            }
        );
    }
}
