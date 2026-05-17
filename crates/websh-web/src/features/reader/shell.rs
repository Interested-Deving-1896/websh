//! `ReaderShell` — outer chrome shared by every reader view.
//!
//! All view modes (markdown / html / plain / pdf / asset / redirect, plus
//! the edit textarea) sit inside the same surface: site chrome, an
//! identifier strip, a title block + meta table, the body slot, the
//! reader actions menu, the author toolbar, and the attestation footer.
//!
//! The shell takes the body as `children`. Reader's state reaches it
//! through two `Copy` bundles, mirroring the project's `AppContext`
//! pattern — every field is a signal / memo / callback (themselves
//! `Copy`), so the bundles travel as cheap value types.

use leptos::prelude::*;

use crate::features::chrome::SiteChrome;
use crate::shared::components::AttestationSigFooter;
use websh_core::filesystem::RouteFrame;

use super::ReaderMode;
use super::actions::ReaderActionsBindings;
use super::css;
use super::intent::ReaderIntent;
use super::meta::ReaderMeta;
use super::title_block::{Ident, TitleBlock};
use super::toolbar::ReaderToolbar;

/// Document-side reactive inputs to the shell — what the chrome, title
/// block, and footer need.
#[derive(Clone, Copy)]
pub struct ReaderShellState {
    pub intent: Memo<ReaderIntent>,
    pub meta: Memo<ReaderMeta>,
    pub chrome_route: Memo<RouteFrame>,
    pub attestation_route: Signal<String>,
    pub show_pending: Signal<bool>,
    pub save_error: ReadSignal<Option<String>>,
    pub set_preferred_locale: Callback<String>,
}

/// Edit-mode reactive state and action callbacks — what the toolbar
/// reads and dispatches. Used by both the shell (to forward to the
/// toolbar) and the toolbar itself.
#[derive(Clone, Copy)]
pub struct ReaderEditBindings {
    pub mode: RwSignal<ReaderMode>,
    pub can_edit: Memo<bool>,
    pub saving: ReadSignal<bool>,
    pub dirty: ReadSignal<bool>,
    pub on_edit: Callback<()>,
    pub on_preview: Callback<()>,
    pub on_save: Callback<()>,
    pub on_cancel: Callback<()>,
}

#[component]
pub fn ReaderShell(
    state: ReaderShellState,
    edit: ReaderEditBindings,
    actions: ReaderActionsBindings,
    children: Children,
) -> impl IntoView {
    view! {
        <div class=css::surface>
            <SiteChrome route=state.chrome_route />
            <main class=css::page>
                <div class=css::content>
                    <Show when=move || !matches!(state.intent.get(), ReaderIntent::Redirect { .. })>
                        <Ident meta=state.meta />
                        <TitleBlock
                            intent=state.intent
                            meta=state.meta
                            actions=actions
                            variants_disabled=Signal::derive(move || {
                                edit.mode.get() == ReaderMode::Edit && edit.dirty.get()
                            })
                            set_preferred_locale=state.set_preferred_locale
                        />
                    </Show>
                    {move || state.save_error.get().map(|message| view! {
                        <div class=css::errorBanner role="alert">{message}</div>
                    })}
                    <div
                        class=css::readerBody
                        data-reader-body="true"
                        data-text-scale=move || actions.text_scale.get().attr()
                    >
                        {children()}
                    </div>
                </div>
                <ReaderToolbar edit=edit />
                <AttestationSigFooter
                    route=state.attestation_route
                    show_pending=state.show_pending
                />
            </main>
        </div>
    }
}
