//! Wasm-side host glue. The `WasmHost` newtype wraps a `Rc<RefCell<Inner>>`
//! and is the only type the JS side talks to. Inner is shared with the rAF
//! loop closure (and the input/button listeners) via an `Rc` clone captured by
//! each closure. The rAF loop is the sole driver of emulation; audio is a
//! downstream sink (`audio::AudioPlayer`) fed each frame — it does not run the
//! console, so no closure needs to re-borrow `Inner` from the audio thread.
//!
//! This module holds the shared state (`Inner`), the per-tick emulation body
//! (`step_and_present`), and the `WasmHost` API the JS side calls. The DOM
//! event wiring, the ROM/save persistence flow, and the audio toggle each
//! live in a child module (`wiring`, `saves`, `audio_toggle`); they `impl`
//! the types defined here, which Rust allows within the same crate.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use rgametoy_core::Console;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, EventTarget, HtmlCanvasElement, KeyboardEvent};

use crate::audio::AudioPlayer;
use crate::canvas::{get_canvas, Presenter, RGBA_LEN};
use crate::input::{from_keydown, InputState, TURBO_KEY};
use crate::pacing::{frames_to_run, DMG_FRAME_MS, MAX_CATCHUP_MS, MAX_FRAMES_PER_TICK};
use crate::palette::{rgba_table, shade_to_rgba, PALETTES};
use crate::ui::{get_html_element, set_text};
use crate::weblog;
use web_sys::IdbDatabase;

use saves::restore_save;

mod audio_toggle;
mod saves;
mod wiring;

/// Fast-forward multiplier: emulation runs this many times real time while the
/// turbo key is held. Matches the desktop frontend's default. Paced against the
/// wall clock (not per repaint), so it's a consistent 4× on any display.
const TURBO_SPEED: f64 = 4.0;

/// Per-tick frame cap for fast-forward — generous enough that 4× never binds at
/// common refresh rates, low enough that a stall can't avalanche.
const TURBO_MAX_FRAMES_PER_TICK: u32 = 12;

/// How many rAF ticks between auto-saves of battery-backed RAM. 120 frames
/// is 2 s at 60 Hz — slow enough not to thrash the disk, fast enough that
/// a tab crash loses at most ~2 s of progress.
const AUTOSAVE_DEBOUNCE: u64 = 120;

/// T-cycles in one DMG frame — used to turn the core's `total_cycles` delta
/// into an emulated frame count for the fps readout.
const CYCLES_PER_FRAME: f64 = 70224.0;

