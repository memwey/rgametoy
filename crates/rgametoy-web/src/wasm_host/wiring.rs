//! DOM event wiring for the host: the rAF render loop, the "Load ROM" button
//! and file picker, the keyboard listeners, and the toolbar buttons. Each
//! handler is a small closure that borrows `Inner` and flips state; the rAF
//! loop (`wasm_host`) is what acts on it.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Event, HtmlInputElement, KeyboardEvent};

use crate::canvas::get_element_by_id;
use crate::ui::get_html_element;
use crate::weblog;

use super::audio_toggle::{start_audio, stop_audio};
use super::{on_click, on_event, Inner, RafSlot, WasmHost};

impl WasmHost {
    /// Wire the "Load ROM" button: clicking it opens a hidden `<input
    /// type="file">`; on `change` we read the file as ArrayBuffer and hand
    /// the bytes to `Inner::load_rom`.
    pub(super) fn wire_load_button(&self) -> Result<(), JsValue> {
        let doc = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| JsValue::from_str("no document"))?;
        let button = get_html_element(&doc, "load-button")?;
        let file_input: HtmlInputElement = get_element_by_id(&doc, "rom-input")?;

        // Click on #load-button → .click() on the hidden #rom-input. The
        // input's onchange handler then runs as if the user had picked a
        // file from the OS file dialog.
        let input_for_click: HtmlInputElement = file_input.clone();
        on_click(&button, move || input_for_click.click())?;

