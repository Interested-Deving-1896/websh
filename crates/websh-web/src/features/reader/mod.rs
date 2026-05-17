//! Reader page — view and edit modes for content under `/`.
//!
//! For mempool paths in author mode, a small toolbar at the top of the
//! article frame surfaces an `edit` button (View) or `preview / cancel /
//! save` (Edit). The URL never changes across the toggle. `/new` mounts
//! the same component in Edit with a frontmatter placeholder.
//!
//! Toolbar lives inside the reader (document-scoped); site chrome stays
//! site-scoped. Draft state survives the Edit ↔ Preview round-trip via a
//! `draft_dirty` flag — the user's typed content is never silently
//! clobbered by re-seeding from `raw_source`.

mod actions;
mod document;
mod error;
mod intent;
mod keybindings;
mod meta;
mod preferences;
mod shell;
mod title_block;
mod toolbar;
mod views;

pub use intent::{ReaderFrame, ReaderIntent};

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::app::{AppContext, RuntimeServices};
use crate::features::mempool::save_raw;
use crate::platform::current_timestamp;
use crate::platform::dom::{
    absolute_hash_url_for_request_path, push_request_path, replace_request_path,
};
use websh_core::filesystem::{RouteFrame, attestation_route_for_node_path, content_route_for_path};
use websh_core::mempool::{derive_new_path, placeholder_frontmatter};
use websh_core::support::format::format_date_iso;
use websh_core::support::normalize_locale_tag;

use actions::ReaderActionsBindings;
use document::{ReaderDocument, RendererContent, load_reader_document};
use error::ReaderLoadError;
use keybindings::{KeybindingTargets, install_reader_keybindings};
use meta::{ReaderMeta, reader_meta};
use preferences::{initial_text_scale, intent_supports_text_scale, persist_text_scale};
use shell::{ReaderEditBindings, ReaderShell, ReaderShellState};
use views::{
    AssetReaderView, HtmlReaderView, MarkdownEditorView, MarkdownReaderView, PdfReaderView,
    PlainReaderView, RedirectingView,
};

