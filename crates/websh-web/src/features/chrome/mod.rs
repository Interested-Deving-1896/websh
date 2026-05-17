//! Shared site chrome primitives.
//!
//! These components own the common site chrome visual language used by the
//! homepage, renderer pages, ledger pages, and the live shell. Route-aware callers provide plain labels,
//! links, active state, and display values.

use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::app::AppContext;
use crate::app::RuntimeServices;
use crate::config::APP_NAME;
use crate::features::ledger::routes::is_ledger_filter_route_segment;
use crate::render::theme::THEMES;
use crate::shared::components::{MonoOverflow, MonoValue};
use websh_core::domain::{VirtualPath, WalletState};
use websh_core::filesystem::{
    RouteFrame, RouteSurface, request_path_for_canonical_path, route_cwd,
};

stylance::import_crate_style!(css, "src/features/chrome/site_chrome.module.css");

pub const HOME_HREF: &str = "#/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteChromeSurface {
    Home,
    Shell,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteChromeBreadcrumbItem {
    pub label: String,
    pub href: Option<String>,
    pub current: bool,
}

impl SiteChromeBreadcrumbItem {
    pub fn link(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: Some(href.into()),
            current: false,
        }
    }

    pub fn current(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: None,
            current: true,
        }
    }
}

#[component]
pub fn SiteChromeRoot(surface: SiteChromeSurface, children: Children) -> impl IntoView {
    let surface_class = match surface {
        SiteChromeSurface::Home => css::surfaceHome,
        SiteChromeSurface::Shell => css::surfaceShell,
    };

    view! {
        <header class=format!("{} {}", css::archive, surface_class)>
            {children()}
        </header>
    }
}

#[component]
pub fn SiteChrome(route: Memo<RouteFrame>) -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext must be provided");
    let theme = ctx.theme;
    let identity_href = Signal::derive(|| HOME_HREF.to_string());
    let breadcrumbs = Signal::derive(move || route_breadcrumb_items(&route.get()));
    let active_section = Signal::derive(move || {
        route
            .get()
            .request
            .url_path
            .trim_matches('/')
            .split('/')
            .next()
            .unwrap_or("")
            .to_string()
    });
    let websh_href = Signal::derive(move || {
        let frame = route.get();
        let cwd = match frame.surface() {
            RouteSurface::Shell => route_cwd(&frame),
            RouteSurface::Content => VirtualPath::root(),
        };
        route_href(&request_path_for_canonical_path(&cwd, RouteSurface::Shell))
    });
    let websh_active = Signal::derive(move || {
        let frame = route.get();
        matches!(frame.surface(), RouteSurface::Shell) || active_section.get() == "websh"
    });

    view! {
        <SiteChromeRoot surface=SiteChromeSurface::Home>
            <SiteChromeLead>
                <SiteChromeIdentity label=APP_NAME href=identity_href />
                <SiteChromeWalletButton />
            </SiteChromeLead>
            <SiteChromeBreadcrumb items=breadcrumbs />
            <SiteChromeActions>
                <SiteChromeNav>
                    <SiteChromeSiteNavItems
                        active_key=active_section
                        websh_href=websh_href
                        websh_active=websh_active
                    />
                </SiteChromeNav>
                <SiteChromeDivider />
                <SiteChromePalettePicker theme=theme />
            </SiteChromeActions>
        </SiteChromeRoot>
    }
}

#[component]
pub fn SiteChromeIdentity(label: &'static str, href: Signal<String>) -> impl IntoView {
    view! {
        <a href=move || href.get() class=css::identity>{label}</a>
    }
}

#[component]
pub fn SiteChromeLead(children: Children) -> impl IntoView {
    view! {
        <div class=css::lead data-chrome-role="lead">{children()}</div>
    }
}

#[component]
pub fn SiteChromeBreadcrumb(
    items: Signal<Vec<SiteChromeBreadcrumbItem>>,
    #[prop(optional, default = "path")] aria_label: &'static str,
) -> impl IntoView {
    view! {
        <nav class=css::breadcrumb aria-label=aria_label data-chrome-role="breadcrumb">
            {move || render_site_chrome_breadcrumb_items(items.get())}
        </nav>
    }
}

#[component]
pub fn SiteChromeActions(children: Children) -> impl IntoView {
    view! {
        <div class=css::actions data-chrome-role="actions">{children()}</div>
    }
}

