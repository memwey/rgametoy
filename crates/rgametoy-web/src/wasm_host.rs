//! Wasm-side host glue. The `WasmHost` newtype wraps a `Rc<RefCell<Inner>>`
//! and is the only type the JS side talks to. Inner is shared with the rAF
//! loop closure (and the input/button listeners) via an `Rc` clone captured by
//! each closure. The rAF loop is the sole driver of emulation; audio is a
//! downstream sink (`audio::AudioPlayer`) fed each frame — it does not run the
//! console, so no closure needs to re-borrow `Inner` from the audio thread.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::Console;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    CanvasRenderingContext2d, Event, EventTarget, HtmlCanvasElement, HtmlInputElement,
    KeyboardEvent,
};

use crate::audio::AudioPlayer;
use crate::canvas::{get_canvas, get_element_by_id, Presenter, RGBA_LEN};
use crate::input::{from_keydown, InputState, TURBO_KEY};
use crate::pacing::{frames_to_run, DMG_FRAME_MS, MAX_CATCHUP_MS, MAX_FRAMES_PER_TICK};
use crate::palette::{rgba_table, shade_to_rgba, PALETTES};
use crate::rom::{cartridge_title, load_rom};
use crate::storage::{rom_hash, SaveRecord};
use crate::ui::{get_html_element, set_text};
use crate::weblog;
use web_sys::IdbDatabase;

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
fn on_click(
    target: &impl AsRef<EventTarget>,
    cb: impl FnMut() + 'static,
) -> Result<(), JsValue> {
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
    pub rom_reader_closure: Option<Closure<dyn FnMut(Event)>>,
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

    /// Install a ROM from raw bytes. The frontend already sanity-checks the
    /// header (`rom::is_supported_type`); any further failure is reported
    /// into `#status` so the user sees it without having to open DevTools.
    /// On success, also looks up the matching IDB save record (if any) and
    /// restores its RAM / quick-state into the console. The IDB read is
    /// fire-and-forget — the user sees "loaded: <title>" immediately and
    /// the restored-save status message lands a tick later.
    fn load_rom(&mut self, bytes: &[u8]) {
        let cart: Cartridge = match load_rom(bytes.to_vec()) {
            Ok(c) => c,
            Err(e) => {
                let msg = e.as_string().unwrap_or_else(|| "load failed".to_string());
                self.set_status(&format!("ROM error: {msg}"));
                return;
            }
        };
        let title = cartridge_title(bytes);
        self.title = title.clone();
        self.console.power_on(cart);
        self.has_rom = true;
        self.paused = false;

        // Compute the content hash and look up an existing save record.
        // The lookup is async: the user sees the ROM running first, and
        // the saved RAM / state land a tick later.
        self.rom_hash = rom_hash(bytes);
        self.set_status(&format!("loaded: {title} ({} KB)", bytes.len() / 1024));

        // Look up and restore this ROM's save. If IDB is already open, do it
        // now; otherwise defer — the IDB-ready hook retries (P2). Either way the
        // restore is hash-guarded so a late reply can't land on a newer ROM.
        let Some(host_rc) = self.host_rc.clone() else { return };
        if self.storage.borrow().is_some() {
            restore_save(&host_rc, &self.storage, self.rom_hash.clone());
        } else {
            self.pending_restore = Some(self.rom_hash.clone());
        }
    }

    /// Reboot the current ROM in place. Used by the Reset button: the
    /// cartridge is kept, but the bus is reloaded and CPU / PPU / APU
    /// state is reset to the post-boot values. Saved RAM and the
    /// in-memory title are not touched.
    fn reset(&mut self) {
        if !self.has_rom {
            self.set_status("nothing to reset");
            return;
        }
        // Reboot in place: the inserted cartridge stays; only the CPU / PPU /
        // APU / timer return to their post-boot state. Saved RAM is untouched.
        self.console.reset();
        self.paused = false;
        self.set_status("reset");
    }

    /// Persist the current battery RAM to IDB. Called from the rAF loop
    /// on a debounced cadence, and from the save/load hotkeys (5/7) so
    /// the user-visible slot updates immediately.
    fn persist_record(&mut self) {
        if self.rom_hash.is_empty() {
            return;
        }
        let ram = self.console.cartridge().ram().to_vec();
        // Only write if the cartridge actually has RAM — saves with no
        // external RAM are pointless, and the dirty-flag check (a separate code
        // path) would not be triggered anyway. Log to the console (not the
        // status bar — this is a ~2 s background flush, not a user action).
        let ram = if ram.is_empty() {
            None
        } else {
            web_sys::console::log_1(
                &format!("battery saved ({} KiB)", ram.len() / 1024).into(),
            );
            Some(ram)
        };
        let record = SaveRecord {
            rom_hash: self.rom_hash.clone(),
            rom_title: self.title.clone(),
            ram,
            // Preserve the saved quick-state slot: autosave rewrites the whole
            // record, so writing `None` here would wipe the user's quick-save.
            quick_state: self.quick_state.clone(),
            updated_at: js_sys::Date::now(),
        };
        crate::storage::put_record_async(&self.storage.borrow(), record);
    }

    /// Update the quick-state slot in IDB. `Some(bytes)` saves the slot
    /// with that data; `None` loads the slot from IDB into the console.
    fn write_quick_state_to_storage(&mut self, bytes: Vec<u8>) {
        if self.rom_hash.is_empty() {
            return;
        }
        let record = SaveRecord {
            rom_hash: self.rom_hash.clone(),
            rom_title: self.title.clone(),
            ram: self.console.cartridge().ram().to_vec().into(),
            quick_state: Some(bytes),
            updated_at: js_sys::Date::now(),
        };
        crate::storage::put_record_async(&self.storage.borrow(), record);
    }

    /// Load the quick-state slot. Prefers the in-memory copy: it's synchronous
    /// and repeatable (same as the desktop frontend), so a second load of the
    /// same save works — routing every load through an async IDB read made it
    /// fragile, seeming to only work once. IDB is a fallback for the one case
    /// the in-memory slot can't cover: a fresh page load before the slot has
    /// been populated. The IDB result is cached in memory so the next load is
    /// instant.
    fn load_quick_state(&mut self) {
        if let Some(qs) = self.quick_state.clone() {
            match self.console.load_state_bytes(&qs) {
                Ok(()) => self.set_status("loaded state"),
                Err(e) => self.set_status(&format!("state error: {e}")),
            }
            return;
        }
        if self.rom_hash.is_empty() {
            self.set_status("no save slot for this ROM");
            return;
        }
        self.set_status("loading state…");
        let Some(host_rc) = self.host_rc.clone() else { return };
        let storage = self.storage.clone();
        let hash = self.rom_hash.clone();
        crate::storage::get_record_async(&storage.borrow(), hash, move |rec| {
            let Ok(mut inner) = host_rc.try_borrow_mut() else { return };
            match rec.and_then(|r| r.quick_state) {
                Some(qs) => match inner.console.load_state_bytes(&qs) {
                    Ok(()) => {
                        inner.quick_state = Some(qs);
                        inner.set_status("loaded state");
                    }
                    Err(e) => inner.set_status(&format!("state error: {e}")),
                },
                None => inner.set_status("no save slot for this ROM"),
            }
        });
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
        let handled = s.buttons != 0xFF
            || s.turbo
            || s.save
            || s.load
            || s.screenshot
            || s.palette_cycle;
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
            self.reset();
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
                frames_to_run(self.frame_accum_ms, dt * TURBO_SPEED, TURBO_MAX_FRAMES_PER_TICK)
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
            self.console.cartridge_mut().clear_ram_dirty();
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
                    let Ok(mut inner) = host_rc2.try_borrow_mut() else { return };
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

    pub fn set_buttons(&self, mask: u8) {
        // Direct console write; we don't keep a separate field any more —
        // the rAF loop rolls the live key set straight into the console.
        self.inner.borrow_mut().console.set_buttons(mask);
    }

    pub fn get_buttons(&self) -> u8 {
        // There's no public getter on the console's P1; report the live
        // key set so the JS-side debug overlay can show what's pressed.
        let inner = self.inner.borrow();
        let mut buttons = 0xFFu8;
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

impl WasmHost {
    /// Wire the "Load ROM" button: clicking it opens a hidden `<input
    /// type="file">`; on `change` we read the file as ArrayBuffer and hand
    /// the bytes to `Inner::load_rom`.
    fn wire_load_button(&self) -> Result<(), JsValue> {
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
                .add_event_listener_with_callback(
                    "load",
                    reader_closure.as_ref().unchecked_ref(),
                )
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
    fn start_render_loop(&self) -> Result<(), JsValue> {
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
    fn wire_keyboard(&self) -> Result<(), JsValue> {
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
    fn wire_control_buttons(&self) -> Result<(), JsValue> {
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

/// Restore a ROM's saved RAM + quick-state from IDB into the console. The async
/// reply re-checks that the live ROM (`inner.rom_hash`) *and* the record's hash
/// still equal `hash` before applying anything, so a slow reply for ROM A can't
/// clobber a since-loaded ROM B.
fn restore_save(
    host_rc: &Rc<RefCell<Inner>>,
    storage: &Rc<RefCell<Option<IdbDatabase>>>,
    hash: String,
) {
    let host_rc = host_rc.clone();
    crate::storage::get_record_async(&storage.borrow(), hash.clone(), move |rec| {
        let Ok(mut inner) = host_rc.try_borrow_mut() else { return };
        if inner.rom_hash != hash {
            return; // a different ROM is loaded now — discard this late reply
        }
        let Some(rec) = rec else { return };
        if rec.rom_hash != hash {
            return; // record is for another ROM — discard
        }
        if let Some(ram) = &rec.ram {
            inner.console.cartridge_mut().load_ram(ram);
        }
        if let Some(qs) = &rec.quick_state {
            if let Err(e) = inner.console.load_state_bytes(qs) {
                weblog::error(&format!("saved quick-state could not be restored: {e}"));
            }
        }
        inner.quick_state = rec.quick_state.clone();
        inner.set_status("restored save");
    });
}

/// Start (or resume) audio. Sets the enabled flag + relabels the button
/// optimistically, then either resumes the existing context or builds one
/// asynchronously (`AudioWorklet.addModule` is a promise). Building is
/// fire-and-forget: until it resolves, the rAF loop's audio feed simply finds
/// no player yet and skips — emulation is unaffected.
fn start_audio(host: &Rc<RefCell<Inner>>, label: Option<web_sys::HtmlElement>) {
    {
        let mut h = host.borrow_mut();
        if h.audio_enabled {
            return;
        }
        h.audio_enabled = true;
        h.set_status("audio: on");
        if let Some(btn) = &label {
            btn.set_text_content(Some("Disable audio"));
        }
        if let Some(player) = h.audio.as_ref() {
            if let Err(e) = player.resume() {
                weblog::error_val("audio: failed to resume the AudioContext", &e);
            }
            return;
        }
    }
    // No graph yet — build it off the borrow (async).
    let source_rate = host.borrow().console.audio_output_rate();
    let host = host.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match crate::audio::enable(source_rate).await {
            Ok(player) => {
                let mut h = host.borrow_mut();
                // The user may have toggled audio off while the graph was
                // loading; honour the current flag, but keep the player for reuse.
                if h.audio_enabled {
                    if let Err(e) = player.resume() {
                        weblog::error_val("audio: failed to resume the AudioContext", &e);
                    }
                } else {
                    let _ = player.suspend();
                }
                h.audio = Some(player);
            }
            Err(e) => {
                weblog::error_val("audio init failed", &e);
                let mut h = host.borrow_mut();
                h.audio_enabled = false;
                h.set_status("audio: init failed");
                if let Some(btn) = &label {
                    btn.set_text_content(Some("Enable audio"));
                }
            }
        }
    });
}

/// Suspend audio (disable). The rAF loop keeps driving emulation regardless.
fn stop_audio(host: &Rc<RefCell<Inner>>, label: Option<web_sys::HtmlElement>) {
    let mut h = host.borrow_mut();
    if !h.audio_enabled {
        return;
    }
    if let Some(player) = h.audio.as_ref() {
        if let Err(e) = player.suspend() {
            weblog::error_val("audio: failed to suspend the AudioContext", &e);
        }
    }
    h.audio_enabled = false;
    h.set_status("audio: off");
    if let Some(btn) = &label {
        btn.set_text_content(Some("Enable audio"));
    }
}
