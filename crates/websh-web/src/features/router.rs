//! Application router component.
//!
//! Handles URL-based routing with hash history for static hosting.
//! Uses native hashchange events instead of leptos_router for true hash routing.
//!
//! # Architecture
//!
//! - **URL hash is the source of truth**: Navigation state is derived from `#/path`
//! - **Shell never re-renders on navigation**: AppLayout is always mounted
//! - **Reader handles content files**: File routes use a stable page shell
//! - **hashchange events**: Browser back/forward buttons work automatically

use std::collections::BTreeMap;

use leptos::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::Closure;

#[cfg(target_arch = "wasm32")]
use crate::app::AppContext;
use crate::features::home::HomePage;
use crate::features::ledger::LedgerPage;
use crate::features::ledger::routes::{LEDGER_ROUTE, is_ledger_filter_route_segment};
use crate::features::reader::{Reader, ReaderFrame};
use crate::features::terminal::Shell;

/// URL patterns that bypass the engine and produce a synthetic [`RouteFrame`].
///
/// Each variant corresponds to a reserved URL prefix (or full path) the
/// router handles directly. The engine never resolves these — it does not
/// know about UI-level concerns like compose mode or ledger filter views.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuiltinRoute {
    /// `/` — homepage.
    Home,
    /// `/ledger` and `/<category>` — ledger filter views.
    LedgerFilter,
    /// `/new` — mempool compose flow.
    NewCompose,
}

impl BuiltinRoute {
    /// Classify a request against the reserved URL list. Returns `None`
    /// when the request is to be routed through the engine.
    pub fn detect(request: &RouteRequest) -> Option<Self> {
        if request.url_path == "/" {
            return Some(Self::Home);
        }
        if is_ledger_filter_route_segment(request.url_path.trim_matches('/')) {
            return Some(Self::LedgerFilter);
        }
        if is_new_request_path(request) {
            return Some(Self::NewCompose);
        }
        None
    }
}
use crate::platform::dom::{current_route_request, focus_terminal_input};
use websh_core::domain::VirtualPath;
#[cfg(target_arch = "wasm32")]
use websh_core::filesystem::FsEngine;
use websh_core::filesystem::{
    RenderIntent, ResolvedKind, RouteFrame, RouteRequest, RouteResolution, RouteSurface,
    build_render_intent_with_preferred_locale, is_new_request_path,
};

