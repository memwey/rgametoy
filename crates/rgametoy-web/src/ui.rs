//! Tiny DOM helpers: get elements, set status text, attach event listeners
//! with a Rust closure. Centralised here so the hot loops in `wasm_host.rs`
//! stay focused on game state.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Event, EventTarget, HtmlElement};

use crate::canvas::get_element_by_id;

pub fn get_html_element(doc: &Document, id: &str) -> Result<HtmlElement, JsValue> {
    get_element_by_id::<HtmlElement>(doc, id)
}

/// Set the inner text of an element, e.g. the status line.
pub fn set_text(el: &HtmlElement, text: &str) {
    el.set_text_content(Some(text));
}

/// Attach an event listener that runs `f` whenever `target` fires
/// `event_name`. The closure is stored in `slot` so the borrow stays valid
/// as long as the host needs it.
#[allow(dead_code)]
pub fn add_listener<F>(
    target: &EventTarget,
    event_name: &str,
    f: F,
    slot: &mut Option<Closure<dyn FnMut(Event)>>,
) -> Result<(), JsValue>
where
    F: FnMut(Event) + 'static,
{
    let closure = Closure::wrap(Box::new(f) as Box<dyn FnMut(Event)>);
    target.add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref())?;
    *slot = Some(closure);
    Ok(())
}

/// Convenience: same as [`add_listener`], but for `() -> ()` listeners (no
/// event payload needed). Used for the "Load ROM" / "Pause" buttons.
#[allow(dead_code)]
pub fn add_unit_listener<F>(
    target: &EventTarget,
    event_name: &str,
    f: F,
    slot: &mut Option<Closure<dyn FnMut()>>,
) -> Result<(), JsValue>
where
    F: FnMut() + 'static,
{
    let closure = Closure::wrap(Box::new(f) as Box<dyn FnMut()>);
    target.add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref())?;
    *slot = Some(closure);
    Ok(())
}
