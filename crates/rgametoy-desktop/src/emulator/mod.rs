pub mod display;
pub mod input;
pub mod log;
pub mod palette;
pub mod paths;
pub mod screenshot;
#[cfg(feature = "audio")]
pub mod audio;

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::{Console, SaveState};
use crate::emulator::display::Display;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Flush battery RAM to disk at most this often (in frames, ~2 s at 60 fps)
/// while the game keeps writing saves.
const AUTOSAVE_INTERVAL_FRAMES: u32 = 120;

/// Real-time duration of one emulated frame at 1× speed: 70224 dots / 4.19 MHz.
const FRAME_SECONDS: f64 = 70224.0 / 4_194_304.0;

const DEFAULT_TURBO_SPEED: f64 = 4.0;

/// Real-time budget for one emulated frame at the given speed multiplier.
/// Higher speed → smaller budget → the loop sleeps less and emulation runs
/// faster, while the work per frame (one PPU frame of CPU cycles) is fixed.
fn frame_budget(speed: f64) -> Duration {
    Duration::from_secs_f64(FRAME_SECONDS / speed.max(1e-3))
}

/// Human name for a supported cartridge-type byte (header 0x0147), for the
/// startup banner. Mirrors the set `Cartridge::is_type_supported` accepts.
fn cart_type_name(type_byte: u8) -> &'static str {
    match type_byte {
        0x00 => "ROM only",
        0x01..=0x03 => "MBC1",
        0x0F..=0x13 => "MBC3",
        0x19..=0x1E => "MBC5",
        _ => "unknown",
    }
}

pub struct Emulator {
    console: Console,
    display: Display,
    /// Speed multiplier applied while the fast-forward key is held.
    turbo_speed: f64,
    /// Instant save-state slot, plus previous key states for edge detection.
    quick_state: Option<SaveState>,
    prev_save: bool,
    prev_load: bool,
    prev_screenshot: bool,
    prev_palette: bool,
    /// Save-file path (`<data-dir>/saves/<rom>-<hash>.sav`), set only for
    /// battery-backed cartridges.
    save_path: Option<PathBuf>,
    /// Base data directory holding `saves/` and `screenshots/`, and the loaded
    /// ROM's title (used to name screenshots).
    data_dir: PathBuf,
    rom_title: String,
    #[cfg(feature = "audio")]
    audio: Option<crate::emulator::audio::AudioPlayer>,
}

impl Emulator {
    pub fn new() -> Emulator {
        let console = Console::new();

        #[cfg(feature = "audio")]
        let audio = {
            // The core APU emits at its own fixed rate; the player resamples to
            // the device. The core is never told the device rate.
            let source_rate = console.audio_output_rate();
            let player = crate::emulator::audio::AudioPlayer::new(source_rate);
            match &player {
                Some(p) => log::info(&format!("audio: {} Hz output", p.sample_rate())),
                None => log::warn("no audio output device found; running muted"),
            }
            player
        };

        Emulator {
            console,
            display: Display::new(),
            turbo_speed: DEFAULT_TURBO_SPEED,
            quick_state: None,
            prev_save: false,
            prev_load: false,
            prev_screenshot: false,
            prev_palette: false,
            save_path: None,
            data_dir: paths::data_dir(),
            rom_title: String::new(),
            #[cfg(feature = "audio")]
            audio,
        }
    }

    /// Set the fast-forward multiplier (clamped to at least 1×).
    pub fn set_turbo_speed(&mut self, speed: f64) {
        self.turbo_speed = speed.max(1.0);
    }

    /// Base data directory holding `saves/` and `screenshots/` (default:
    /// `$RGAMETOY_DATA_DIR` or the working directory). Set before `load_rom`.
    pub fn set_data_dir<P: Into<PathBuf>>(&mut self, dir: P) {
        self.data_dir = dir.into();
    }