/// Main application router.
///
/// Sets up hash-based routing with the following structure:
/// - `/` and `#/` → built-in homepage
/// - `#/ledger` → merged content ledger
/// - `#/websh/*path` → shell surface at canonical cwd
/// - other `#/*` paths → content route resolution against `/`
#[component]
pub fn RouterView() -> impl IntoView {
    #[cfg(target_arch = "wasm32")]
    let ctx = use_context::<AppContext>().expect("AppContext must be provided");

    // Raw request from URL hash (updated on hashchange).
    let _raw_request = RwSignal::new(current_route_request());

    // Set up hashchange event listener (runs once on mount).
    #[cfg(target_arch = "wasm32")]
    {
        use crate::platform::wasm_cleanup::WasmCleanup;
        use leptos::prelude::on_cleanup;
        use wasm_bindgen::JsCast;
        let closure = Closure::wrap(Box::new(move || {
            _raw_request.set(current_route_request());
        }) as Box<dyn Fn()>);

        if let Some(window) = web_sys::window() {
            let _ = window
                .add_event_listener_with_callback("hashchange", closure.as_ref().unchecked_ref());

            let cleanup = WasmCleanup(closure);
            on_cleanup(move || {
                let _ =
                    window.remove_event_listener_with_callback("hashchange", cleanup.js_function());
            });
        }
    }

    // Resolved route frame: re-runs whenever the hash changes OR fs loads/changes.
    #[cfg(target_arch = "wasm32")]
    let route = Memo::new(move |_| {
        let request = _raw_request.get();
        let fs = if route_request_needs_system_fs(&request) {
            ctx.system_global_fs.get()
        } else {
            ctx.view_global_fs.get()
        };
        let resolution = fs.resolve_route(&request)?;
        let preferred_locale = ctx
            .runtime_state
            .with(|state| state.env.get(crate::config::LANG_ENV_KEY).cloned());
        let intent = build_render_intent_with_preferred_locale(
            &fs,
            &resolution,
            preferred_locale.as_deref(),
        )?;
        Some(RouteFrame {
            request,
            resolution,
            intent,
        })
    });
    #[cfg(not(target_arch = "wasm32"))]
    let route = Memo::new(move |_| None::<RouteFrame>);

    install_terminal_focus_effect(_raw_request, route);

    view! {
        {move || {
            let request = _raw_request.get();
            match BuiltinRoute::detect(&request) {
                Some(BuiltinRoute::Home) => view! {
                    <HomePage route=Memo::new(move |_| {
                        route
                            .get()
                            .unwrap_or_else(|| home_frame(_raw_request.get()))
                    }) />
                }
                .into_any(),
                Some(BuiltinRoute::LedgerFilter) => view! {
                    <LedgerPage route=Memo::new(move |_| ledger_filter_frame(_raw_request.get())) />
                }
                .into_any(),
                Some(BuiltinRoute::NewCompose) => {
                    let reader_frame = ReaderFrame::try_from(new_compose_frame())
                        .expect("compose route always produces a Reader-bound intent");
                    view! {
                        <Reader frame=Memo::new(move |_| reader_frame.clone()) />
                    }
                    .into_any()
                }
                None => match route.get() {
                    Some(frame) => match frame.intent {
                        RenderIntent::TerminalApp { .. } => {
                            view! { <Shell route=static_route_memo(frame.clone()) /> }.into_any()
                        }
                        RenderIntent::DirectoryListing { .. } => {
                            view! { <LedgerPage route=static_route_memo(frame.clone()) /> }.into_any()
                        }
                        RenderIntent::HtmlContent { .. }
                        | RenderIntent::MarkdownContent { .. }
                        | RenderIntent::PlainContent { .. }
                        | RenderIntent::Asset { .. }
                        | RenderIntent::BundleVariant { .. }
                        | RenderIntent::Redirect { .. } => {
                            let reader_frame = ReaderFrame::try_from(frame)
                                .expect("non-surface RenderIntent variants convert to ReaderFrame");
                            view! {
                                <Reader frame=Memo::new(move |_| reader_frame.clone()) />
                            }
                            .into_any()
                        }
                    },
                    None => view! { <NotFound /> }.into_any(),
                }
            }
        }}
    }
}

fn new_compose_frame() -> RouteFrame {
    let request = RouteRequest::new("/new");
    let node_path = VirtualPath::root();
    RouteFrame {
        request: request.clone(),
        resolution: RouteResolution {
            request_path: request.url_path,
            surface: RouteSurface::Content,
            node_path: node_path.clone(),
            kind: ResolvedKind::Document,
            params: BTreeMap::new(),
        },
        intent: RenderIntent::MarkdownContent { node_path },
    }
}

fn route_request_needs_system_fs(request: &RouteRequest) -> bool {
    let trimmed = request.url_path.trim_matches('/');
    if is_runtime_state_request(trimmed) {
        return true;
    }
    trimmed
        .strip_prefix("websh/")
        .is_some_and(is_runtime_state_request)
}

fn is_runtime_state_request(path: &str) -> bool {
    path == ".websh/state" || path.starts_with(".websh/state/")
}

fn ledger_filter_frame(request: RouteRequest) -> RouteFrame {
    let node_path = if request.url_path.trim_matches('/') == LEDGER_ROUTE {
        VirtualPath::root()
    } else {
        VirtualPath::from_absolute(&request.url_path).unwrap_or_else(|_| VirtualPath::root())
    };
    RouteFrame {
        request: request.clone(),
        resolution: RouteResolution {
            request_path: request.url_path,
            surface: RouteSurface::Content,
            node_path: node_path.clone(),
            kind: ResolvedKind::Directory,
            params: BTreeMap::new(),
        },
        intent: RenderIntent::DirectoryListing { node_path },
    }
}

