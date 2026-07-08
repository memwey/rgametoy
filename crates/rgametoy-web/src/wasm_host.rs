//! Wasm-side host glue. The `WasmHost` newtype wraps a `Rc<RefCell<Inner>>`
//! and is the only type the JS side talks to. Inner is shared with the rAF
//! loop closure (and, in Phase 5, the audio callback closure) via an `Rc`
//! clone captured by the closure.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::Console;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, Event, HtmlCanvasElement, HtmlInputElement, KeyboardEvent};

use crate::audio::AudioPlayer;
use crate::canvas::{get_canvas, get_element_by_id, present, RGBA_LEN};
use crate::input::{from_keydown, InputState};
use crate::palette::{rgba_table, shade_to_rgba, PALETTES};
use crate::rom::{cartridge_title, load_rom};
use crate::storage::{rom_hash, SaveRecord};
use crate::ui::{get_html_element, set_text};
use crate::weblog;
use web_sys::IdbDatabase;

/// How many frames the rAF loop ticks when the turbo key is held. 4× is
/// enough to feel snappy without breaking tests / accuracy-sensitive games.
const TURBO_FRAMES_PER_TICK: u32 = 4;

/// How many rAF ticks between auto-saves of battery-backed RAM. 120 frames
/// is 2 s at 60 Hz — slow enough not to thrash the disk, fast enough that
/// a tab crash loses at most ~2 s of progress.
const AUTOSAVE_DEBOUNCE: u64 = 120;

/// One DMG frame in milliseconds (4.194304 MHz / 70224 dots ≈ 59.7275 Hz).
/// The rAF loop paces emulation against this, not the display refresh, so a
/// 120 Hz panel doesn't run the game 2×.
const DMG_FRAME_MS: f64 = 70224.0 / 4_194_304.0 * 1000.0;

/// Cap on the real time a single tick may consume, so a long stall (e.g. a
/// backgrounded tab) is absorbed instead of triggering a catch-up avalanche.
const MAX_CATCHUP_MS: f64 = 100.0;

/// Hard cap on emulated frames run per rAF tick (belt-and-braces with the
/// catch-up clamp above).
const MAX_FRAMES_PER_TICK: u32 = 4;

/// Real-time frame pacing (pure, so it's unit-tested): given the accumulator and
/// this tick's already-clamped elapsed real time `dt_ms`, return how many
/// ~59.7 Hz emulated frames to run and the leftover accumulator. Capped at
/// [`MAX_FRAMES_PER_TICK`]. This is what keeps a 120 Hz display from running the
/// game 2× — it runs frames per *real time*, not per refresh.
fn frames_to_run(accum_ms: f64, dt_ms: f64) -> (u32, f64) {
    let mut accum = accum_ms + dt_ms;
    let mut n = 0;
    while accum >= DMG_FRAME_MS && n < MAX_FRAMES_PER_TICK {
        accum -= DMG_FRAME_MS;
        n += 1;
    }
    (n, accum)
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
    /// Set once the user has enabled audio. When true, the audio callback
    /// drives the emulator forward in time (so audio and visual stay
    /// in lock-step); the rAF loop only repaints.
    pub audio_enabled: bool,
    /// Holds the AudioContext / ScriptProcessorNode for the page lifetime.
    pub audio: Option<AudioPlayer>,
    /// Closure passed to `ScriptProcessorNode::set_onaudioprocess`. Stashed
    /// in `Inner` so the JS engine doesn't drop it after the call returns.
    pub audio_closure: Option<Closure<dyn FnMut(web_sys::Event)>>,
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
    /// Counter that ticks once per rAF loop. Every [`AUTOSAVE_DEBOUNCE`]
    /// frames we flush a dirty battery-RAM record back to IDB.
    pub frame_idx: u64,
    /// Frames rendered in the last second. Updated by the rAF loop, read
    /// out to `#fps` once a wall-clock second has passed.
    pub fps_count: u32,
    pub fps_last_ms: f64,
    /// Real-time frame pacing (rAF-driven path only): accumulated real
    /// milliseconds not yet spent on emulated frames, and the previous tick's
    /// timestamp. Decouples the emulated ~59.7 Hz from the display refresh.
    pub frame_accum_ms: f64,
    pub last_tick_ms: f64,

    // Closures that must outlive any single JS callback. They go in `Inner`
    // (rather than in `WasmHost`) so a future `Rc<RefCell<Inner>>` clone
    // dropped elsewhere doesn't end the callback's lifetime early.
    pub raf_slot: RafSlot,
    pub load_button_closure: Option<Closure<dyn FnMut()>>,
    pub rom_change_closure: Option<Closure<dyn FnMut(Event)>>,
    pub rom_reader_closure: Option<Closure<dyn FnMut(Event)>>,
    pub keydown_closure: Option<Closure<dyn FnMut(KeyboardEvent)>>,
    pub keyup_closure: Option<Closure<dyn FnMut(KeyboardEvent)>>,
    pub blur_closure: Option<Closure<dyn FnMut(Event)>>,
    pub audio_button_closure: Option<Closure<dyn FnMut()>>,
    pub pause_button_closure: Option<Closure<dyn FnMut()>>,
    pub reset_button_closure: Option<Closure<dyn FnMut()>>,
    pub palette_button_closure: Option<Closure<dyn FnMut()>>,
    pub screenshot_button_closure: Option<Closure<dyn FnMut()>>,
}