        // Change on #rom-input → read file → load_rom.
        let host_for_change: Rc<RefCell<Inner>> = self.inner.clone();
        let file_for_change: HtmlInputElement = file_input.clone();
        on_event(&file_input, "change", move |_event: Event| {
            let files = match file_for_change.files() {
                Some(f) => f,
                None => return,
            };
            if files.length() == 0 {
                return;
            }
            let file = match files.get(0) {
                Some(f) => f,
                None => return,
            };
            let reader = web_sys::FileReader::new().unwrap();
            let host_for_reader = host_for_change.clone();
            let reader_closure = Closure::wrap(Box::new(move |ev: Event| {
                let reader: web_sys::FileReader = match ev
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::FileReader>().ok())
                {
                    Some(r) => r,
                    None => return,
                };
                let buffer = match reader.result() {
                    Ok(b) => b,
                    Err(_) => return,
                };
                let array: js_sys::ArrayBuffer = match buffer.dyn_into() {
                    Ok(a) => a,
                    Err(_) => return,
                };
                let bytes = js_sys::Uint8Array::new(&array);
                let mut vec = vec![0u8; bytes.length() as usize];
                bytes.copy_to(&mut vec);
                host_for_reader.borrow_mut().load_rom(&vec);
            }) as Box<dyn FnMut(Event)>);
            reader
                .add_event_listener_with_callback("load", reader_closure.as_ref().unchecked_ref())
                .unwrap();
            // Unlike the one-shot startup listeners, this reader is rebuilt on
            // every file pick, so it's parked in a field (and replaced next
            // time) rather than leaked — otherwise each ROM load would leak a
            // closure. Dropping it would invalidate the callback before `load`.
            host_for_change.borrow_mut().rom_reader_closure = Some(reader_closure);

            if let Err(e) = reader.read_as_array_buffer(&file) {
                weblog::error_val("could not read the selected ROM file", &e);
            }
        })?;
        Ok(())
    }

    /// Start the requestAnimationFrame loop. Each tick advances the console
    /// by one frame and blits the framebuffer to the canvas.
    pub(super) fn start_render_loop(&self) -> Result<(), JsValue> {
        let inner = self.inner.clone();
        let raf_slot: RafSlot = Rc::new(RefCell::new(None));
        let slot_for_cb = raf_slot.clone();
        *raf_slot.borrow_mut() = Some(Closure::wrap(Box::new(move || {
            // Drain any queued meta actions before stepping, so a save
            // requested on the previous frame's idle still lands. The
            // actual save/load/screenshot handlers live on `WasmHost`; we
            // need a second `Rc<RefCell>` clone for them.
            {
                let mut inner = inner.borrow_mut();
                inner.step_and_present();
            }
            let window = match web_sys::window() {
                Some(w) => w,
                None => return,
            };
            let slot = slot_for_cb.borrow();
            if let Some(c) = slot.as_ref() {
                let _ = window.request_animation_frame(c.as_ref().unchecked_ref());
            }
        }) as Box<dyn FnMut()>));
        // Stash the slot in Inner so the closure is dropped only with the
        // host (i.e. never, for the page's lifetime).
        self.inner.borrow_mut().raf_slot = raf_slot.clone();

        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let slot = raf_slot.borrow();
        let closure = slot
            .as_ref()
            .ok_or_else(|| JsValue::from_str("raf closure not set"))?;
        window.request_animation_frame(closure.as_ref().unchecked_ref())?;
        Ok(())
    }

    /// Bind keydown/keyup to the window, and a blur handler that drops
    /// every key. The blur matters because if the user alt-tabs out with a
    /// direction held, the browser won't fire `keyup` for it, and the
    /// emulated character would keep walking.
    pub(super) fn wire_keyboard(&self) -> Result<(), JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let host_for_kd: Rc<RefCell<Inner>> = self.inner.clone();
        on_event(&window, "keydown", move |ev: KeyboardEvent| {
            host_for_kd.borrow_mut().on_keydown(&ev);
        })?;

        let host_for_ku: Rc<RefCell<Inner>> = self.inner.clone();
        on_event(&window, "keyup", move |ev: KeyboardEvent| {
            host_for_ku.borrow_mut().on_keyup(&ev);
        })?;

        let host_for_blur: Rc<RefCell<Inner>> = self.inner.clone();
        on_event(&window, "blur", move |_ev: Event| {
            host_for_blur.borrow_mut().on_blur();
        })?;
        Ok(())
    }

    /// Wire the small toolbar buttons (Pause, Reset, Cycle palette,
    /// Screenshot, Enable audio). Each is a one-line `click` handler
    /// that flips a flag on `Inner`; the rAF loop acts on the flag.
    /// The audio button is the odd one out: it's the only one that
    /// needs to mutate its own label after the first click.
    pub(super) fn wire_control_buttons(&self) -> Result<(), JsValue> {
        let doc = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| JsValue::from_str("no document"))?;

        // Pause — toggles the `paused` field. The rAF loop still runs
        // (so the canvas keeps repainting), it just skips `run_frame`.
        let host_pause: Rc<RefCell<Inner>> = self.inner.clone();
        on_click(&get_html_element(&doc, "pause-button")?, move || {
            let mut h = host_pause.borrow_mut();
            h.paused = !h.paused;
            let state = if h.paused { "paused" } else { "running" };
            h.set_status(state);
        })?;

        // Reset — same effect as the DMG power-on sequence.
        let host_reset: Rc<RefCell<Inner>> = self.inner.clone();
        on_click(&get_html_element(&doc, "reset-button")?, move || {
            host_reset.borrow_mut().reset_pending = true;
        })?;

        // Cycle palette — same as pressing Digit3.
        let host_pal: Rc<RefCell<Inner>> = self.inner.clone();
        on_click(&get_html_element(&doc, "palette-button")?, move || {
            host_pal.borrow_mut().palette_pending = true;
        })?;

        // Screenshot — same as pressing Digit2.
        let host_shot: Rc<RefCell<Inner>> = self.inner.clone();
        on_click(&get_html_element(&doc, "screenshot-button")?, move || {
            host_shot.borrow_mut().screenshot_pending = true;
        })?;

        // Audio toggle. Reuses a single AudioContext across toggles (built once,
        // asynchronously, on first enable — Web Audio requires a user gesture),
        // then suspend/resume. See `start_audio` / `stop_audio`.
        let host_audio: Rc<RefCell<Inner>> = self.inner.clone();
        let audio_btn: web_sys::HtmlElement = get_html_element(&doc, "audio-button")?;
        let audio_btn_for_cb: web_sys::HtmlElement = audio_btn.clone();
        on_click(&audio_btn, move || {
            let enabled = host_audio.borrow().audio_enabled;
            let btn = Some(audio_btn_for_cb.clone());
            if enabled {
                stop_audio(&host_audio, btn);
            } else {
                start_audio(&host_audio, btn);
            }
        })?;
        Ok(())
    }
}