fn home_frame(request: RouteRequest) -> RouteFrame {
    RouteFrame {
        request: request.clone(),
        resolution: RouteResolution {
            request_path: request.url_path,
            surface: RouteSurface::Content,
            node_path: VirtualPath::root(),
            kind: ResolvedKind::Directory,
            params: BTreeMap::new(),
        },
        intent: RenderIntent::DirectoryListing {
            node_path: VirtualPath::root(),
        },
    }
}

/// Wraps a concrete [`RouteFrame`] in a [`Memo`] so it can be passed to a
/// component that expects a reactive prop, without each call site repeating
/// the `Option`-unwrap-and-`expect` dance against the outer route Memo.
fn static_route_memo(frame: RouteFrame) -> Memo<RouteFrame> {
    Memo::new(move |_| frame.clone())
}

/// Refocuses the terminal input when the user returns to a shell surface from
/// a Reader-bound surface. Lives in its own helper so the router body doesn't
/// carry the cross-cutting concern inline.
fn install_terminal_focus_effect(
    raw_request: RwSignal<RouteRequest>,
    route: Memo<Option<RouteFrame>>,
) {
    Effect::new(move |prev_was_reader: Option<bool>| {
        if matches!(
            BuiltinRoute::detect(&raw_request.get()),
            Some(BuiltinRoute::Home)
        ) {
            return false;
        }

        let is_reader = route.get().is_some_and(|frame| {
            !matches!(
                frame.intent,
                RenderIntent::TerminalApp { .. } | RenderIntent::DirectoryListing { .. }
            )
        });
        if prev_was_reader == Some(true) && !is_reader {
            focus_terminal_input();
        }
        is_reader
    });
}

#[component]
fn NotFound() -> impl IntoView {
    view! {
        <div style="padding: 2rem; font-family: monospace;">
            <h1>"404"</h1>
            <p>"No route matched the current path."</p>
        </div>
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod builtin_route_tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn detects_builtin_routes() {
        let cases = [
            ("/", Some(BuiltinRoute::Home)),
            ("/ledger", Some(BuiltinRoute::LedgerFilter)),
            ("/writing", Some(BuiltinRoute::LedgerFilter)),
            ("/projects", Some(BuiltinRoute::LedgerFilter)),
            ("/papers", Some(BuiltinRoute::LedgerFilter)),
            ("/talks", Some(BuiltinRoute::LedgerFilter)),
            ("/misc", Some(BuiltinRoute::LedgerFilter)),
            ("/new", Some(BuiltinRoute::NewCompose)),
        ];

        for (path, expected) in cases {
            assert_eq!(
                BuiltinRoute::detect(&RouteRequest::new(path)),
                expected,
                "unexpected builtin route for {path}"
            );
        }
    }

    #[wasm_bindgen_test]
    fn rejects_engine_routes() {
        // `/ledger/foo` and `/papers/x.pdf` lock in that ledger detection is
        // an exact-match on the trimmed path, not a prefix match — sub-paths
        // under reserved categories must reach the engine.
        for path in ["/blog/hello.md", "/websh", "/papers/x.pdf", "/ledger/foo"] {
            assert_eq!(
                BuiltinRoute::detect(&RouteRequest::new(path)),
                None,
                "expected engine route for {path}"
            );
        }
    }

    #[wasm_bindgen_test]
    fn detects_explicit_runtime_state_routes() {
        for path in [
            "/.websh/state",
            "/.websh/state/session",
            "/websh/.websh/state/session",
        ] {
            assert!(
                route_request_needs_system_fs(&RouteRequest::new(path)),
                "expected system fs for {path}"
            );
        }
    }

    #[wasm_bindgen_test]
    fn keeps_normal_routes_on_content_view() {
        for path in ["/", "/ledger", "/websh", "/writing/example"] {
            assert!(
                !route_request_needs_system_fs(&RouteRequest::new(path)),
                "expected content fs for {path}"
            );
        }
    }
}
