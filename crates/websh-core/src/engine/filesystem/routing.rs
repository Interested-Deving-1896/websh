use std::collections::BTreeMap;

use crate::domain::{NodeKind, RendererKind, VirtualPath};

use super::global_fs::GlobalFs;
use super::intent::RenderIntent;

const SHELL_ROUTE_PREFIX: &str = "/websh";

/// Browser request normalized into a filesystem-first input shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteRequest {
    pub url_path: String,
}

impl RouteRequest {
    pub fn new(url_path: impl Into<String>) -> Self {
        let raw = url_path.into();
        if raw.is_empty() {
            return Self {
                url_path: "/".to_string(),
            };
        }
        if raw.starts_with('/') {
            return Self {
                url_path: normalize_request_path(&raw),
            };
        }
        Self {
            url_path: normalize_request_path(&format!("/{}", raw)),
        }
    }
}

/// User-facing route surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RouteSurface {
    /// Canonical content route, e.g. `#/blog/hello.md`.
    #[default]
    Content,
    /// Shell route for a canonical cwd, e.g. `#/websh/blog`.
    Shell,
}

/// Broad resolution result prior to renderer-specific details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedKind {
    Directory,
    Bundle,
    Page,
    Document,
    App,
    Asset,
    Redirect,
}

/// Output of route resolution before content loading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteResolution {
    pub request_path: String,
    pub surface: RouteSurface,
    pub node_path: VirtualPath,
    pub kind: ResolvedKind,
    pub params: BTreeMap<String, String>,
}

/// Full route state consumed by the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteFrame {
    pub request: RouteRequest,
    pub resolution: RouteResolution,
    pub intent: RenderIntent,
}

impl RouteFrame {
    pub fn is_root(&self) -> bool {
        !self.is_file() && route_cwd(self).is_root()
    }

    pub fn is_home(&self) -> bool {
        route_cwd(self).is_root()
    }

    pub fn surface(&self) -> RouteSurface {
        self.resolution.surface
    }

    pub fn display_path(&self) -> String {
        let path = if self.is_file() {
            self.resolution.node_path.clone()
        } else {
            route_cwd(self)
        };
        display_path_for(&path)
    }

    pub fn is_file(&self) -> bool {
        !matches!(
            self.resolution.kind,
            ResolvedKind::Directory | ResolvedKind::App
        )
    }
}

/// Returns true if `req` is the synthetic `/new` mempool authoring route.
///
/// `RouteRequest::new` always normalizes to a leading `/`, so the practical
/// inputs are `/new`, `/new/`, and `/new/<rest>`. The trim defends against
/// the `new`-no-slash shape too, but that path doesn't currently arise.
pub fn is_new_request_path(req: &RouteRequest) -> bool {
    req.url_path.trim_matches('/') == "new"
}

pub fn request_path_for_canonical_path(path: &VirtualPath, surface: RouteSurface) -> String {
    match surface {
        RouteSurface::Content => {
            if path.is_root() {
                "/".to_string()
            } else {
                path.as_str().to_string()
            }
        }
        RouteSurface::Shell => surface_request_path(SHELL_ROUTE_PREFIX, path),
    }
}

pub fn parent_request_path(path: &str) -> String {
    let normalized = normalize_request_path(path);
    if normalized == "/" {
        return "/".to_string();
    }

    if let Some((surface, current)) = surface_target_from_request(&normalized) {
        return current
            .parent()
            .map(|parent| request_path_for_canonical_path(&parent, surface))
            .unwrap_or_else(|| request_path_for_canonical_path(&VirtualPath::root(), surface));
    }

    if let Ok(current) = VirtualPath::from_absolute(normalized.clone()) {
        return current
            .parent()
            .map(|parent| request_path_for_canonical_path(&parent, RouteSurface::Content))
            .unwrap_or_else(|| "/".to_string());
    }

    match normalized.rsplit_once('/') {
        Some(("", _)) | None => "/".to_string(),
        Some((parent, _)) => parent.to_string(),
    }
}