#[component]
pub fn SiteChromeNav(children: Children) -> impl IntoView {
    view! {
        <nav class=css::nav aria-label="site navigation" data-chrome-role="nav">{children()}</nav>
    }
}

fn render_site_chrome_breadcrumb_items(items: Vec<SiteChromeBreadcrumbItem>) -> impl IntoView {
    items
        .into_iter()
        .enumerate()
        .map(move |(idx, item)| {
            let separator = (idx > 0).then(|| {
                view! {
                    <span class=css::separator aria-hidden="true">"/"</span>
                }
            });

            view! {
                <>
                    {separator}
                    {render_site_chrome_breadcrumb_item(item)}
                </>
            }
        })
        .collect_view()
}

fn render_site_chrome_breadcrumb_item(item: SiteChromeBreadcrumbItem) -> AnyView {
    let is_current = item.current || item.href.is_none();
    if is_current {
        view! {
            <span class=css::crumbCurrent aria-current="location" data-breadcrumb-current="true">
                {item.label}
            </span>
        }
        .into_any()
    } else if let Some(href) = item.href {
        view! {
            <a href=href class=css::crumb>{item.label}</a>
        }
        .into_any()
    } else {
        view! {
            <span class=css::crumbCurrent aria-current="location" data-breadcrumb-current="true">
                {item.label}
            </span>
        }
        .into_any()
    }
}

#[component]
pub fn SiteChromeNavLink(
    label: &'static str,
    href: Signal<String>,
    active: Signal<bool>,
) -> impl IntoView {
    view! {
        <a
            href=move || href.get()
            class=move || {
                if active.get() {
                    format!("{} {}", css::navLink, css::navLinkActive)
                } else {
                    css::navLink.to_string()
                }
            }
            aria-current=move || if active.get() { "page" } else { "false" }
        >
            {label}
        </a>
    }
}

#[component]
pub fn SiteChromeSiteNavItems(
    active_key: Signal<String>,
    websh_href: Signal<String>,
    websh_active: Signal<bool>,
) -> impl IntoView {
    let ledger_active =
        Signal::derive(move || is_ledger_filter_route_segment(active_key.get().as_str()));

    view! {
        <SiteChromeNavLink
            label="home"
            href=Signal::derive(|| HOME_HREF.to_string())
            active=Signal::derive(move || active_key.get().is_empty())
        />
        <SiteChromeNavLink
            label="ledger"
            href=Signal::derive(|| "#/ledger".to_string())
            active=ledger_active
        />
        <SiteChromeNavLink
            label="websh"
            href=websh_href
            active=websh_active
        />
    }
}

#[component]
pub fn SiteChromeChip(label: &'static str, value: Signal<String>) -> impl IntoView {
    view! {
        <span class=css::chip>
            <span class=css::chipKey>{label}</span>
            <span class=css::chipValue>{value}</span>
        </span>
    }
}

#[component]
pub fn SiteChromeTextChip(value: Signal<String>) -> impl IntoView {
    view! {
        <span class=css::textChip>{value}</span>
    }
}

/// Interactive variant of the lead chips that surfaces wallet connect/disconnect
/// actions without altering the chip visuals. Two `SiteChromeChip`s
/// (session, network) are wrapped in a single `<button>` so the entire pair is
/// a single hit target; hover only shifts text color.
///
/// On open, a dropdown menu is shown whose contents reflect `WalletState`:
/// - Disconnected: a `connect wallet` action.
/// - Connecting: a static `connecting…` line (no actions).
/// - Connected: address, network, divider, `disconnect`.
#[component]
pub fn SiteChromeWalletButton() -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext must be provided");
    let (open, set_open) = signal(false);

    let session = Signal::derive(move || ctx.wallet.with(|w| w.display_name()));
    let network = Signal::derive(move || {
        ctx.wallet.with(|wallet| {
            wallet
                .chain_id()
                .map(|id| websh_core::domain::chain_name(id).to_ascii_lowercase())
                .unwrap_or_else(|| "offline".to_string())
        })
    });

    let toggle = move |ev: ev::MouseEvent| {
        ev.stop_propagation();
        set_open.update(|o| *o = !*o);
    };
    let trigger_keydown = move |ev: ev::KeyboardEvent| match ev.key().as_str() {
        "Escape" => set_open.set(false),
        "ArrowDown" | "Enter" | " " => {
            ev.prevent_default();
            set_open.set(true);
        }
        _ => {}
    };

    view! {
        <span class=css::walletButton>
            <button
                class=css::walletTrigger
                type="button"
                aria-haspopup="dialog"
                aria-expanded=move || open.get().to_string()
                on:click=toggle
                on:keydown=trigger_keydown
            >
                <SiteChromeChip label="session" value=session />
                <SiteChromeChip label="network" value=network />
            </button>
            <Show when=move || open.get()>
                <button
                    class=css::walletDismiss
                    type="button"
                    aria-label="Close wallet menu"
                    on:click=move |_| set_open.set(false)
                ></button>
                <SiteChromeWalletMenu set_open=set_open />
            </Show>
        </span>
    }
}