    /// Load a `.gb` ROM from disk and boot into the DMG post-boot state. For a
    /// battery-backed cartridge, restore its save from `<data-dir>/saves`.
    pub fn load_rom<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        let data = std::fs::read(&path)?;
        // Refuse a cartridge type we don't implement, rather than silently
        // mis-emulating it as MBC1 (e.g. MBC2 has a different bank layout).
        let cart_type = data.get(0x0147).copied().unwrap_or(0);
        if !Cartridge::is_type_supported(cart_type) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "unsupported cartridge type {cart_type:#04x} \
                     (only no-MBC / MBC1 / MBC3-no-RTC / MBC5 are implemented)"
                ),
            ));
        }
        // Derive the save name and ROM size from the bytes before they move
        // into the cartridge (hashing a few MB is negligible and avoids a copy).
        let save_name = paths::save_name(path.as_ref(), &data);
        let rom_kib = data.len() / 1024;
        let cartridge = Cartridge::from_bytes(data);
        self.rom_title = cartridge.title();
        let has_battery = cartridge.has_battery();
        let ram_kib = cartridge.ram().len() / 1024;
        self.console.load_cartridge(cartridge);

        // Startup banner: what got loaded.
        log::heading(&format!(
            "rgametoy — {}",
            if self.rom_title.is_empty() { "(untitled)" } else { &self.rom_title }
        ));
        log::field("cartridge", cart_type_name(cart_type));
        log::field("ROM", &format!("{rom_kib} KiB"));
        if ram_kib > 0 {
            let battery = if has_battery { " (battery)" } else { "" };
            log::field("RAM", &format!("{ram_kib} KiB{battery}"));
        }

        if has_battery {
            // Saves live in <data-dir>/saves keyed by ROM name + content hash.
            let save_path = self.data_dir.join("saves").join(save_name);
            match std::fs::read(&save_path) {
                Ok(saved) => {
                    self.console.get_bus_mut().cartridge_mut().load_ram(&saved);
                    log::field("save", &format!("{} (loaded)", save_path.display()));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    log::field("save", &format!("{} (new)", save_path.display()));
                }
                Err(e) => log::warn(&format!("could not read save {}: {e}", save_path.display())),
            }
            self.save_path = Some(save_path);
        }
        Ok(())
    }

    pub fn run(&mut self) {
        let mut frames_since_save = 0u32;
        // Rolling window for the fps read-out shown in the window title.
        let mut fps_frames = 0u32;
        let mut fps_window_start = Instant::now();
        // (debug) accumulated core-emulation / present time over that window,
        // shown in the title so the core-vs-frontend split is visible.
        #[cfg(feature = "debug")]
        let (mut core_accum, mut present_accum) = (Duration::ZERO, Duration::ZERO);
        while self.display.is_open() {
            let frame_start = Instant::now();

            let input = crate::emulator::input::poll(&self.display);
            self.apply_input(&input);
            self.handle_hotkeys(&input);
            let speed = if input.turbo { self.turbo_speed } else { 1.0 };

            // Emulate one frame in the core, then present it — presentation is a
            // frontend concern, so the core just hands back its framebuffer.
            #[cfg(feature = "debug")]
            let core_t = Instant::now();
            self.console.run_frame();
            #[cfg(feature = "debug")]
            {
                core_accum += core_t.elapsed();
            }
            #[cfg(feature = "debug")]
            let present_t = Instant::now();
            self.display.present(self.console.framebuffer());
            #[cfg(feature = "debug")]
            {
                present_accum += present_t.elapsed();
            }

            // Forward serial output (test ROMs print their results here).
            let serial = self.console.take_serial_output();
            if !serial.is_empty() {
                use std::io::Write;
                print!("{}", String::from_utf8_lossy(&serial));
                let _ = std::io::stdout().flush();
            }

            // Drain the APU each frame to keep its buffer bounded. Only feed
            // the device at normal speed: off-speed produces the wrong number
            // of samples per real second, so we mute (drop) instead — which
            // keeps the resampler a fixed source→device ratio.
            let samples = self.console.take_audio_samples();
            let normal_speed = speed == 1.0;
            #[cfg(feature = "audio")]
            if normal_speed {
                if let Some(player) = &mut self.audio {
                    player.queue(&samples);
                }
            }
            #[cfg(not(feature = "audio"))]
            let _ = (samples, normal_speed);

            frames_since_save += 1;
            if frames_since_save >= AUTOSAVE_INTERVAL_FRAMES {
                self.save_ram();
                frames_since_save = 0;
            }

            // Pace emulation against the CPU clock: one frame should take
            // FRAME_SECONDS / speed of real time. If we're behind (host too
            // slow, or fast-forwarding flat out), don't sleep.
            let elapsed = frame_start.elapsed();
            let budget = frame_budget(speed);
            if elapsed < budget {
                std::thread::sleep(budget - elapsed);
            }

            // Refresh the title's fps read-out over a ~0.5 s window (measuring
            // real wall-clock, so it reflects the paced/turbo rate we actually
            // hit, not the fixed emulated 59.7 Hz).
            fps_frames += 1;
            let window = fps_window_start.elapsed();
            if window >= Duration::from_millis(500) {
                let fps = fps_frames as f64 / window.as_secs_f64();
                // (debug) average core / present ms per frame this window.
                #[cfg(feature = "debug")]
                let extra = {
                    let n = fps_frames.max(1) as f64;
                    format!(
                        " — core {:.1}ms present {:.1}ms",
                        core_accum.as_secs_f64() * 1e3 / n,
                        present_accum.as_secs_f64() * 1e3 / n,
                    )
                };
                #[cfg(not(feature = "debug"))]
                let extra = "";
                self.display.set_title(&format!(
                    "rgametoy — {fps:.0} fps ({speed:.1}x) — {}{extra}",
                    self.display.palette_name()
                ));
                fps_frames = 0;
                fps_window_start = Instant::now();
                #[cfg(feature = "debug")]
                {
                    core_accum = Duration::ZERO;
                    present_accum = Duration::ZERO;
                }
            }
        }
        // Final flush on exit.
        self.save_ram();
    }

    /// Write battery RAM to the `.sav` file if it has changed since last flush.
    fn save_ram(&mut self) {
        let path = match &self.save_path {
            Some(p) => p.clone(),
            None => return,
        };
        let bus = self.console.get_bus_mut();
        if !bus.cartridge().ram_dirty() {
            return;
        }
        let ram = bus.cartridge().ram().to_vec();
        // The saves/ directory may not exist yet on the first flush.
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::error(&format!("failed to create save dir {}: {e}", dir.display()));
                return;
            }
        }
        match std::fs::write(&path, &ram) {
            Ok(()) => bus.cartridge_mut().clear_ram_dirty(),
            Err(e) => log::error(&format!("failed to write save {}: {e}", path.display())),
        }
    }

    /// Handle the edge-triggered hotkeys (one press acts once): F5/F7 store and
    /// restore the instant save-state slot, F2 saves a screenshot.
    fn handle_hotkeys(&mut self, input: &crate::emulator::input::InputState) {
        if input.save && !self.prev_save {
            self.quick_state = Some(self.console.save_state());
            log::info("save state stored");
        }
        if input.load && !self.prev_load {
            if let Some(state) = &self.quick_state {
                self.console.load_state(state);
                log::info("save state loaded");
            }
        }
        if input.screenshot && !self.prev_screenshot {
            self.take_screenshot();
        }
        if input.palette_cycle && !self.prev_palette {
            let name = self.display.cycle_palette();
            log::info(&format!("palette: {name}"));
        }
        self.prev_save = input.save;
        self.prev_load = input.load;
        self.prev_screenshot = input.screenshot;
        self.prev_palette = input.palette_cycle;
    }

    /// Write the current frame to the screenshot directory, reporting where it
    /// landed (or why it failed) — a screenshot should never take down the
    /// emulator.
    fn take_screenshot(&mut self) {
        let hint = if self.rom_title.is_empty() {
            "rgametoy"
        } else {
            &self.rom_title
        };
        let dir = self.data_dir.join("screenshots");
        // Match the window: screenshots use the active palette.
        let palette = self.display.palette();
        match screenshot::save(self.console.framebuffer(), &dir, hint, palette) {
            Ok(path) => log::info(&format!("screenshot saved: {}", path.display())),
            Err(e) => log::error(&format!("screenshot failed ({}): {e}", dir.display())),
        }
    }

    fn apply_input(&mut self, input: &crate::emulator::input::InputState) {
        // The joypad hardware raises the interrupt itself (gated by the P1
        // select lines), so the frontend just forwards the button state.
        self.console.set_buttons(input.buttons);
    }

    pub fn get_console(&self) -> &Console {
        &self.console
    }

    pub fn get_console_mut(&mut self) -> &mut Console {
        &mut self.console
    }
}

impl Default for Emulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_speed_frame_budget_is_one_gb_frame() {
        let budget = frame_budget(1.0).as_secs_f64();
        assert!((budget - 1.0 / 59.7275).abs() < 1e-4, "budget was {budget}");
    }

    #[test]
    fn turbo_divides_the_budget_by_the_multiplier() {
        let normal = frame_budget(1.0).as_secs_f64();
        let turbo = frame_budget(4.0).as_secs_f64();
        assert!((turbo * 4.0 - normal).abs() < 1e-6);
    }
}