impl Inner {
    fn new() -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let doc = window
            .document()
            .ok_or_else(|| JsValue::from_str("no document"))?;
        let (canvas, ctx) = get_canvas(&doc)?;
        Ok(Inner {
            console: Console::new(),
            canvas,
            ctx,
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
            audio_closure: None,
            storage: Rc::new(RefCell::new(None)),
            host_rc: None,
            rom_hash: String::new(),
            frame_idx: 0,
            fps_count: 0,
            fps_last_ms: 0.0,
            frame_accum_ms: 0.0,
            last_tick_ms: 0.0,
            raf_slot: Rc::new(RefCell::new(None)),
            load_button_closure: None,
            rom_change_closure: None,
            rom_reader_closure: None,
            keydown_closure: None,
            keyup_closure: None,
            blur_closure: None,
            audio_button_closure: None,
            pause_button_closure: None,
            reset_button_closure: None,
            palette_button_closure: None,
            screenshot_button_closure: None,
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

        // The IDB callback needs to re-enter `Inner` to apply RAM /
        // state. Pass it the shared `Rc<RefCell<Inner>>` we hold.
        let Some(host_rc) = self.host_rc.clone() else { return };
        let storage = self.storage.clone();
        let hash = self.rom_hash.clone();
        crate::storage::get_record_async(&storage.borrow(), hash, move |rec| {
            let Some(rec) = rec else { return };
            let Some(ram) = &rec.ram else { return };
            let Ok(mut inner) = host_rc.try_borrow_mut() else { return };
            inner.console.cartridge_mut().load_ram(ram);
            if let Some(qs) = &rec.quick_state {
                if let Err(e) = inner.console.load_state_bytes(qs) {
                    weblog::error(&format!("saved quick-state could not be restored: {e}"));
                }
            }
            inner.quick_state = rec.quick_state.clone();
            inner.set_status("restored save");
        });
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
            // record, so writing `None` here would wipe the user's F5 save.
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

    fn read_quick_state_from_storage(&mut self) {
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
                    Ok(()) => inner.set_status("loaded state"),
                    Err(e) => inner.set_status(&format!("state error: {e}")),
                },
                None => inner.set_status("no save slot for this ROM"),
            }
        });
    }

    /// Apply a keydown event: track the key and queue any edge-detected
    /// meta action. `Tab` (turbo) and the digit meta keys never reach the
    /// browser, so we `prevent_default()` to keep the page from scrolling
    /// on Space/etc.
    fn on_keydown(&mut self, ev: &KeyboardEvent) {
        let code = ev.code();
        // Don't let key autorepeat re-trigger the meta edges: only the
        // *first* keydown of a save/load/screenshot/palette press acts.
        let was_down = !self.keys_down.insert(code.clone());
        if was_down {
            return;
        }
        let s: InputState = from_keydown(&code);
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
        if s.turbo || s.save || s.load || s.screenshot || s.palette_cycle {
            ev.prevent_default();
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
            self.read_quick_state_from_storage();
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
        if !self.paused && self.has_rom && !self.audio_enabled {
            let now = js_sys::Date::now();
            if self.keys_down.contains("Tab") {
                // Fast-forward: a fixed burst per tick. Reset the accumulator
                // so releasing turbo doesn't leave a backlog to replay.
                for _ in 0..TURBO_FRAMES_PER_TICK {
                    self.console.run_frame();
                }
                self.frame_accum_ms = 0.0;
            } else {
                // Pace against real elapsed time, not the display refresh: a
                // fixed one-frame-per-rAF would run 2× on a 120 Hz panel. Run
                // as many ~59.7 Hz frames as fit the elapsed time, capped.
                let dt = if self.last_tick_ms == 0.0 {
                    DMG_FRAME_MS
                } else {
                    (now - self.last_tick_ms).min(MAX_CATCHUP_MS)
                };
                let (n, accum) = frames_to_run(self.frame_accum_ms, dt);
                self.frame_accum_ms = accum;
                for _ in 0..n {
                    self.console.run_frame();
                }
            }
            self.last_tick_ms = now;
        }
        let fb = self.console.framebuffer();
        let table = rgba_table(self.palette_idx);
        shade_to_rgba(fb, table, &mut self.rgba_buf);
        let _ = present(&self.ctx, &self.rgba_buf);

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

        // FPS: count frames, recompute the displayed value once per
        // wall-clock second. The current rAF rate is a useful indicator
        // that the user can match against their monitor's refresh rate.
        self.fps_count = self.fps_count.saturating_add(1);
        let now = js_sys::Date::now();
        if self.fps_last_ms == 0.0 {
            self.fps_last_ms = now;
        } else if now - self.fps_last_ms >= 1000.0 {
            let fps = self.fps_count;
            self.fps_count = 0;
            self.fps_last_ms = now;
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
            if let Err(e) = crate::storage::init_async(storage_slot, on_unavailable) {
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

    /// Enable WebAudio output. Idempotent: a second call is a no-op.
    /// Must be called from a user-gesture handler (click/keypress), or
    /// the browser will keep the AudioContext suspended.
    pub fn enable_audio(&self) -> Result<(), JsValue> {
        let mut inner = self.inner.borrow_mut();
        if inner.audio_enabled {
            return Ok(());
        }
        let player = crate::audio::enable(self.inner.clone())?;
        if let Err(e) = player.resume() {
            weblog::error_val("audio: failed to resume the AudioContext", &e);
        }
        inner.audio = Some(player);
        inner.audio_enabled = true;
        inner.set_status("audio: on");
        Ok(())
    }

    pub fn is_audio_enabled(&self) -> bool {
        self.inner.borrow().audio_enabled
    }

    /// Suspend audio output (e.g. on tab blur). The emulator keeps
    /// running visually; audio resumes when [`Self::resume_audio`] is
    /// called.
    pub fn suspend_audio(&self) -> Result<(), JsValue> {
        let inner = self.inner.borrow();
        if let Some(player) = &inner.audio {
            player.suspend()?;
        }
        Ok(())
    }

    pub fn resume_audio(&self) -> Result<(), JsValue> {
        let inner = self.inner.borrow();
        if let Some(player) = &inner.audio {
            player.resume()?;
        }
        Ok(())
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
        let click_closure = Closure::wrap(Box::new(move || {
            input_for_click.click();
        }) as Box<dyn FnMut()>);
        button.add_event_listener_with_callback(
            "click",
            click_closure.as_ref().unchecked_ref(),
        )?;

        // Change on #rom-input → read file → load_rom.
        let host_for_change: Rc<RefCell<Inner>> = self.inner.clone();
        let file_for_change: HtmlInputElement = file_input.clone();
        let change_closure = Closure::wrap(Box::new(move |_event: Event| {
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
            // Stash the reader closure on the host so it isn't dropped
            // (which would invalidate the callback before `load` fires).
            host_for_change.borrow_mut().rom_reader_closure = Some(reader_closure);

            if let Err(e) = reader.read_as_array_buffer(&file) {
                weblog::error_val("could not read the selected ROM file", &e);
            }
        }) as Box<dyn FnMut(Event)>);
        file_input
            .add_event_listener_with_callback("change", change_closure.as_ref().unchecked_ref())?;

        // Stash so they don't get dropped.
        let mut inner = self.inner.borrow_mut();
        inner.load_button_closure = Some(click_closure);
        inner.rom_change_closure = Some(change_closure);
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
        let kd_closure = Closure::wrap(Box::new(move |ev: KeyboardEvent| {
            host_for_kd.borrow_mut().on_keydown(&ev);
        }) as Box<dyn FnMut(KeyboardEvent)>);
        window.add_event_listener_with_callback("keydown", kd_closure.as_ref().unchecked_ref())?;

        let host_for_ku: Rc<RefCell<Inner>> = self.inner.clone();
        let ku_closure = Closure::wrap(Box::new(move |ev: KeyboardEvent| {
            host_for_ku.borrow_mut().on_keyup(&ev);
        }) as Box<dyn FnMut(KeyboardEvent)>);
        window.add_event_listener_with_callback("keyup", ku_closure.as_ref().unchecked_ref())?;

        let host_for_blur: Rc<RefCell<Inner>> = self.inner.clone();
        let blur_closure = Closure::wrap(Box::new(move |_ev: Event| {
            host_for_blur.borrow_mut().on_blur();
        }) as Box<dyn FnMut(Event)>);
        window.add_event_listener_with_callback("blur", blur_closure.as_ref().unchecked_ref())?;

        let mut inner = self.inner.borrow_mut();
        inner.keydown_closure = Some(kd_closure);
        inner.keyup_closure = Some(ku_closure);
        inner.blur_closure = Some(blur_closure);
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
        let pause_closure = Closure::wrap(Box::new(move || {
            let mut h = host_pause.borrow_mut();
            h.paused = !h.paused;
            let state = if h.paused { "paused" } else { "running" };
            h.set_status(state);
        }) as Box<dyn FnMut()>);
        let pause_btn = get_html_element(&doc, "pause-button")?;
        pause_btn.add_event_listener_with_callback(
            "click",
            pause_closure.as_ref().unchecked_ref(),
        )?;

        // Reset — same effect as the DMG power-on sequence.
        let host_reset: Rc<RefCell<Inner>> = self.inner.clone();
        let reset_closure = Closure::wrap(Box::new(move || {
            host_reset.borrow_mut().reset_pending = true;
        }) as Box<dyn FnMut()>);
        let reset_btn = get_html_element(&doc, "reset-button")?;
        reset_btn.add_event_listener_with_callback(
            "click",
            reset_closure.as_ref().unchecked_ref(),
        )?;

        // Cycle palette — same as pressing Digit3.
        let host_pal: Rc<RefCell<Inner>> = self.inner.clone();
        let pal_closure = Closure::wrap(Box::new(move || {
            host_pal.borrow_mut().palette_pending = true;
        }) as Box<dyn FnMut()>);
        let pal_btn = get_html_element(&doc, "palette-button")?;
        pal_btn.add_event_listener_with_callback(
            "click",
            pal_closure.as_ref().unchecked_ref(),
        )?;

        // Screenshot — same as pressing Digit2.
        let host_shot: Rc<RefCell<Inner>> = self.inner.clone();
        let shot_closure = Closure::wrap(Box::new(move || {
            host_shot.borrow_mut().screenshot_pending = true;
        }) as Box<dyn FnMut()>);
        let shot_btn = get_html_element(&doc, "screenshot-button")?;
        shot_btn.add_event_listener_with_callback(
            "click",
            shot_closure.as_ref().unchecked_ref(),
        )?;

        // Enable audio — the only button that needs to know its own
        // element so it can relabel itself. Web Audio policies require
        // the AudioContext to be resumed from a user gesture, hence
        // the action is gated to a button click.
        let host_audio: Rc<RefCell<Inner>> = self.inner.clone();
        let audio_btn: web_sys::HtmlElement = get_html_element(&doc, "audio-button")?;
        let audio_btn_for_cb: web_sys::HtmlElement = audio_btn.clone();
        let audio_closure = Closure::wrap(Box::new(move || {
            let mut h = host_audio.borrow_mut();
            if h.audio_enabled {
                return;
            }
            match crate::audio::enable(host_audio.clone()) {
                Ok(player) => {
                    if let Err(e) = player.resume() {
                        weblog::error_val("audio: failed to resume the AudioContext", &e);
                    }
                    h.audio = Some(player);
                    h.audio_enabled = true;
                    h.set_status("audio: on");
                    audio_btn_for_cb.set_text_content(Some("Disable audio"));
                }
                Err(e) => {
                    weblog::error_val("audio init failed", &e);
                    let msg = e.as_string().unwrap_or_else(|| "audio init failed".to_string());
                    h.set_status(&format!("audio: {msg}"));
                }
            }
        }) as Box<dyn FnMut()>);
        audio_btn.add_event_listener_with_callback(
            "click",
            audio_closure.as_ref().unchecked_ref(),
        )?;

        let mut inner = self.inner.borrow_mut();
        inner.pause_button_closure = Some(pause_closure);
        inner.reset_button_closure = Some(reset_closure);
        inner.palette_button_closure = Some(pal_closure);
        inner.screenshot_button_closure = Some(shot_closure);
        inner.audio_button_closure = Some(audio_closure);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{frames_to_run, DMG_FRAME_MS, MAX_FRAMES_PER_TICK};

    #[test]
    fn one_dmg_frame_of_elapsed_runs_one_frame() {
        let (n, accum) = frames_to_run(0.0, DMG_FRAME_MS);
        assert_eq!(n, 1);
        assert!(accum.abs() < 1e-9, "no leftover, got {accum}");
    }

    #[test]
    fn a_short_tick_runs_nothing_but_accumulates() {
        // A 60 Hz tick (16.667 ms) is just under one DMG frame (16.743 ms), so
        // it runs 0 frames and carries the remainder — the next tick runs 1.
        let (n, accum) = frames_to_run(0.0, 1000.0 / 60.0);
        assert_eq!(n, 0);
        assert!(accum > 16.0, "carried the elapsed time, got {accum}");
    }

    #[test]
    fn catch_up_is_capped() {
        // 10 frames' worth of elapsed time in one tick is clamped to the cap.
        let (n, _) = frames_to_run(0.0, DMG_FRAME_MS * 10.0);
        assert_eq!(n, MAX_FRAMES_PER_TICK);
    }

    /// The regression that motivated the accumulator: emulation must run at the
    /// DMG's ~59.7 Hz for *one real second* regardless of the display refresh —
    /// a fixed one-frame-per-rAF ran the game 2× on a 120 Hz panel.
    #[test]
    fn paces_to_dmg_rate_regardless_of_refresh() {
        for hz in [60.0_f64, 120.0, 144.0] {
            let dt = 1000.0 / hz;
            let mut accum = 0.0;
            let mut total = 0u32;
            for _ in 0..(hz as u32) {
                // one real second of ticks
                let (n, a) = frames_to_run(accum, dt);
                total += n;
                accum = a;
            }
            assert!(
                (total as i32 - 60).abs() <= 1,
                "{hz} Hz ran {total} frames/s (expected ~59.7, not ~{hz})"
            );
        }
    }
}
