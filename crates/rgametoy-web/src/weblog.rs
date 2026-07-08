//! Tiny console-logging helpers for the frontend's error paths. These write to
//! the browser console (dev-visible), never the user-facing `#status` bar — the
//! swallowed IDB / canvas / restore failures used to be invisible in DevTools.

use wasm_bindgen::JsValue;

/// Log an error message to the browser console.
pub fn error(msg: &str) {
    web_sys::console::error_1(&JsValue::from_str(msg));
}

/// Log an error message plus the JS value that carries the real detail
/// (a `DOMException`, thrown error, etc.).
pub fn error_val(context: &str, err: &JsValue) {
    web_sys::console::error_2(&JsValue::from_str(context), err);
}