pub fn route_cwd(frame: &RouteFrame) -> VirtualPath {
    if let Some(cwd) = frame.resolution.params.get("cwd")
        && let Ok(path) = VirtualPath::from_absolute(cwd.clone())
    {
        return path;
    }

    match frame.resolution.kind {
        ResolvedKind::Directory => frame.resolution.node_path.clone(),
        _ => frame
            .resolution
            .node_path
            .parent()
            .unwrap_or_else(VirtualPath::root),
    }
}

pub fn display_path_for(path: &VirtualPath) -> String {
    if path.is_root() {
        return "~".to_string();
    }
    path.as_str().to_string()
}

pub fn canonicalize_user_path(cwd: &VirtualPath, raw: &str) -> Option<VirtualPath> {
    if raw.is_empty() || raw == "." {
        return Some(cwd.clone());
    }

    let input = if raw == "~" {
        "/".to_string()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        format!("/{}", rest)
    } else if raw.starts_with('/') {
        raw.to_string()
    } else if cwd.is_root() {
        format!("/{}", raw)
    } else {
        format!("{}/{}", cwd.as_str().trim_end_matches('/'), raw)
    };

    normalize_absolute_path(&input)
}

/// Resolve routes in priority order:
/// 1. reserved shell route
/// 2. bundle variant routes
/// 3. derived index
/// 4. convention fallback
pub fn resolve_route(fs: &GlobalFs, request: &RouteRequest) -> Option<RouteResolution> {
    let path = normalize_request_path(&request.url_path);

    if is_reserved_request_path(&path) {
        return resolve_reserved_route(fs, &path);
    }

    resolve_bundle_variant_route(fs, &path)
        .or_else(|| resolve_index_route(fs, &path))
        .or_else(|| resolve_convention_route(fs, &path))
}

