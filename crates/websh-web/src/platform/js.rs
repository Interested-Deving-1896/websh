use wasm_bindgen::JsValue;

pub fn js_value_message(value: &JsValue) -> String {
    if let Some(message) = non_empty_js_string(value) {
        return message;
    }

    if let Ok(message) = js_sys::Reflect::get(value, &JsValue::from_str("message"))
        && let Some(message) = non_empty_js_string(&message)
    {
        return message;
    }

    "browser operation failed".to_string()
}

fn non_empty_js_string(value: &JsValue) -> Option<String> {
    value
        .as_string()
        .filter(|message| !message.trim().is_empty())
}
