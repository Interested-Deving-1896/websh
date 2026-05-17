//! Reader-facing action menu.
//!
//! This is separate from the author edit toolbar: it holds viewer actions
//! such as text size and sharing, and is designed to accept future reader
//! controls without changing the title layout.

use gloo_timers::callback::Timeout;
use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use super::css;
use super::preferences::ReaderTextScale;

#[derive(Clone, Copy)]
pub struct ReaderActionsBindings {
    pub visible: Signal<bool>,
    pub text_scalable: Signal<bool>,
    pub text_scale: ReadSignal<ReaderTextScale>,
    pub set_text_scale: Callback<ReaderTextScale>,
    pub share_url: Signal<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CopyStatus {
    #[default]
    Idle,
    Copying,
    Copied,
    Failed,
}

#[component]
pub fn ReaderActionsMenu(actions: ReaderActionsBindings) -> impl IntoView {
    let (open, set_open) = signal(false);
    let (copy_status, set_copy_status) = signal(CopyStatus::Idle);
    install_reader_actions_escape(open, set_open);

    let toggle = move |ev: ev::MouseEvent| {
        ev.stop_propagation();
        set_open.update(|open| *open = !*open);
        set_copy_status.set(CopyStatus::Idle);
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
        <Show when=move || actions.visible.get()>
            <span class=css::readerActions>
                <button
                    class=css::readerActionsTrigger
                    type="button"
                    title="Reader actions"
                    aria-label="Reader actions"
                    aria-haspopup="dialog"
                    aria-expanded=move || open.get().to_string()
                    on:click=toggle
                    on:keydown=trigger_keydown
                >
                    <span class=css::readerActionsDots aria-hidden="true"></span>
                </button>
                <Show when=move || open.get()>
                    <button
                        class=css::readerActionsDismiss
                        type="button"
                        aria-label="Close reader actions"
                        on:click=move |_| set_open.set(false)
                    ></button>
                    <ReaderActionsPopover
                        actions=actions
                        set_open=set_open
                        copy_status=copy_status
                        set_copy_status=set_copy_status
                    />
                </Show>
            </span>
        </Show>
    }
}

#[component]
fn ReaderActionsPopover(
    actions: ReaderActionsBindings,
    set_open: WriteSignal<bool>,
    copy_status: ReadSignal<CopyStatus>,
    set_copy_status: WriteSignal<CopyStatus>,
) -> impl IntoView {
    let stop_inside = move |ev: ev::MouseEvent| ev.stop_propagation();
    let close_on_escape = move |ev: ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            ev.prevent_default();
            set_open.set(false);
        }
    };

    view! {
        <div
            class=css::readerActionsMenu
            role="dialog"
            aria-label="Reader actions"
            on:click=stop_inside
            on:keydown=close_on_escape
        >
            <Show when=move || actions.text_scalable.get()>
                <ReaderTextSizeSection actions=actions />
                <span class=css::readerActionsDivider aria-hidden="true"></span>
            </Show>
            <ReaderShareSection
                actions=actions
                copy_status=copy_status
                set_copy_status=set_copy_status
            />
        </div>
    }
}

#[component]
fn ReaderTextSizeSection(actions: ReaderActionsBindings) -> impl IntoView {
    let decrease = move |_| {
        if let Some(next) = actions.text_scale.get_untracked().smaller() {
            actions.set_text_scale.run(next);
        }
    };
    let increase = move |_| {
        if let Some(next) = actions.text_scale.get_untracked().larger() {
            actions.set_text_scale.run(next);
        }
    };

    view! {
        <section class=css::readerActionsSection aria-label="Text size">
            <div class=css::readerActionsLabel>"Text size"</div>
            <div class=css::readerTextSizeGroup>
                <button
                    class=css::readerTextSizeOption
                    type="button"
                    aria-label="Decrease text size"
                    disabled=move || actions.text_scale.get().smaller().is_none()
                    on:click=decrease
                >
                    "A-"
                </button>
                <button
                    class=css::readerTextSizeOption
                    type="button"
                    aria-label="Increase text size"
                    disabled=move || actions.text_scale.get().larger().is_none()
                    on:click=increase
                >
                    "A+"
                </button>
            </div>
        </section>
    }
}

#[component]
fn ReaderShareSection(
    actions: ReaderActionsBindings,
    copy_status: ReadSignal<CopyStatus>,
    set_copy_status: WriteSignal<CopyStatus>,
) -> impl IntoView {
    let copy_label = move || match copy_status.get() {
        CopyStatus::Idle => "copy link",
        CopyStatus::Copying => "copying",
        CopyStatus::Copied => "copied",
        CopyStatus::Failed => "copy failed",
    };
    let copy_class = move || {
        if matches!(copy_status.get(), CopyStatus::Copied | CopyStatus::Failed) {
            format!("{} {}", css::readerActionItem, css::readerActionItemDone)
        } else {
            css::readerActionItem.to_string()
        }
    };
    let copy_link = move |_| {
        let url = actions.share_url.get_untracked();
        set_copy_status.set(CopyStatus::Copying);
        spawn_local(async move {
            let result = copy_to_clipboard(&url).await;
            set_copy_status.set(if result.is_ok() {
                CopyStatus::Copied
            } else {
                CopyStatus::Failed
            });
            Timeout::new(1600, move || set_copy_status.set(CopyStatus::Idle)).forget();
        });
    };

    view! {
        <section class=css::readerActionsSection aria-label="Share">
            <div class=css::readerActionsLabel>"Share"</div>
            <button
                class=copy_class
                type="button"
                disabled=move || copy_status.get() == CopyStatus::Copying
                on:click=copy_link
            >
                <span>{copy_label}</span>
                <span class=css::readerActionStatus>
                    {move || match copy_status.get() {
                        CopyStatus::Copied => "ok",
                        CopyStatus::Failed => "err",
                        _ => "",
                    }}
                </span>
            </button>
        </section>
    }
}

async fn copy_to_clipboard(text: &str) -> Result<(), ClipboardError> {
    let Some(window) = web_sys::window() else {
        return Err(ClipboardError::NoWindow);
    };
    let clipboard = window.navigator().clipboard();
    JsFuture::from(clipboard.write_text(text))
        .await
        .map(|_| ())
        .map_err(|error| ClipboardError::Write {
            message: error
                .as_string()
                .unwrap_or_else(|| "clipboard write failed".to_string()),
        })
}

#[derive(Clone, Debug, thiserror::Error)]
enum ClipboardError {
    #[error("window not available")]
    NoWindow,
    #[error("clipboard write failed: {message}")]
    Write { message: String },
}

#[cfg(target_arch = "wasm32")]
fn install_reader_actions_escape(open: ReadSignal<bool>, set_open: WriteSignal<bool>) {
    use crate::platform::wasm_cleanup::WasmCleanup;
    use leptos::prelude::on_cleanup;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };

    let closure = Closure::wrap(Box::new(move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Escape" && open.get_untracked() {
            ev.prevent_default();
            set_open.set(false);
        }
    }) as Box<dyn Fn(web_sys::KeyboardEvent)>);

    let _ = window.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());

    let cleanup = WasmCleanup(closure);
    on_cleanup(move || {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback("keydown", cleanup.js_function());
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn install_reader_actions_escape(_open: ReadSignal<bool>, _set_open: WriteSignal<bool>) {}