pub fn normalize_request_path(path: &str) -> String {
    if path == "/" {
        return "/".to_string();
    }

    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn resolve_reserved_route(fs: &GlobalFs, request_path: &str) -> Option<RouteResolution> {
    let (surface, cwd) = surface_target_from_request(request_path)?;
    if !fs.is_directory(&cwd) {
        return None;
    }

    let mut params = BTreeMap::new();
    params.insert("cwd".to_string(), cwd.to_string());

    Some(RouteResolution {
        request_path: request_path.to_string(),
        surface,
        node_path: cwd,
        kind: match surface {
            RouteSurface::Shell => ResolvedKind::App,
            RouteSurface::Content => return None,
        },
        params,
    })
}

fn is_reserved_request_path(request_path: &str) -> bool {
    request_path == SHELL_ROUTE_PREFIX
        || request_path.starts_with(&format!("{SHELL_ROUTE_PREFIX}/"))
}

fn resolve_index_route(fs: &GlobalFs, request_path: &str) -> Option<RouteResolution> {
    let entry = fs.route_entry(request_path)?;
    let node_path = VirtualPath::from_absolute(entry.node_path.clone()).ok()?;
    if !fs.exists(&node_path) {
        return None;
    }

    Some(RouteResolution {
        request_path: request_path.to_string(),
        surface: RouteSurface::Content,
        node_path: node_path.clone(),
        kind: resolved_kind_from_index(
            fs,
            &node_path,
            entry.kind.as_ref(),
            entry.renderer.as_ref(),
        ),
        params: BTreeMap::new(),
    })
}

fn resolve_convention_route(fs: &GlobalFs, request_path: &str) -> Option<RouteResolution> {
    let rel = request_path.trim_start_matches('/');

    for candidate in route_candidates(rel) {
        if let Some(kind) = classify_candidate(fs, &candidate) {
            return Some(RouteResolution {
                request_path: request_path.to_string(),
                surface: RouteSurface::Content,
                node_path: candidate,
                kind,
                params: BTreeMap::new(),
            });
        }
    }

    None
}

fn resolved_kind_from_index(
    fs: &GlobalFs,
    node_path: &VirtualPath,
    kind: Option<&NodeKind>,
    renderer: Option<&RendererKind>,
) -> ResolvedKind {
    if let Some(kind) = kind {
        return match kind {
            NodeKind::Page => ResolvedKind::Page,
            NodeKind::Document => ResolvedKind::Document,
            NodeKind::App => ResolvedKind::App,
            NodeKind::Asset => ResolvedKind::Asset,
            NodeKind::Redirect => ResolvedKind::Redirect,
            NodeKind::Data => classify_candidate(fs, node_path).unwrap_or(ResolvedKind::Document),
            NodeKind::Directory => ResolvedKind::Directory,
            NodeKind::Bundle => ResolvedKind::Bundle,
        };
    }

    if let Some(renderer) = renderer {
        return match renderer {
            RendererKind::HtmlPage | RendererKind::MarkdownPage => ResolvedKind::Page,
            RendererKind::DirectoryListing => ResolvedKind::Directory,
            RendererKind::TerminalApp => ResolvedKind::App,
            RendererKind::Image => ResolvedKind::Asset,
            RendererKind::Pdf | RendererKind::DocumentReader | RendererKind::RawText => {
                ResolvedKind::Document
            }
            RendererKind::Redirect => ResolvedKind::Redirect,
        };
    }

    classify_candidate(fs, node_path).unwrap_or(ResolvedKind::Document)
}

fn classify_candidate(fs: &GlobalFs, candidate: &VirtualPath) -> Option<ResolvedKind> {
    let entry = fs.get_entry(candidate)?;
    if entry.is_directory() {
        if entry.meta().is_bundle() {
            return Some(ResolvedKind::Bundle);
        }
        return Some(ResolvedKind::Directory);
    }

    let ext = candidate
        .file_name()
        .and_then(|name| name.rsplit_once('.').map(|(_, ext)| ext))
        .unwrap_or("");

    Some(match ext {
        "app" => ResolvedKind::App,
        "md" | "html" => ResolvedKind::Page,
        "link" => ResolvedKind::Redirect,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => ResolvedKind::Asset,
        "pdf" => ResolvedKind::Document,
        _ => ResolvedKind::Document,
    })
}

fn resolve_bundle_variant_route(fs: &GlobalFs, request_path: &str) -> Option<RouteResolution> {
    let requested = VirtualPath::from_absolute(request_path.to_string()).ok()?;
    if fs
        .node_metadata(&requested)
        .is_some_and(|metadata| metadata.is_bundle())
    {
        return Some(RouteResolution {
            request_path: request_path.to_string(),
            surface: RouteSurface::Content,
            node_path: requested,
            kind: ResolvedKind::Bundle,
            params: BTreeMap::new(),
        });
    }

    let variant_id = requested.file_name()?.to_string();
    let bundle_path = requested.parent()?;
    let bundle = fs.node_metadata(&bundle_path)?;
    if !bundle.is_bundle() {
        return None;
    }
    let bundle_meta = bundle.bundle.as_ref()?;
    if !bundle_meta
        .variants
        .iter()
        .any(|variant| variant.id == variant_id)
    {
        return None;
    }

    let mut params = BTreeMap::new();
    params.insert("variant".to_string(), variant_id);

    Some(RouteResolution {
        request_path: request_path.to_string(),
        surface: RouteSurface::Content,
        node_path: bundle_path,
        kind: ResolvedKind::Bundle,
        params,
    })
}

fn route_candidates(relative_request: &str) -> Vec<VirtualPath> {
    let mut out = Vec::new();
    let trimmed = relative_request.trim_matches('/');
    let site_root = VirtualPath::root();

    if trimmed.is_empty() {
        for suffix in [
            "index.page.html",
            "index.page.md",
            "index.html",
            "index.md",
            "index.app",
            "index.link",
        ] {
            out.push(site_root.join(suffix));
        }
        return out;
    }

    for suffix in [
        format!("{trimmed}.page.html"),
        format!("{trimmed}.page.md"),
        format!("{trimmed}.html"),
        format!("{trimmed}.md"),
        format!("{trimmed}.app"),
        format!("{trimmed}.link"),
        format!("{trimmed}/index.page.html"),
        format!("{trimmed}/index.page.md"),
        format!("{trimmed}/index.html"),
        format!("{trimmed}/index.md"),
        format!("{trimmed}/index.link"),
    ] {
        out.push(site_root.join(&suffix));
    }

    out.push(site_root.join(trimmed));
    out
}

fn surface_request_path(prefix: &str, path: &VirtualPath) -> String {
    if path.is_root() {
        prefix.to_string()
    } else {
        format!("{}/{}", prefix, path.as_str().trim_start_matches('/'))
    }
}

fn surface_target_from_request(request_path: &str) -> Option<(RouteSurface, VirtualPath)> {
    if request_path == SHELL_ROUTE_PREFIX {
        return Some((RouteSurface::Shell, VirtualPath::root()));
    }
    if let Some(rest) = request_path.strip_prefix(&format!("{SHELL_ROUTE_PREFIX}/")) {
        return normalize_absolute_path(&format!("/{rest}"))
            .map(|path| (RouteSurface::Shell, path));
    }
    None
}

fn normalize_absolute_path(path: &str) -> Option<VirtualPath> {
    let mut parts = Vec::new();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        match segment {
            "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(segment),
        }
    }

    let normalized = if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    };
    VirtualPath::from_absolute(normalized).ok()
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        BundleMetadata, BundleVariant, EntryExtensions, Fields, NodeMetadata, RouteIndexEntry,
        SCHEMA_VERSION,
    };
    use crate::ports::{ScannedDirectory, ScannedFile, ScannedSubtree};

    use super::*;

    fn make_meta(kind: NodeKind) -> NodeMetadata {
        NodeMetadata {
            schema: SCHEMA_VERSION,
            kind,
            bundle: None,
            authored: Fields::default(),
            derived: Fields::default(),
        }
    }

    fn make_dir_meta(name: &str) -> NodeMetadata {
        NodeMetadata {
            schema: SCHEMA_VERSION,
            kind: NodeKind::Directory,
            bundle: None,
            authored: Fields {
                title: Some(name.to_string()),
                ..Fields::default()
            },
            derived: Fields::default(),
        }
    }

    fn site(files: &[&str], directories: &[&str]) -> GlobalFs {
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
        let mut snapshot = ScannedSubtree {
            files: vec![
                ScannedFile {
                    path: "writing/foo/en.md".to_string(),
                    meta: make_meta(NodeKind::Page),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "writing/foo/ko.md".to_string(),
                    meta: make_meta(NodeKind::Page),
                    extensions: EntryExtensions::default(),
                },
            ],
            directories: vec![
                ScannedDirectory {
                    path: "writing".to_string(),
                    meta: make_dir_meta("writing"),
                },
                ScannedDirectory {
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
                        authored: Fields {
                            title: Some("Foo".to_string()),
                            ..Fields::default()
                        },
                        derived: Fields {
                            kind: Some(NodeKind::Bundle),
                            ..Fields::default()
                        },
                    },
                },
            ],
        };
        snapshot.files.sort_by(|a, b| a.path.cmp(&b.path));
        let mut global = GlobalFs::empty();
        global
            .mount_scanned_subtree(VirtualPath::root(), &snapshot)
            .unwrap();
        global
    }

    #[test]
    fn route_request_normalizes_leading_and_trailing_slashes() {
        assert_eq!(RouteRequest::new("").url_path, "/");
        assert_eq!(RouteRequest::new("about").url_path, "/about");
        assert_eq!(RouteRequest::new("/about/").url_path, "/about");
    }

    #[test]
    fn resolves_shell_route_from_reserved_surface() {
        let fs = site(&["blog/post.md"], &["blog"]);
        let resolved = resolve_route(&fs, &RouteRequest::new("/websh")).unwrap();

        assert_eq!(resolved.kind, ResolvedKind::App);
        assert_eq!(resolved.surface, RouteSurface::Shell);
        assert_eq!(resolved.node_path.as_str(), "/");
        assert_eq!(resolved.params.get("cwd").map(String::as_str), Some("/"));
    }

    #[test]
    fn resolves_nested_shell_route_to_canonical_cwd() {
        let fs = site(&["blog/post.md"], &["blog"]);
        let resolved = resolve_route(&fs, &RouteRequest::new("/websh/blog")).unwrap();

        assert_eq!(resolved.kind, ResolvedKind::App);
        assert_eq!(resolved.surface, RouteSurface::Shell);
        assert_eq!(resolved.node_path.as_str(), "/blog");
        assert_eq!(
            resolved.params.get("cwd").map(String::as_str),
            Some("/blog")
        );
    }

    #[test]
    fn explorer_is_no_longer_a_reserved_route_prefix() {
        let fs = site(&["explorer/foo.md"], &["explorer"]);
        let resolved = resolve_route(&fs, &RouteRequest::new("/explorer")).unwrap();
        assert_eq!(resolved.kind, ResolvedKind::Directory);
        assert_eq!(resolved.surface, RouteSurface::Content);
        assert_eq!(resolved.node_path.as_str(), "/explorer");

        let resolved = resolve_route(&fs, &RouteRequest::new("/explorer/foo")).unwrap();
        assert_eq!(resolved.kind, ResolvedKind::Page);
        assert_eq!(resolved.surface, RouteSurface::Content);
        assert_eq!(resolved.node_path.as_str(), "/explorer/foo.md");
    }

    #[test]
    fn fs_namespace_is_not_a_route() {
        let fs = site(&["blog/post.md"], &["blog"]);
        assert!(resolve_route(&fs, &RouteRequest::new("/fs/site/blog/post.md")).is_none());
    }

    #[test]
    fn reserved_shell_route_wins_over_content_node() {
        let fs = site(&["shell/index.md"], &["shell"]);
        let resolved = resolve_route(&fs, &RouteRequest::new("/websh")).unwrap();

        assert_eq!(resolved.kind, ResolvedKind::App);
        assert_eq!(resolved.surface, RouteSurface::Shell);
        assert_eq!(resolved.node_path.as_str(), "/");
    }

    #[test]
    fn resolves_route_from_derived_index() {
        let mut fs = site(&["about.md"], &[]);
        fs.replace_route_index([RouteIndexEntry {
            route: "/company".to_string(),
            node_path: "/about.md".to_string(),
            kind: Some(NodeKind::Page),
            renderer: Some(RendererKind::MarkdownPage),
        }]);

        let resolved = resolve_route(&fs, &RouteRequest::new("/company")).unwrap();
        assert_eq!(resolved.surface, RouteSurface::Content);
        assert_eq!(resolved.node_path.as_str(), "/about.md");
        assert_eq!(resolved.kind, ResolvedKind::Page);
    }

    #[test]
    fn resolves_root_to_index_page_via_convention_fallback() {
        let fs = site(&["index.page.md"], &[]);
        let resolved = resolve_route(&fs, &RouteRequest::new("/")).unwrap();

        assert_eq!(resolved.kind, ResolvedKind::Page);
        assert_eq!(resolved.node_path.as_str(), "/index.page.md");
    }

    #[test]
    fn resolves_direct_canonical_content_file() {
        let fs = site(&["about.md", "db/fresh.md"], &["db"]);
        let resolved = resolve_route(&fs, &RouteRequest::new("/db/fresh.md")).unwrap();

        assert_eq!(resolved.kind, ResolvedKind::Page);
        assert_eq!(resolved.surface, RouteSurface::Content);
        assert_eq!(resolved.node_path.as_str(), "/db/fresh.md");
    }

    #[test]
    fn resolves_bundle_root_to_bundle_directory() {
        let fs = bundle_site();
        let resolved = resolve_route(&fs, &RouteRequest::new("/writing/foo")).unwrap();

        assert_eq!(resolved.kind, ResolvedKind::Bundle);
        assert_eq!(resolved.node_path.as_str(), "/writing/foo");
        assert_eq!(resolved.params.get("variant"), None);
    }

    #[test]
    fn bundle_root_route_wins_over_derived_index_alias() {
        let mut fs = bundle_site();
        fs.replace_route_index([RouteIndexEntry {
            route: "/writing/foo".to_string(),
            node_path: "/writing/foo/en.md".to_string(),
            kind: Some(NodeKind::Page),
            renderer: Some(RendererKind::MarkdownPage),
        }]);

        let resolved = resolve_route(&fs, &RouteRequest::new("/writing/foo")).unwrap();
        assert_eq!(resolved.kind, ResolvedKind::Bundle);
        assert_eq!(resolved.node_path.as_str(), "/writing/foo");
        assert_eq!(resolved.params.get("variant"), None);
    }

    #[test]
    fn bundle_variant_route_wins_over_markdown_convention() {
        let fs = bundle_site();
        let resolved = resolve_route(&fs, &RouteRequest::new("/writing/foo/ko")).unwrap();

        assert_eq!(resolved.kind, ResolvedKind::Bundle);
        assert_eq!(resolved.node_path.as_str(), "/writing/foo");
        assert_eq!(
            resolved.params.get("variant").map(String::as_str),
            Some("ko")
        );
    }

    #[test]
    fn bundle_variant_route_wins_over_derived_index_alias() {
        let mut fs = bundle_site();
        fs.replace_route_index([RouteIndexEntry {
            route: "/writing/foo/ko".to_string(),
            node_path: "/writing/foo/ko.md".to_string(),
            kind: Some(NodeKind::Page),
            renderer: Some(RendererKind::MarkdownPage),
        }]);

        let resolved = resolve_route(&fs, &RouteRequest::new("/writing/foo/ko")).unwrap();
        assert_eq!(resolved.kind, ResolvedKind::Bundle);
        assert_eq!(resolved.node_path.as_str(), "/writing/foo");
        assert_eq!(
            resolved.params.get("variant").map(String::as_str),
            Some("ko")
        );
    }

    #[test]
    fn direct_bundle_variant_file_route_remains_a_file_route() {
        let fs = bundle_site();
        let resolved = resolve_route(&fs, &RouteRequest::new("/writing/foo/ko.md")).unwrap();

        assert_eq!(resolved.kind, ResolvedKind::Page);
        assert_eq!(resolved.node_path.as_str(), "/writing/foo/ko.md");
    }

    #[test]
    fn display_path_uses_home_alias_for_root() {
        assert_eq!(
            display_path_for(&VirtualPath::from_absolute("/blog").unwrap()),
            "/blog"
        );
        assert_eq!(display_path_for(&VirtualPath::root()), "~");
    }

    #[test]
    fn canonicalize_user_path_understands_aliases_and_parent_segments() {
        let cwd = VirtualPath::from_absolute("/blog").unwrap();
        assert_eq!(
            canonicalize_user_path(&cwd, "../about.md")
                .unwrap()
                .as_str(),
            "/about.md"
        );
        assert_eq!(
            canonicalize_user_path(&cwd, "~/posts").unwrap().as_str(),
            "/posts"
        );
        assert_eq!(canonicalize_user_path(&cwd, "/db").unwrap().as_str(), "/db");
    }

    #[test]
    fn request_paths_are_surface_aware() {
        let path = VirtualPath::from_absolute("/blog/hello.md").unwrap();
        assert_eq!(
            request_path_for_canonical_path(&path, RouteSurface::Content),
            "/blog/hello.md"
        );
        assert_eq!(
            request_path_for_canonical_path(&path, RouteSurface::Shell),
            "/websh/blog/hello.md"
        );
    }

    #[test]
    fn is_new_request_path_matches_canonical_new_route() {
        assert!(is_new_request_path(&RouteRequest::new("/new")));
        assert!(is_new_request_path(&RouteRequest::new("/new/")));
        assert!(is_new_request_path(&RouteRequest::new("new")));
    }

    #[test]
    fn is_new_request_path_rejects_non_matches() {
        assert!(!is_new_request_path(&RouteRequest::new("/news")));
        assert!(!is_new_request_path(&RouteRequest::new("/new/foo")));
        assert!(!is_new_request_path(&RouteRequest::new("/")));
        assert!(!is_new_request_path(&RouteRequest::new("/edit")));
        assert!(!is_new_request_path(&RouteRequest::new("/ledger")));
    }
}
