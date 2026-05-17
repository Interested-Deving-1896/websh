//! HTML and Markdown rendering utilities.
//!
//! Provides safe HTML rendering boundaries with XSS protection.

use std::collections::{HashMap, HashSet};

use comrak::{Options, markdown_to_html as comrak_markdown_to_html};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedMarkdown {
    pub html: String,
    pub has_math: bool,
    /// In-document outline (h2 / h3 only) extracted from the rendered HTML.
    /// Empty for inputs with no qualifying headings or for non-markdown
    /// HTML inputs that lack `id` attributes on their headings.
    pub outline: Vec<HeadingEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadingEntry {
    pub level: u8,
    pub text: String,
    pub id: String,
}

/// Sanitize untrusted HTML before rendering it with `inner_html`.
pub fn sanitize_html(html: &str) -> String {
    let mut builder = ammonia::Builder::empty();
    builder
        .tags(markdown_tags())
        .tag_attributes(markdown_tag_attributes())
        .generic_attributes(HashSet::from(["lang", "title"]))
        .url_schemes(HashSet::from(["http", "https", "mailto"]))
        .link_rel(Some("noopener noreferrer"));
    builder.add_tag_attribute_values("input", "type", &["checkbox"]);
    builder.add_tag_attribute_values("span", "data-math-style", &["inline", "display"]);
    builder.clean(html).to_string()
}

/// Convert markdown content to sanitized HTML plus hydration metadata.
pub fn render_markdown(markdown: &str) -> RenderedMarkdown {
    let html = comrak_markdown_to_html(markdown, &markdown_options());
    rendered_from_html(sanitize_html(&html))
}

/// Convert a single inline markdown fragment to sanitized HTML plus hydration metadata.
pub fn render_inline_markdown(markdown: &str) -> RenderedMarkdown {
    let rendered = render_markdown(markdown);
    rendered_from_html(strip_paragraph_wrapper(&rendered.html).to_string())
}

pub fn rendered_from_html(html: String) -> RenderedMarkdown {
    let has_math =
        html.contains("data-math-style=\"inline\"") || html.contains("data-math-style=\"display\"");
    let outline = extract_outline(&html);
    RenderedMarkdown {
        html,
        has_math,
        outline,
    }
}

/// Walk a sanitized HTML body for `<h2>` / `<h3>` blocks and capture the
/// (level, anchor id, visible text) triple for each. Comrak emits the
/// heading id on an inner self-link `<a id="...">`; we tolerate that
/// shape and a bare `<h2 id="...">` as fallback.
///
/// The text strips inline markup (`<code>`, `<strong>`, …) and decodes
/// the entities ammonia produces (`&amp;`, `&lt;`, `&gt;`, `&quot;`,
/// `&#39;`, `&apos;`, plus numeric ampersand-escapes).
fn extract_outline(html: &str) -> Vec<HeadingEntry> {
    let mut entries = Vec::new();
    let bytes = html.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() {
        let Some(rel) = html[cursor..].find("<h") else {
            break;
        };
        let tag_start = cursor + rel;
        let level_pos = tag_start + 2;
        let level = match bytes.get(level_pos) {
            Some(b'2') => 2u8,
            Some(b'3') => 3u8,
            _ => {
                cursor = level_pos;
                continue;
            }
        };

        // Confirm tag boundary — `<h2x` should be skipped.
        let after_level = level_pos + 1;
        match bytes.get(after_level) {
            Some(b' ' | b'\t' | b'\n' | b'>') => {}
            _ => {
                cursor = after_level;
                continue;
            }
        }

        // Find the matching closing tag.
        let close_tag = if level == 2 { "</h2>" } else { "</h3>" };
        let Some(close_rel) = html[after_level..].find(close_tag) else {
            break;
        };
        let block_end = after_level + close_rel;
        let block = &html[tag_start..block_end];

        // The id attribute may sit on the opening `<h*>` itself or on the
        // inner self-link Comrak emits; either is fine.
        let Some(id) = find_id_attr(block) else {
            cursor = block_end + close_tag.len();
            continue;
        };

        // Locate the body content — skip past the opening `<h*…>` tag
        // and (when present) the empty inner anchor Comrak prepends.
        let after_open = match block.find('>') {
            Some(p) => p + 1,
            None => {
                cursor = block_end + close_tag.len();
                continue;
            }
        };
        let body = &block[after_open..];
        let body = body.strip_prefix("<a").map_or(body, |rest| {
            // Skip the closing `</a>` of the opening anchor (if any).
            rest.find("</a>")
                .map(|p| &rest[p + "</a>".len()..])
                .unwrap_or(body)
        });

        let text = decode_entities(&strip_tags(body)).trim().to_string();
        if !text.is_empty() {
            entries.push(HeadingEntry {
                level,
                text,
                id: id.to_string(),
            });
        }

        cursor = block_end + close_tag.len();
    }

    entries
}

fn find_id_attr(block: &str) -> Option<&str> {
    let needle = "id=\"";
    let mut cursor = 0;
    while let Some(rel) = block[cursor..].find(needle) {
        let attr_start = cursor + rel;
        // The id= must be preceded by whitespace to be a real attribute,
        // not the tail of some other token.
        let preceded_by_ws =
            attr_start == 0 || matches!(block.as_bytes()[attr_start - 1], b' ' | b'\t' | b'\n');
        let value_start = attr_start + needle.len();
        let value_end_rel = block[value_start..].find('"')?;
        if preceded_by_ws {
            return Some(&block[value_start..value_start + value_end_rel]);
        }
        cursor = value_start + value_end_rel + 1;
    }
    None
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;

    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }

    out
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with('&')
            && let Some(end_rel) = rest.find(';')
        {
            let entity = &rest[..end_rel + 1];
            if let Some(decoded) = decode_named_entity(entity) {
                out.push_str(decoded);
                i += end_rel + 1;
                continue;
            }
            if let Some(decoded) = decode_numeric_entity(entity) {
                out.push(decoded);
                i += end_rel + 1;
                continue;
            }
        }

        let ch = rest
            .chars()
            .next()
            .expect("non-empty slice while decoding entities");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn decode_named_entity(entity: &str) -> Option<&'static str> {
    match entity {
        "&amp;" => Some("&"),
        "&lt;" => Some("<"),
        "&gt;" => Some(">"),
        "&quot;" => Some("\""),
        "&apos;" => Some("'"),
        "&nbsp;" => Some("\u{00A0}"),
        _ => None,
    }
}