// One stylance import for the whole reader module. `views/*.rs` and
// `title_block.rs` reach this via `crate::features::reader::css` rather
// than re-importing the CSS — every additional `import_crate_style!` site
// duplicates the full constant set and produces dead-code warnings for
// classes that file doesn't reference.
stylance::import_crate_style!(
    pub(crate) css,
    "src/features/reader/reader.module.css"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReaderMode {
    View,
    Edit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditorRouteKey {
    request_path: String,
    node_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditorSeed {
    mode: ReaderMode,
    draft_body: String,
    draft_owned: bool,
    draft_dirty: bool,
}

#[component]
pub fn Reader(frame: Memo<ReaderFrame>) -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext must be provided");
    let canonical_path = Memo::new(move |_| frame.get().resolution.node_path.clone());
    let attestation_route =
        Signal::derive(move || attestation_route_for_node_path(&canonical_path.get()));

    let intent_memo = Memo::new(move |_| frame.get().intent.clone());
    let reader_meta_memo = Memo::new(move |_| reader_meta(ctx, &intent_memo.get()));

    let author_mode = Memo::new(move |_| ctx.runtime_state.with(|rs| rs.github_token_present));
    let is_new_route = Memo::new(move |_| frame.get().request.url_path == "/new");
    let edit_visible = Memo::new(move |_| {
        author_mode.get()
            && (canonical_path.get().as_str().starts_with("/mempool/") || is_new_route.get())
    });

    // Construction-time seed.
    //
    // /new starts in Edit with the placeholder; existing entries start in
    // View with an empty draft (filled lazily when the user clicks edit).
    let initial_seed = editor_seed_for(&frame.get_untracked());
    let mode = RwSignal::new(initial_seed.mode);
    let draft_body = RwSignal::new(initial_seed.draft_body);
    // `draft_owned` guards against re-seeding the textarea from `raw_source`
    // when the user round-trips Edit ↔ preview ↔ Edit. `draft_dirty` is the
    // narrower save-state — only flips true on actual keystrokes — and feeds
    // the toolbar's "● unsaved" indicator.
    let draft_owned = RwSignal::new(initial_seed.draft_owned);
    let draft_dirty = RwSignal::new(initial_seed.draft_dirty);
    let save_error = RwSignal::new(None::<String>);
    let saving = RwSignal::new(false);
    let refetch_epoch = RwSignal::new(0u32);
    let text_scale = RwSignal::new(initial_text_scale());

    // Author-mode redirect for /new — non-author lands on /ledger.
    Effect::new(move |_| {
        if is_new_route.get() && !author_mode.get() {
            replace_request_path("/ledger");
        }
    });

    // Defensive: if Leptos's into_any() boundary keeps the component identity
    // across Reader routes, apply the same editor seed a fresh mount would
    // have used. This preserves `/new` as Edit-with-placeholder and clears
    // transient edit state when leaving it.
    Effect::new(move |prev: Option<EditorRouteKey>| {
        let snapshot = frame.get();
        let key = editor_route_key(&snapshot);
        if prev.as_ref().is_some_and(|prev_key| prev_key != &key) {
            let seed = editor_seed_for(&snapshot);
            mode.set(seed.mode);
            draft_body.set(seed.draft_body);
            save_error.set(None);
            saving.set(false);
            draft_owned.set(seed.draft_owned);
            draft_dirty.set(seed.draft_dirty);
        }
        key
    });

    let document = LocalResource::new({
        move || {
            let snapshot = frame.get();
            let intent = snapshot.intent.clone();
            let _ = refetch_epoch.get();
            async move { load_reader_document(ctx, intent).await }
        }
    });

    let on_toggle_edit = move |()| {
        // Seed the editor only on first entry into Edit; the round-trip
        // back from preview must keep the in-flight draft intact. If the
        // resource hasn't resolved yet (mid-refetch on intent transition),
        // defer ownership so the next toggle re-tries with a real seed
        // — entering Edit with an empty seed and locking out the eventual
        // value would silently swallow the source.
        if !draft_owned.get_untracked()
            && let Some(seed) = document
                .get()
                .and_then(|result| result.ok().and_then(|document| document.raw_source))
        {
            draft_body.set(seed);
            draft_owned.set(true);
        }
        save_error.set(None);
        mode.set(ReaderMode::Edit);
    };

    let on_preview = move |()| {
        save_error.set(None);
        mode.set(ReaderMode::View);
    };

    let on_cancel = move |()| {
        if saving.get_untracked() {
            return;
        }
        if is_new_route.get_untracked() {
            replace_request_path("/ledger");
            return;
        }
        let seed = document
            .get()
            .and_then(|result| result.ok().and_then(|document| document.raw_source))
            .unwrap_or_default();
        draft_body.set(seed);
        draft_owned.set(false);
        draft_dirty.set(false);
        save_error.set(None);
        mode.set(ReaderMode::View);
    };

    let on_save = move |()| {
        if saving.get_untracked() {
            return;
        }
        let body = draft_body.get_untracked();

        if is_new_route.get_untracked() {
            let target = match derive_new_path(&body) {
                Ok(target) => target,
                Err(message) => {
                    save_error.set(Some(message.to_string()));
                    return;
                }
            };
            let rel = target
                .as_str()
                .trim_start_matches("/mempool/")
                .trim_end_matches(".md");
            let message = format!("mempool: add {rel}");
            saving.set(true);
            let target_for_nav = target.clone();
            spawn_local(async move {
                let result = save_raw(ctx, target, body, message, true).await;
                saving.set(false);
                match result {
                    Ok(()) => {
                        save_error.set(None);
                        push_request_path(&content_route_for_path(target_for_nav.as_str()));
                    }
                    Err(message) => save_error.set(Some(message.to_string())),
                }
            });
            return;
        }

        let path = canonical_path.get_untracked();
        if !path.as_str().starts_with("/mempool/") {
            save_error.set(Some(
                "save is only allowed for /mempool/... paths".to_string(),
            ));
            return;
        }
        let rel = path
            .as_str()
            .trim_start_matches("/mempool/")
            .trim_end_matches(".md");
        let message = format!("mempool: edit {rel}");
        saving.set(true);
        spawn_local(async move {
            let result = save_raw(ctx, path, body, message, false).await;
            saving.set(false);
            match result {
                Ok(()) => {
                    save_error.set(None);
                    draft_owned.set(false);
                    draft_dirty.set(false);
                    mode.set(ReaderMode::View);
                    refetch_epoch.update(|n| *n += 1);
                    document.refetch();
                }
                Err(message) => save_error.set(Some(message.to_string())),
            }
        });
    };

    let on_edit_cb = Callback::new(on_toggle_edit);
    let on_preview_cb = Callback::new(on_preview);
    let on_cancel_cb = Callback::new(on_cancel);
    let on_save_cb = Callback::new(on_save);
    let on_input_dirty_cb = Callback::new(move |()| draft_dirty.set(true));

    install_reader_keybindings(KeybindingTargets {
        mode,
        edit_visible,
        saving: saving.read_only(),
        on_save: on_save_cb,
        on_preview: on_preview_cb,
        on_toggle_edit: on_edit_cb,
    });

    let chrome_route = Memo::new(move |_| RouteFrame::from(frame.get()));
    let set_preferred_locale = Callback::new(move |locale: String| {
        let Some(locale) = normalize_locale_tag(&locale) else {
            return;
        };
        if let Err(error) =
            RuntimeServices::new(ctx).set_env_var(crate::config::LANG_ENV_KEY, &locale)
        {
            leptos::logging::warn!("reader: failed to persist LANG preference: {error}");
        }
    });

    // Mempool drafts are pre-signature; surface a "pending" chip there.
    // Other content paths show a chip only when an attestation exists
    // (the default behaviour). `/new` has no canonical path yet, so the
    // footer renders the border/colophon line without a chip.
    let show_pending = Signal::derive(move || {
        canonical_path.get().as_str().starts_with("/mempool/") && !is_new_route.get()
    });

    let shell_state = ReaderShellState {
        intent: intent_memo,
        meta: reader_meta_memo,
        chrome_route,
        attestation_route,
        show_pending,
        save_error: save_error.read_only(),
        set_preferred_locale,
    };

    let edit_bindings = ReaderEditBindings {
        mode,
        can_edit: edit_visible,
        saving: saving.read_only(),
        dirty: draft_dirty.read_only(),
        on_edit: on_edit_cb,
        on_preview: on_preview_cb,
        on_save: on_save_cb,
        on_cancel: on_cancel_cb,
    };

    let actions_bindings = ReaderActionsBindings {
        visible: Signal::derive(move || mode.get() == ReaderMode::View),
        text_scalable: Signal::derive(move || intent_supports_text_scale(&intent_memo.get())),
        text_scale: text_scale.read_only(),
        set_text_scale: Callback::new(move |scale| {
            text_scale.set(scale);
            persist_text_scale(scale);
        }),
        share_url: Signal::derive(move || {
            absolute_hash_url_for_request_path(&frame.get().request.url_path)
        }),
    };

    view! {
        <ReaderShell state=shell_state edit=edit_bindings actions=actions_bindings>
            <Show
                when=move || mode.get() == ReaderMode::Edit
                fallback=move || view! {
                    <Suspense fallback=move || view! {
                        <div class=css::loading>"Loading..."</div>
                    }>
                        {move || {
                            document.get().map(|result| {
                                render_view_body(result, reader_meta_memo)
                            })
                        }}
                    </Suspense>
                }
            >
                <MarkdownEditorView
                    draft_body=draft_body
                    on_input_dirty=on_input_dirty_cb
                />
            </Show>
        </ReaderShell>
    }
}

fn render_view_body(
    result: Result<ReaderDocument, ReaderLoadError>,
    meta: Memo<ReaderMeta>,
) -> AnyView {
    let document = match result {
        Ok(document) => document,
        Err(error) => return view! { <div class=css::error>{error.to_string()}</div> }.into_any(),
    };

    match document.content {
        RendererContent::Markdown(rendered) => {
            let rendered = Signal::derive(move || rendered.clone());
            view! { <MarkdownReaderView rendered=rendered /> }.into_any()
        }
        RendererContent::Html(rendered) => {
            let rendered = Signal::derive(move || rendered.clone());
            view! { <HtmlReaderView rendered=rendered /> }.into_any()
        }
        RendererContent::Text(text) => view! { <PlainReaderView text=text /> }.into_any(),
        RendererContent::Pdf { url } => {
            let title = Signal::derive(move || meta.get().title.clone());
            let m = meta.get_untracked();
            view! {
                <PdfReaderView
                    title=title
                    url=url
                    size_pretty=m.size_pretty
                    abstract_text=m.description
                    page_size=m.page_size
                    page_count=m.page_count
                />
            }
            .into_any()
        }
        RendererContent::Image { url } => {
            let m = meta.get_untracked();
            view! {
                <AssetReaderView
                    url=url
                    alt=m.title
                    dimensions=m.image_dimensions
                />
            }
            .into_any()
        }
        RendererContent::Redirecting => view! { <RedirectingView /> }.into_any(),
    }
}

fn iso_today() -> String {
    format_date_iso(current_timestamp() / 1000)
}

fn editor_route_key(frame: &ReaderFrame) -> EditorRouteKey {
    EditorRouteKey {
        request_path: frame.request.url_path.clone(),
        node_path: frame.resolution.node_path.as_str().to_string(),
    }
}

fn editor_seed_for(frame: &ReaderFrame) -> EditorSeed {
    if is_new_frame(frame) {
        return EditorSeed {
            mode: ReaderMode::Edit,
            draft_body: placeholder_frontmatter(&iso_today()),
            // /new puts the user on the hook for the placeholder content from
            // first paint, so both flags start true.
            draft_owned: true,
            draft_dirty: true,
        };
    }

    EditorSeed {
        mode: ReaderMode::View,
        draft_body: String::new(),
        draft_owned: false,
        draft_dirty: false,
    }
}

fn is_new_frame(frame: &ReaderFrame) -> bool {
    frame.request.url_path == "/new"
}