#[component]
fn SiteChromeWalletMenu(set_open: WriteSignal<bool>) -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext must be provided");

    let close = move || set_open.set(false);

    let on_connect = move |ev: ev::MouseEvent| {
        ev.stop_propagation();
        close();
        spawn_local(async move {
            let _ = RuntimeServices::new(ctx)
                .connect_wallet_with_session()
                .await;
        });
    };

    let on_disconnect = move |ev: ev::MouseEvent| {
        ev.stop_propagation();
        close();
        let _ = RuntimeServices::new(ctx).disconnect_wallet();
    };

    let stop_inside = move |ev: ev::MouseEvent| ev.stop_propagation();
    let close_on_escape = move |ev: ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            ev.prevent_default();
            close();
        }
    };

    view! {
        <div class=css::walletMenu aria-label="Wallet" on:click=stop_inside on:keydown=close_on_escape>
            {move || ctx.wallet.with(|state| match state {
                WalletState::Disconnected => view! {
                    <button
                        class=css::walletMenuItem
                        type="button"
                        on:click=on_connect
                    >
                        "connect wallet"
                    </button>
                }.into_any(),
                WalletState::Connecting => view! {
                    <span class=css::walletMenuStatus>"connecting…"</span>
                }.into_any(),
                WalletState::Connected { address, ens_name, chain_id } => {
                    let address_full = address.clone();
                    let ens = ens_name.clone();
                    let chain = chain_id
                        .map(|id| format!("{} · chain {}", websh_core::domain::chain_name(id).to_ascii_lowercase(), id))
                        .unwrap_or_else(|| "no chain".to_string());
                    view! {
                        <div class=css::walletMenuRow>
                            <span class=css::walletMenuKey>"address"</span>
                            <MonoValue
                                value=address_full.clone()
                                overflow=MonoOverflow::Middle { head: 10, tail: 8 }
                                title=address_full
                            />
                        </div>
                        {ens.map(|name| view! {
                            <div class=css::walletMenuRow>
                                <span class=css::walletMenuKey>"ens"</span>
                                <MonoValue value=name overflow=MonoOverflow::TruncateEnd />
                            </div>
                        })}
                        <div class=css::walletMenuRow>
                            <span class=css::walletMenuKey>"network"</span>
                            <span class=css::walletMenuVal>{chain}</span>
                        </div>
                        <span class=css::walletMenuDivider aria-hidden="true"></span>
                        <button
                            class=css::walletMenuItem
                            type="button"
                            on:click=on_disconnect
                        >
                            "disconnect"
                        </button>
                    }.into_any()
                }
            })}
        </div>
    }
}

#[component]
pub fn SiteChromeDivider() -> impl IntoView {
    view! {
        <span class=css::divider aria-hidden="true"></span>
    }
}