fn decode_numeric_entity(entity: &str) -> Option<char> {
    let body = entity.strip_prefix("&#")?.strip_suffix(';')?;
    let code: u32 = if let Some(hex) = body.strip_prefix(['x', 'X']) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        body.parse().ok()?
    };
    char::from_u32(code)
}

fn markdown_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.autolink = true;
    options.extension.front_matter_delimiter = Some("---".to_string());
    options.extension.header_id_prefix = Some(String::new());
    options.extension.math_dollars = true;
    options.extension.math_code = true;
    options.render.r#unsafe = false;
    options
}

fn markdown_tags() -> HashSet<&'static str> {
    HashSet::from([
        "a",
        "blockquote",
        "br",
        "caption",
        "code",
        "col",
        "colgroup",
        "del",
        "em",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "img",
        "input",
        "li",
        "ol",
        "p",
        "pre",
        "section",
        "span",
        "strong",
        "sup",
        "table",
        "tbody",
        "td",
        "th",
        "thead",
        "tr",
        "ul",
    ])
}

fn markdown_tag_attributes() -> HashMap<&'static str, HashSet<&'static str>> {
    let mut attrs = HashMap::new();
    attrs.insert(
        "a",
        HashSet::from([
            "aria-label",
            "data-footnote-backref",
            "data-footnote-ref",
            "href",
            "id",
            "title",
        ]),
    );
    attrs.insert("col", HashSet::from(["span"]));
    attrs.insert("h1", HashSet::from(["id"]));
    attrs.insert("h2", HashSet::from(["id"]));
    attrs.insert("h3", HashSet::from(["id"]));
    attrs.insert("h4", HashSet::from(["id"]));
    attrs.insert("h5", HashSet::from(["id"]));
    attrs.insert("h6", HashSet::from(["id"]));
    attrs.insert(
        "img",
        HashSet::from(["alt", "height", "src", "title", "width"]),
    );
    attrs.insert("input", HashSet::from(["checked", "disabled", "type"]));
    attrs.insert("li", HashSet::from(["id"]));
    attrs.insert("ol", HashSet::from(["start"]));
    attrs.insert("section", HashSet::from(["data-footnotes"]));
    attrs.insert("span", HashSet::from(["data-math-style"]));
    attrs.insert("td", HashSet::from(["colspan", "rowspan"]));
    attrs.insert("th", HashSet::from(["colspan", "rowspan", "scope"]));
    attrs
}

