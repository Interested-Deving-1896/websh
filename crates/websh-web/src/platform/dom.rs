//! Browser DOM helpers owned by the web crate.

use wasm_bindgen::JsCast;
use websh_core::filesystem::RouteRequest;

pub fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

/// Focus an element by CSS selector. Returns `true` on success.
pub fn focus_element(selector: &str) -> bool {
    if let Some(window) = window()
        && let Some(document) = window.document()
        && let Some(element) = document.query_selector(selector).ok().flatten()
        && let Ok(html_element) = element.dyn_into::<web_sys::HtmlElement>()
    {
        html_element.focus().is_ok()
    } else {
        false
    }
}

#[inline]
pub fn focus_terminal_input() {
    focus_element("input");
}

pub fn current_route_request() -> RouteRequest {
    RouteRequest::new(current_hash())
}

pub fn push_route(route: &RouteRequest) {
    push_request_path(&route.url_path);
}

pub fn replace_route(route: &RouteRequest) {
    replace_request_path(&route.url_path);
}

pub fn push_request_path(path: &str) {
    set_hash(&format!("#{}", RouteRequest::new(path).url_path));
}

pub fn replace_request_path(path: &str) {
    replace_hash(&format!("#{}", RouteRequest::new(path).url_path));
    dispatch_hashchange();
}

pub fn absolute_hash_url_for_request_path(path: &str) -> String {
    let route_path = RouteRequest::new(path).url_path;
    let Some(window) = window() else {
        return format!("#{route_path}");
    };
    let location = window.location();
    let origin = location.origin().unwrap_or_default();
    let pathname = location.pathname().unwrap_or_else(|_| "/".to_string());
    let search = location.search().unwrap_or_default();
    format!("{origin}{pathname}{search}#{route_path}")
}

fn current_hash() -> String {
    window()
        .and_then(|w| w.location().hash().ok())
        .unwrap_or_default()
        .trim_start_matches('#')
        .to_string()
}

fn set_hash(hash: &str) {
    if let Some(window) = window() {
        let _ = window.location().set_hash(hash);
    }
}

fn replace_hash(hash: &str) {
    if let Some(window) = window()
        && let Ok(history) = window.history()
    {
        let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(hash));
    }
}

fn dispatch_hashchange() {
    if let Some(window) = window()
        && let Ok(event) = web_sys::Event::new("hashchange")
    {
        let _ = window.dispatch_event(&event);
    }
}