#[component]
pub fn SiteChromePalettePicker(theme: RwSignal<&'static str>) -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext must be provided");
    let (palette_open, set_palette_open) = signal(false);
    let toggle_palette = move |_| {
        set_palette_open.update(|open| *open = !*open);
    };
    let palette_keydown = move |ev: ev::KeyboardEvent| match ev.key().as_str() {
        "Escape" => set_palette_open.set(false),
        "ArrowDown" | "Enter" | " " => {
            ev.prevent_default();
            set_palette_open.set(true);
        }
        _ => {}
    };

    view! {
        <div class=css::themePicker data-chrome-role="theme">
            <button
                class=css::paletteTrigger
                type="button"
                title="Palette"
                aria-haspopup="dialog"
                aria-expanded=move || palette_open.get().to_string()
                on:click=toggle_palette
                on:keydown=palette_keydown
            >
                <span class=css::themeSwatch aria-hidden="true"></span>
                <span class=css::themeLabel>"palette"</span>
                <span class=css::paletteChevron aria-hidden="true">"▾"</span>
            </button>
            <Show when=move || palette_open.get()>
                <button
                    class=css::paletteDismiss
                    type="button"
                    aria-label="Close palette menu"
                    on:click=move |_| set_palette_open.set(false)
                ></button>
                <div class=css::paletteMenu aria-label="Palette">
                    {THEMES.iter().map(|item| {
                        let id = item.id;
                        let label = item.label;
                        let bg = item.meta_color;
                        let accent = item.accent_color;
                        let option_class = move || {
                            if theme.get() == id {
                                format!("{} {}", css::paletteOption, css::paletteOptionActive)
                            } else {
                                css::paletteOption.to_string()
                            }
                        };
                        let select_theme = move |_| {
                            if let Err(error) = RuntimeServices::new(ctx).set_theme(id) {
                                web_sys::console::error_1(&format!("theme: {error}").into());
                            }
                            set_palette_open.set(false);
                        };
                        view! {
                            <button
                                class=option_class
                                type="button"
                                aria-pressed=move || (theme.get() == id).to_string()
                                style=format!("--palette-bg: {bg}; --palette-accent: {accent}")
                                on:click=select_theme
                            >
                                <span class=css::paletteOptionSwatch aria-hidden="true"></span>
                                <span class=css::paletteOptionLabel>{label}</span>
                                <span class=css::paletteOptionStatus>
                                    {move || if theme.get() == id { "on" } else { "" }}
                                </span>
                            </button>
                        }
                    }).collect_view()}
                </div>
            </Show>
        </div>
    }
}

fn route_breadcrumb_items(frame: &RouteFrame) -> Vec<SiteChromeBreadcrumbItem> {
    match frame.surface() {
        RouteSurface::Content => content_breadcrumb_items(frame),
        RouteSurface::Shell => surface_breadcrumb_items("websh", RouteSurface::Shell, frame),
    }
}

fn content_breadcrumb_items(frame: &RouteFrame) -> Vec<SiteChromeBreadcrumbItem> {
    if frame.request.url_path == "/ledger" {
        return vec![
            SiteChromeBreadcrumbItem::link("~", HOME_HREF),
            SiteChromeBreadcrumbItem::current("ledger"),
        ];
    }

    let path = VirtualPath::from_absolute(frame.request.url_path.clone()).unwrap_or_else(|_| {
        if frame.is_file() {
            frame.resolution.node_path.clone()
        } else {
            route_cwd(frame)
        }
    });
    canonical_breadcrumb_items(&path, RouteSurface::Content, None)
}

fn surface_breadcrumb_items(
    label: &'static str,
    surface: RouteSurface,
    frame: &RouteFrame,
) -> Vec<SiteChromeBreadcrumbItem> {
    canonical_breadcrumb_items(&route_cwd(frame), surface, Some(label))
}

fn canonical_breadcrumb_items(
    path: &VirtualPath,
    surface: RouteSurface,
    surface_label: Option<&'static str>,
) -> Vec<SiteChromeBreadcrumbItem> {
    let mut items = Vec::new();

    if path.is_root() && surface_label.is_none() {
        items.push(SiteChromeBreadcrumbItem::current("~"));
        return items;
    }

    items.push(SiteChromeBreadcrumbItem::link("~", HOME_HREF));

    if let Some(label) = surface_label {
        if path.is_root() {
            items.push(SiteChromeBreadcrumbItem::current(label));
            return items;
        }
        items.push(SiteChromeBreadcrumbItem::link(
            label,
            route_href(&request_path_for_canonical_path(
                &VirtualPath::root(),
                surface,
            )),
        ));
    }

    let segments = path.segments().collect::<Vec<_>>();
    for idx in 0..segments.len() {
        let label = segments[idx];
        if idx + 1 == segments.len() {
            items.push(SiteChromeBreadcrumbItem::current(label));
        } else {
            let path = VirtualPath::from_absolute(format!("/{}", segments[..=idx].join("/")))
                .expect("route path");
            items.push(SiteChromeBreadcrumbItem::link(
                label,
                route_href(&request_path_for_canonical_path(&path, surface)),
            ));
        }
    }

    items
}

fn route_href(path: &str) -> String {
    if path == "/" {
        HOME_HREF.to_string()
    } else {
        format!("#{path}")
    }
}