fn strip_paragraph_wrapper(html: &str) -> &str {
    html.strip_prefix("<p>")
        .and_then(|inner| {
            inner
                .strip_suffix("</p>\n")
                .or_else(|| inner.strip_suffix("</p>"))
        })
        .unwrap_or(html)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace = katex, js_name = render, catch)]
    fn katex_render(
        tex: &str,
        element: &web_sys::Element,
        options: &wasm_bindgen::JsValue,
    ) -> Result<(), wasm_bindgen::JsValue>;
}

#[cfg(target_arch = "wasm32")]
pub fn hydrate_math(root: &web_sys::Element) {
    let Ok(nodes) = root.query_selector_all("[data-math-style]:not([data-katex-rendered])") else {
        return;
    };
    if nodes.length() == 0 {
        return;
    }

    let root = root.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match wasm_bindgen_futures::JsFuture::from(ensure_katex_loaded()).await {
            Ok(_) => render_math_nodes(&root),
            Err(error) => {
                reset_katex_loader();
                web_sys::console::warn_1(&error);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
pub fn hydrate_math(_root: &web_sys::Element) {}

#[cfg(target_arch = "wasm32")]
fn render_math_nodes(root: &web_sys::Element) {
    use wasm_bindgen::JsCast;

    let Ok(nodes) = root.query_selector_all("[data-math-style]:not([data-katex-rendered])") else {
        return;
    };

    for index in 0..nodes.length() {
        let Some(node) = nodes.item(index) else {
            continue;
        };
        let Ok(element) = node.dyn_into::<web_sys::Element>() else {
            continue;
        };
        let tex = element.text_content().unwrap_or_default();
        if tex.trim().is_empty() {
            continue;
        }

        let display = element
            .get_attribute("data-math-style")
            .as_deref()
            .is_some_and(|style| style == "display");
        let options = katex_options(display);

        match katex_render(&tex, &element, &options) {
            Ok(()) => {
                let _ = element.set_attribute("data-katex-rendered", "true");
            }
            Err(error) => web_sys::console::warn_1(&error),
        }
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static KATEX_LOAD_PROMISE: std::cell::RefCell<Option<js_sys::Promise>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_arch = "wasm32")]
fn ensure_katex_loaded() -> js_sys::Promise {
    KATEX_LOAD_PROMISE.with(|slot| {
        if let Some(promise) = slot.borrow().as_ref() {
            return promise.clone();
        }

        let promise = create_katex_load_promise();
        *slot.borrow_mut() = Some(promise.clone());
        promise
    })
}

#[cfg(target_arch = "wasm32")]
fn reset_katex_loader() {
    KATEX_LOAD_PROMISE.with(|slot| {
        *slot.borrow_mut() = None;
    });
    if let Some(document) = web_sys::window().and_then(|window| window.document())
        && let Some(script) = document.get_element_by_id("websh-katex-js")
    {
        script.remove();
    }
}

#[cfg(target_arch = "wasm32")]
fn create_katex_load_promise() -> js_sys::Promise {
    use wasm_bindgen::JsCast;

    js_sys::Promise::new(&mut |resolve, reject| {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            let _ = reject.call1(
                &wasm_bindgen::JsValue::NULL,
                &wasm_bindgen::JsValue::from_str("KaTeX loader requires document"),
            );
            return;
        };

        inject_katex_css(&document);
        if katex_render_available() {
            let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
            return;
        }

        let script = match document
            .get_element_by_id("websh-katex-js")
            .or_else(|| create_katex_script(&document).ok())
        {
            Some(script) => script,
            None => {
                let _ = reject.call1(
                    &wasm_bindgen::JsValue::NULL,
                    &wasm_bindgen::JsValue::from_str("failed to create KaTeX script"),
                );
                return;
            }
        };

        let resolve_on_load = {
            let resolve = resolve.clone();
            wasm_bindgen::closure::Closure::<dyn FnMut()>::once(move || {
                let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
            })
        };
        let reject_on_error = {
            let reject = reject.clone();
            wasm_bindgen::closure::Closure::<dyn FnMut()>::once(move || {
                reset_katex_loader();
                let _ = reject.call1(
                    &wasm_bindgen::JsValue::NULL,
                    &wasm_bindgen::JsValue::from_str("failed to load KaTeX"),
                );
            })
        };
        let _ = script
            .add_event_listener_with_callback("load", resolve_on_load.as_ref().unchecked_ref());
        let _ = script
            .add_event_listener_with_callback("error", reject_on_error.as_ref().unchecked_ref());
        resolve_on_load.forget();
        reject_on_error.forget();
    })
}

#[cfg(target_arch = "wasm32")]
fn inject_katex_css(document: &web_sys::Document) {
    if document.get_element_by_id("websh-katex-css").is_some() {
        return;
    }
    let Ok(link) = document.create_element("link") else {
        return;
    };
    let _ = link.set_attribute("id", "websh-katex-css");
    let _ = link.set_attribute("rel", "stylesheet");
    let _ = link.set_attribute("href", "assets/vendor/katex/katex.min.css");
    if let Some(head) = document.head() {
        let _ = head.append_child(&link);
    }
}

#[cfg(target_arch = "wasm32")]
fn create_katex_script(
    document: &web_sys::Document,
) -> Result<web_sys::Element, wasm_bindgen::JsValue> {
    let script = document.create_element("script")?;
    script.set_attribute("id", "websh-katex-js")?;
    script.set_attribute("src", "assets/vendor/katex/katex.min.js")?;
    script.set_attribute("defer", "true")?;
    if let Some(head) = document.head() {
        let _ = head.append_child(&script);
    }
    Ok(script)
}

#[cfg(target_arch = "wasm32")]
fn katex_render_available() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Ok(katex) = js_sys::Reflect::get(&window, &wasm_bindgen::JsValue::from_str("katex")) else {
        return false;
    };
    if katex.is_null() || katex.is_undefined() {
        return false;
    }
    let Ok(render) = js_sys::Reflect::get(&katex, &wasm_bindgen::JsValue::from_str("render"))
    else {
        return false;
    };
    render.is_function()
}

#[cfg(target_arch = "wasm32")]
fn katex_options(display_mode: bool) -> wasm_bindgen::JsValue {
    use wasm_bindgen::JsValue;

    let options = js_sys::Object::new();
    set_js_option(&options, "displayMode", JsValue::from_bool(display_mode));
    set_js_option(&options, "throwOnError", JsValue::from_bool(false));
    set_js_option(&options, "trust", JsValue::from_bool(false));
    set_js_option(&options, "strict", JsValue::from_str("warn"));
    set_js_option(&options, "output", JsValue::from_str("htmlAndMathml"));
    set_js_option(&options, "maxSize", JsValue::from_f64(12.0));
    set_js_option(&options, "maxExpand", JsValue::from_f64(500.0));
    options.into()
}

#[cfg(target_arch = "wasm32")]
fn set_js_option(options: &js_sys::Object, key: &str, value: wasm_bindgen::JsValue) {
    let _ = js_sys::Reflect::set(options, &wasm_bindgen::JsValue::from_str(key), &value);
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn sanitize_html_removes_scripts_and_event_handlers() {
        let html = r#"<img src="x" onerror="alert(1)"><script>alert(2)</script>"#;
        let sanitized = sanitize_html(html);
        assert!(!sanitized.contains("onerror"));
        assert!(!sanitized.contains("<script"));
    }

    #[wasm_bindgen_test]
    fn sanitize_html_removes_encoded_javascript_links() {
        let sanitized = sanitize_html(r#"<a href="jav&#x61;script:alert(1)">bad</a>"#);
        assert!(sanitized.contains(">bad</a>"), "{sanitized}");
        assert!(!sanitized.contains("href"));
        assert!(!sanitized.contains("javascript"));
    }

    #[wasm_bindgen_test]
    fn sanitize_html_removes_data_image_sources() {
        let sanitized = sanitize_html(r#"<img alt="x" src="data:image/svg+xml,boom">"#);
        assert!(sanitized.contains("<img"), "{sanitized}");
        assert!(sanitized.contains(r#"alt="x""#), "{sanitized}");
        assert!(!sanitized.contains("src="), "{sanitized}");
        assert!(!sanitized.contains("data:"), "{sanitized}");
    }

    #[wasm_bindgen_test]
    fn sanitize_html_removes_unquoted_event_handlers() {
        let sanitized = sanitize_html(r#"<p onclick=alert(1) title=safe>text</p>"#);
        assert!(
            sanitized.contains(r#"<p title="safe">text</p>"#),
            "{sanitized}"
        );
        assert!(!sanitized.contains("onclick"));
    }

    #[wasm_bindgen_test]
    fn sanitize_html_removes_dangerous_container_tags() {
        let sanitized = sanitize_html(
            r#"<p>safe</p><style>.x{}</style><script>alert(1)</script><iframe><p>bad</p></iframe>"#,
        );
        assert!(sanitized.contains("<p>safe</p>"), "{sanitized}");
        assert!(!sanitized.contains("<style"));
        assert!(!sanitized.contains("<script"));
        assert!(!sanitized.contains("<iframe"));
    }

    #[wasm_bindgen_test]
    fn sanitize_html_keeps_safe_relative_links() {
        let sanitized = sanitize_html(r#"<a href="../notes?q=1#top">notes</a>"#);
        assert!(
            sanitized.contains(r#"<a href="../notes?q=1#top" rel="noopener noreferrer">notes</a>"#),
            "{sanitized}"
        );
    }

    #[wasm_bindgen_test]
    fn render_markdown_preserves_tables_task_lists_and_footnotes() {
        let rendered = render_markdown(
            "| A | B |\n| - | - |\n| 1 | 2 |\n\n- [x] done\n\nfootnote[^a]\n\n[^a]: note",
        );
        assert!(rendered.html.contains("<table>"), "{}", rendered.html);
        assert!(rendered.html.contains("<td>1</td>"), "{}", rendered.html);
        assert!(
            rendered
                .html
                .contains(r#"<input checked="" disabled="" type="checkbox">"#)
                || rendered
                    .html
                    .contains(r#"<input type="checkbox" checked="" disabled="">"#),
            "{}",
            rendered.html
        );
        assert!(
            rendered.html.contains("data-footnote-ref")
                && rendered.html.contains("data-footnote-backref"),
            "{}",
            rendered.html
        );
    }

    #[wasm_bindgen_test]
    fn rendered_raw_html_is_sanitized_before_metadata_extraction() {
        let rendered = rendered_from_html(sanitize_html(
            r#"<h2 id="safe">Safe <em>Title</em></h2><script>alert(1)</script><a href="javascript:alert(1)">x</a>"#,
        ));
        assert_eq!(rendered.outline.len(), 1);
        assert_eq!(rendered.outline[0].id, "safe");
        assert_eq!(rendered.outline[0].text, "Safe Title");
        assert!(!rendered.html.contains("<script"));
        assert!(!rendered.html.contains("javascript:"));
    }

    #[wasm_bindgen_test]
    fn render_markdown_strips_frontmatter() {
        let rendered = render_markdown("---\ndate: 2026-04-26\ntags: [math]\n---\n\n# Body\n");
        assert!(rendered.html.contains(r##"<a href="#body""##));
        assert!(rendered.html.contains(r#"id="body""#));
        assert!(!rendered.html.contains("date:"));
        assert!(!rendered.html.contains("tags:"));
    }

    #[wasm_bindgen_test]
    fn render_markdown_keeps_safe_links() {
        let html = render_markdown("Writing [tabula](/#/papers/tabula).").html;
        assert!(
            html.contains(r#"<a href="/#/papers/tabula" rel="noopener noreferrer">tabula</a>"#)
        );
    }

    #[wasm_bindgen_test]
    fn render_inline_markdown_keeps_links_without_paragraph_wrapper() {
        let html = render_inline_markdown("Writing [tabula](/#/papers/tabula).").html;
        assert!(
            html.contains(r#"<a href="/#/papers/tabula" rel="noopener noreferrer">tabula</a>"#)
        );
        assert!(!html.starts_with("<p>"));
    }

    #[wasm_bindgen_test]
    fn render_markdown_omits_raw_html_and_scripts() {
        let rendered = render_markdown(r#"<script>alert(1)</script><img src=x onerror=alert(2)>"#);
        assert!(!rendered.html.contains("<script"));
        assert!(!rendered.html.contains("onerror"));
    }

    #[wasm_bindgen_test]
    fn render_markdown_outputs_inline_math_placeholder() {
        let rendered = render_markdown("$E = mc^2$");
        assert!(rendered.has_math);
        assert!(rendered.html.contains(r#"data-math-style="inline""#));
        assert!(rendered.html.contains("E = mc^2"));
    }

    #[wasm_bindgen_test]
    fn render_markdown_outputs_display_math_placeholder() {
        let rendered = render_markdown("$$x^2$$");
        assert!(rendered.has_math);
        assert!(rendered.html.contains(r#"data-math-style="display""#));
        assert!(rendered.html.contains("x^2"));
    }

    #[wasm_bindgen_test]
    fn render_markdown_keeps_escaped_dollars_literal() {
        let rendered = render_markdown("Cost is \\$5.");
        assert!(!rendered.has_math);
        assert!(rendered.html.contains("Cost is $5."));
    }

    #[wasm_bindgen_test]
    fn hydrate_math_without_math_does_not_inject_katex_assets() {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .expect("document should be available");
        if let Some(node) = document.get_element_by_id("websh-katex-css") {
            node.remove();
        }
        if let Some(node) = document.get_element_by_id("websh-katex-js") {
            node.remove();
        }
        reset_katex_loader();

        let root = document.create_element("div").expect("div");
        root.set_inner_html("<p>plain text</p>");
        hydrate_math(&root);

        assert!(document.get_element_by_id("websh-katex-css").is_none());
        assert!(document.get_element_by_id("websh-katex-js").is_none());
    }

    #[wasm_bindgen_test]
    fn outline_collects_h2_h3_in_document_order() {
        let md = "# Title\n\n## Section A\n\nfoo\n\n### Sub A1\n\nbar\n\n## Section B\n";
        let rendered = render_markdown(md);
        let outline = &rendered.outline;
        assert_eq!(outline.len(), 3, "{:?}", outline);
        assert_eq!(outline[0].level, 2);
        assert_eq!(outline[0].text, "Section A");
        assert_eq!(outline[1].level, 3);
        assert_eq!(outline[1].text, "Sub A1");
        assert_eq!(outline[2].level, 2);
        assert_eq!(outline[2].text, "Section B");
    }

    #[wasm_bindgen_test]
    fn outline_skips_h1_and_h4_through_h6() {
        let md = "# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6\n";
        let rendered = render_markdown(md);
        let levels: Vec<u8> = rendered.outline.iter().map(|e| e.level).collect();
        assert_eq!(levels, vec![2, 3]);
    }

    #[wasm_bindgen_test]
    fn outline_empty_for_no_headings() {
        let rendered = render_markdown("Just a paragraph with no headings.\n");
        assert!(rendered.outline.is_empty());
    }

    #[wasm_bindgen_test]
    fn outline_strips_inline_formatting_from_text() {
        let md = "## With **bold** and `code` and *em*\n";
        let rendered = render_markdown(md);
        assert_eq!(rendered.outline.len(), 1);
        assert_eq!(rendered.outline[0].text, "With bold and code and em");
    }

    #[wasm_bindgen_test]
    fn outline_preserves_utf8_heading_text() {
        let md = "## Reth 2.0 이후, EVM execution이 다시 중요해졌다.\n\n### 효과와 한계\n";
        let rendered = render_markdown(md);
        assert_eq!(rendered.outline.len(), 2);
        assert_eq!(
            rendered.outline[0].text,
            "Reth 2.0 이후, EVM execution이 다시 중요해졌다."
        );
        assert_eq!(rendered.outline[1].text, "효과와 한계");
    }

    #[wasm_bindgen_test]
    fn outline_decodes_html_entities() {
        let md = "## Foo & Bar < Baz > Qux \"quoted\"\n";
        let rendered = render_markdown(md);
        assert_eq!(rendered.outline.len(), 1);
        assert_eq!(rendered.outline[0].text, "Foo & Bar < Baz > Qux \"quoted\"");
    }

    #[wasm_bindgen_test]
    fn outline_id_matches_anchor_link() {
        let md = "## Hello World\n";
        let rendered = render_markdown(md);
        assert_eq!(rendered.outline.len(), 1);
        let id = &rendered.outline[0].id;
        assert!(
            rendered.html.contains(&format!(r##"href="#{id}""##)),
            "id `{id}` should match an anchor href in the rendered HTML"
        );
    }
}