/// Register a page-lifetime event listener whose handler receives the event,
/// leaking the closure so it stays valid for the document's lifetime. Every
/// listener wired here is set once at startup and never removed, so `forget()`
/// is a bounded one-time leak — the same lifetime the old `Inner::*_closure`
/// keep-alive fields provided, without the per-listener bookkeeping.
fn on_event<E: FromWasmAbi + 'static>(
    target: &impl AsRef<EventTarget>,
    event: &str,
    cb: impl FnMut(E) + 'static,
) -> Result<(), JsValue> {
    let closure = Closure::wrap(Box::new(cb) as Box<dyn FnMut(E)>);
    target
        .as_ref()
        .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

/// [`on_event`] for a `click` handler that ignores the event object.
fn on_click(target: &impl AsRef<EventTarget>, cb: impl FnMut() + 'static) -> Result<(), JsValue> {
    let closure = Closure::wrap(Box::new(cb) as Box<dyn FnMut()>);
    target
        .as_ref()
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

/// The requestAnimationFrame closure slot: kept in an `Rc<RefCell>` so the
/// closure can re-arm itself each frame without being dropped.
type RafSlot = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

/// All mutable state lives in here. The rAF closure (and, in Phase 5, the
/// audio closure) each hold a clone of the surrounding `Rc<RefCell<Inner>>`
/// so they can call `borrow_mut()` and reach every field.
pub struct Inner {
    pub console: Console,
    #[allow(dead_code)]
    pub canvas: HtmlCanvasElement,
    pub ctx: CanvasRenderingContext2d,
    /// Reusable blitter (persistent ImageData) that pushes `rgba_buf` to `ctx`.
    pub presenter: Presenter,
    pub palette_idx: usize,
    pub rgba_buf: Vec<u8>,
    pub paused: bool,
    pub has_rom: bool,
    pub title: String,
    /// Live keyboard state: every `event.code` currently held down. We
    /// recompute the joypad byte each frame so autorepeat doesn't matter
    /// and a key release always takes effect immediately.
    pub keys_down: BTreeSet<String>,
    /// Joypad byte injected by JS via `WasmHost::set_buttons` (0 = pressed).
    /// `0xFF` (all released) when JS isn't driving input. Merged with the
    /// keyboard each frame so touch / external controls work alongside keys.
    pub js_buttons: u8,
    /// Edge-triggered meta keys, set in the keydown listener, cleared after
    /// the rAF loop has acted on them. Only one of each fires per press.
    pub save_pending: bool,
    pub load_pending: bool,
    pub screenshot_pending: bool,
    pub palette_pending: bool,
    pub reset_pending: bool,
    /// In-memory mirror of the persisted quick-state slot, so the debounced
    /// battery-RAM autosave can rewrite the record without clobbering the
    /// user's saved state.
    pub quick_state: Option<Vec<u8>>,
    /// Set once the user has enabled audio. When true, the rAF loop feeds each
    /// frame's APU output to the worklet sink; the rAF loop always drives
    /// emulation either way (audio no longer drives it).
    pub audio_enabled: bool,
    /// The audio graph (AudioContext + AudioWorklet sink), built lazily on first
    /// enable and kept for the page's lifetime. `None` until then.
    pub audio: Option<AudioPlayer>,
    /// IndexedDB connection. `None` until `onsuccess` fires (or
    /// permanently if the open errored). Shared with the IDB callbacks
    /// so the slot is filled in async — the host never blocks on IDB.
    pub storage: Rc<RefCell<Option<IdbDatabase>>>,
    /// Back-reference to the `Rc<RefCell<Inner>>` that owns us. Stored
    /// here so the IDB / audio / rAF closures can re-borrow `Inner`
    /// without needing the caller to pass another `Rc` clone through
    /// every call site.
    pub host_rc: Option<Rc<RefCell<Inner>>>,
    /// FNV-1a 32-bit hex of the loaded ROM — the IDB key for the save
    /// record. Set to empty string before any ROM is loaded.
    pub rom_hash: String,
    /// A ROM hash whose save couldn't be restored yet because IDB wasn't open
    /// when it loaded. When IDB's `onsuccess` fires, if this still equals the
    /// current ROM, its save is restored (a ROM loaded before IDB was ready
    /// would otherwise start from an empty save forever).
    pub pending_restore: Option<String>,
    /// Counter that ticks once per rAF loop. Every [`AUTOSAVE_DEBOUNCE`]
    /// frames we flush a dirty battery-RAM record back to IDB.
    pub frame_idx: u64,
    /// True emulation rate: `total_cycles` at the last `#fps` update and the
    /// wall-clock time of it. We derive fps from the *emulated* cycles elapsed
    /// (works whether the rAF loop or the audio callback is driving), not from
    /// how often we repaint — a 144 Hz panel repaints 144×/s but the DMG still
    /// runs at ~59.7.
    pub fps_cycles_ref: u64,
    pub fps_last_ms: f64,
    /// Real-time frame pacing (rAF-driven path only): accumulated real
    /// milliseconds not yet spent on emulated frames, and the previous tick's
    /// timestamp. Decouples the emulated ~59.7 Hz from the display refresh.
    pub frame_accum_ms: f64,
    pub last_tick_ms: f64,

    // Closures that must outlive any single JS callback but can't just be
    // `forget()`-leaked because they're re-created after startup: the rAF
    // closure re-arms itself through this slot, and the ROM `FileReader`
    // handler is rebuilt on every file pick. The one-shot startup listeners
    // (buttons, keyboard) are registered via `on_click`/`on_event`, which
    // leak their closure once instead of parking it in a field here.
    pub raf_slot: RafSlot,
    pub rom_reader_closure: Option<Closure<dyn FnMut(web_sys::Event)>>,
}

impl Inner {
    fn new() -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let doc = window
            .document()
            .ok_or_else(|| JsValue::from_str("no document"))?;
        let (canvas, ctx) = get_canvas(&doc)?;
        let presenter = Presenter::new()?;
        Ok(Inner {
            console: Console::new(),
            canvas,
            ctx,
            presenter,
            palette_idx: 0,
            rgba_buf: vec![0; RGBA_LEN],
            paused: false,
            has_rom: false,
            title: String::from("(no ROM loaded)"),
            keys_down: BTreeSet::new(),
            js_buttons: 0xFF,
            save_pending: false,
            load_pending: false,
            screenshot_pending: false,
            palette_pending: false,
            reset_pending: false,
            quick_state: None,
            audio_enabled: false,
            audio: None,
            storage: Rc::new(RefCell::new(None)),
            host_rc: None,
            rom_hash: String::new(),
            pending_restore: None,
            frame_idx: 0,
            fps_cycles_ref: 0,
            fps_last_ms: 0.0,
            frame_accum_ms: 0.0,
            last_tick_ms: 0.0,
            raf_slot: Rc::new(RefCell::new(None)),
            rom_reader_closure: None,
        })
    }

    /// Apply a keydown event: swallow the browser's default for any key the
    /// emulator uses, then track it and queue any edge-detected meta action.
    fn on_keydown(&mut self, ev: &KeyboardEvent) {
        let code = ev.code();
        let s: InputState = from_keydown(&code);
        // Keep the browser from *also* acting on a key we consume — arrows and
        // Space would scroll, the meta digits would type. Done on every keydown
        // (including autorepeat) so a held key never leaks a default action.
        // Unmapped keys (Tab, F5, …) fall through to the browser untouched.
        let handled =
            s.buttons != 0xFF || s.turbo || s.save || s.load || s.screenshot || s.palette_cycle;
        if handled {
            ev.prevent_default();
        }
        // Edge-detect the meta keys off the *first* keydown only; autorepeat
        // (key already in the set) must not re-fire save/load/etc.
        if self.keys_down.insert(code) {
            if s.save {
                self.save_pending = true;
            }
            if s.load {
                self.load_pending = true;
            }
            if s.screenshot {
                self.screenshot_pending = true;
            }
            if s.palette_cycle {
                self.palette_pending = true;
            }
        }
    }

    fn on_keyup(&mut self, ev: &KeyboardEvent) {
        self.keys_down.remove(&ev.code());
    }

    /// Drop every key — called on window blur so a user tabbing out and
    /// back doesn't leave a phantom direction held down.
    fn on_blur(&mut self) {
        self.keys_down.clear();
    }

    /// Roll the live key set into a single [`InputState`] and apply it to
    /// the console. Called once per rAF tick before stepping any frames.
    fn apply_input(&mut self) {
        let mut state = InputState {
            buttons: 0xFF,
            ..Default::default()
        };
        for code in &self.keys_down {
            let s = from_keydown(code);
            state.buttons &= s.buttons;
            state.turbo |= s.turbo;
        }
        // Merge JS-injected buttons (touch / external controls): a bit is
        // pressed (0) if either the keyboard or JS presses it.
        state.buttons &= self.js_buttons;
        self.console.set_buttons(state.buttons);
    }

    /// Run a single frame and blit it to the canvas. Silent if no ROM is
    /// loaded — the framebuffer is zero, which paints as palette shade 0
    /// (the lightest colour). Any error from `putImageData` is swallowed
    /// because if the canvas was removed, the rAF loop will error too.
    ///
    /// When audio is enabled, the audio callback drives the emulator
    /// forward; this method only repaints the current framebuffer so
    /// visual and audio stay in lock-step.
    fn step_and_present(&mut self) {
        // Drain the meta edge queue before stepping, so a Digit5 press
        // before the next rAF tick still lands.
        if self.save_pending {
            self.save_pending = false;
            let bytes = self.console.save_state_bytes();
            self.quick_state = Some(bytes.clone());
            self.set_status("saved state");
            self.write_quick_state_to_storage(bytes);
        }
        if self.load_pending {
            self.load_pending = false;
            self.load_quick_state();
        }
        if self.screenshot_pending {
            self.screenshot_pending = false;
            match crate::screenshot::download(&self.rgba_buf) {
                Ok(()) => self.set_status("screenshot saved"),
                Err(e) => {
                    weblog::error_val("screenshot download failed", &e);
                    self.set_status("screenshot failed");
                }
            }
        }
        if self.palette_pending {
            self.palette_pending = false;
            self.palette_idx = (self.palette_idx + 1) % PALETTES.len();
            let name = PALETTES[self.palette_idx].0;
            self.set_status(&format!("palette: {name}"));
        }
        if self.reset_pending {
            self.reset_pending = false;
            self.power_cycle();
        }

        self.apply_input();
        if !self.paused && self.has_rom {
            // Single drive model: the rAF loop always advances the console,
            // paced against the wall clock (frames per *real* time, not per
            // refresh — so a 120/144 Hz panel doesn't run fast). Audio, when on,
            // is a pure downstream sink fed each frame (see below); it no longer
            // drives emulation. Fast-forward is the same loop at TURBO_SPEED.
            let now = js_sys::Date::now();
            let dt = if self.last_tick_ms == 0.0 {
                DMG_FRAME_MS
            } else {
                (now - self.last_tick_ms).min(MAX_CATCHUP_MS)
            };
            let turbo = self.keys_down.contains(TURBO_KEY);
            let (n, accum) = if turbo {
                frames_to_run(
                    self.frame_accum_ms,
                    dt * TURBO_SPEED,
                    TURBO_MAX_FRAMES_PER_TICK,
                )
            } else {
                frames_to_run(self.frame_accum_ms, dt, MAX_FRAMES_PER_TICK)
            };
            self.frame_accum_ms = accum;
            for _ in 0..n {
                self.console.run_frame();
            }
            self.last_tick_ms = now;

            // Drain the APU every frame (keeps its buffer from filling its cap).
            // Feed the worklet only at 1× with audio on; while fast-forwarding we
            // drop it — fast-forward is muted, matching desktop.
            let samples = self.console.take_audio_samples();
            if self.audio_enabled && !turbo {
                if let Some(player) = self.audio.as_mut() {
                    player.feed(&samples);
                }
            }
        }
        let fb = self.console.framebuffer();
        let table = rgba_table(self.palette_idx);
        shade_to_rgba(fb, table, &mut self.rgba_buf);
        let _ = self.presenter.blit(&self.ctx, &self.rgba_buf);

        // Debounced auto-save: on the interval, flush battery RAM to IDB *only
        // when the game has actually written to it* since the last flush — an
        // event-driven trigger, not a blind timer — then clear the dirty flag.
        if self.has_rom
            && self.frame_idx.is_multiple_of(AUTOSAVE_DEBOUNCE)
            && self.console.cartridge().ram_dirty()
        {
            self.persist_record();
        }
        self.frame_idx = self.frame_idx.wrapping_add(1);

        // FPS: report the *emulation* rate, recomputed once per wall-clock
        // second from the emulated cycles elapsed. This reads ~60 whether the
        // rAF loop or the audio callback is driving, and doesn't track the
        // display's refresh rate the way a per-repaint counter would.
        let now = js_sys::Date::now();
        if self.fps_last_ms == 0.0 {
            self.fps_last_ms = now;
            self.fps_cycles_ref = self.console.total_cycles();
        } else if now - self.fps_last_ms >= 1000.0 {
            let cycles = self.console.total_cycles();
            let frames = cycles.saturating_sub(self.fps_cycles_ref) as f64 / CYCLES_PER_FRAME;
            let secs = (now - self.fps_last_ms) / 1000.0;
            let fps = (frames / secs).round() as u32;
            self.fps_last_ms = now;
            self.fps_cycles_ref = cycles;
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                if let Ok(el) = get_html_element(&doc, "fps") {
                    set_text(&el, &format!("{fps} fps"));
                }
            }
        }
    }

    /// Write to `#status`. Looks up the element each call so we can call
    /// from anywhere (rAF loop, audio callback, button handler) without
    /// having to pass the element through. Also logs to the JS console for
    /// headless verification.
    fn set_status(&self, text: &str) {
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            if let Ok(el) = get_html_element(&doc, "status") {
                set_text(&el, text);
            }
        }
        web_sys::console::log_1(&text.into());
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub struct WasmHost {
    inner: Rc<RefCell<Inner>>,
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl WasmHost {
    /// Construct a host. Looks up `#screen` in the document, sets up a 2D
    /// context sized 160×144, starts the rAF loop, and wires the "Load
    /// ROM" button. The loop runs even before a ROM is loaded — it just
    /// paints a blank (shade 0) frame.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<WasmHost, JsValue> {
        let inner = Inner::new()?;
        let host = WasmHost {
            inner: Rc::new(RefCell::new(inner)),
        };
        // `host.inner.borrow_mut()` and `borrow()` reentrantly: the
        // temporary `None` we put in `Inner::new` gets replaced as soon
        // as `Rc<RefCell<Inner>>` is fully constructed. The `host_rc`
        // back-reference is what the IDB / audio / rAF closures use to
        // re-enter `Inner` later.
        {
            let rc = host.inner.clone();
            host.inner.borrow_mut().host_rc = Some(rc);
        }
        // Open IDB asynchronously. We *do not* busy-wait for it; the
        // emulator runs from the first frame, and the persistence slot
        // gets filled in by the next event-loop turn.
        {
            let storage_slot = host.inner.borrow().storage.clone();
            let host_rc = host.inner.clone();
            let on_unavailable = move || {
                if let Ok(h) = host_rc.try_borrow_mut() {
                    h.set_status("IDB unavailable; saves won't persist");
                }
            };
            // Retry a restore that had to be deferred because a ROM loaded
            // before IDB finished opening — but only if it's still the live ROM.
            let host_rc2 = host.inner.clone();
            let on_ready = move || {
                let pending = {
                    let Ok(mut inner) = host_rc2.try_borrow_mut() else {
                        return;
                    };
                    match inner.pending_restore.take() {
                        Some(h) if inner.rom_hash == h => Some((h, inner.storage.clone())),
                        _ => None,
                    }
                };
                if let Some((hash, storage)) = pending {
                    restore_save(&host_rc2, &storage, hash);
                }
            };
            if let Err(e) = crate::storage::init_async(storage_slot, on_unavailable, on_ready) {
                web_sys::console::warn_1(&e);
            }
        }
        host.start_render_loop()?;
        host.wire_load_button()?;
        host.wire_keyboard()?;
        host.wire_control_buttons()?;
        Ok(host)
    }

    /// Load a ROM from a `Uint8Array` of bytes. On failure, writes to
    /// `#status` and returns `Ok(())` so the JS side can ignore the
    /// outcome (the user already saw the message).
    pub fn load_rom(&self, bytes: &[u8]) -> Result<(), JsValue> {
        self.inner.borrow_mut().load_rom(bytes);
        Ok(())
    }

    pub fn step_frame(&self) {
        self.inner.borrow_mut().step_and_present();
    }

    pub fn framebuffer(&self) -> js_sys::Uint8Array {
        let inner = self.inner.borrow();
        js_sys::Uint8Array::from(inner.console.framebuffer())
    }

    /// Inject a joypad byte from JS (0 = pressed) — e.g. on-screen / touch
    /// controls. It's stored, not written straight to the console: the rAF loop
    /// merges it with the keyboard each frame (`apply_input`), so a direct
    /// console write would just be overwritten. `0xFF` releases all JS buttons.
    pub fn set_buttons(&self, mask: u8) {
        self.inner.borrow_mut().js_buttons = mask;
    }

    pub fn get_buttons(&self) -> u8 {
        // There's no public getter on the console's P1; report the merged
        // keyboard + JS state (what `apply_input` feeds the console) so a
        // JS-side debug overlay shows what's actually pressed.
        let inner = self.inner.borrow();
        let mut buttons = inner.js_buttons;
        for code in &inner.keys_down {
            buttons &= from_keydown(code).buttons;
        }
        buttons
    }

    pub fn set_palette(&self, idx: u8) {
        let mut inner = self.inner.borrow_mut();
        let n = PALETTES.len();
        inner.palette_idx = (idx as usize) % n;
        let name = PALETTES[inner.palette_idx].0;
        inner.set_status(&format!("palette: {name}"));
    }

    pub fn get_palette(&self) -> u8 {
        self.inner.borrow().palette_idx as u8
    }

    pub fn set_paused(&self, paused: bool) {
        self.inner.borrow_mut().paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.inner.borrow().paused
    }

    pub fn save_state_bytes(&self) -> js_sys::Uint8Array {
        let inner = self.inner.borrow();
        js_sys::Uint8Array::from(inner.console.save_state_bytes().as_slice())
    }

    pub fn load_state_bytes(&self, bytes: &[u8]) -> Result<(), JsValue> {
        let mut inner = self.inner.borrow_mut();
        match inner.console.load_state_bytes(bytes) {
            Ok(()) => {
                inner.paused = false;
                inner.set_status("loaded state");
                Ok(())
            }
            Err(e) => {
                inner.set_status(&format!("state error: {e}"));
                Err(JsValue::from_str(&format!("{e}")))
            }
        }
    }
}
