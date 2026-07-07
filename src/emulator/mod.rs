pub mod display;
pub mod input;
#[cfg(feature = "audio")]
pub mod audio;

use crate::console::cartridge::Cartridge;
use crate::console::{Console, SaveState};
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

pub struct Emulator {
    console: Console,
    display: Display,
    prev_buttons: u8,
    /// Speed multiplier applied while the fast-forward key is held.
    turbo_speed: f64,
    /// Instant save-state slot, plus previous key states for edge detection.
    quick_state: Option<SaveState>,
    prev_save: bool,
    prev_load: bool,
    /// `<rom>.sav` path, set only for battery-backed cartridges.
    save_path: Option<PathBuf>,
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
                Some(p) => println!("audio: output at {} Hz", p.sample_rate()),
                None => eprintln!("audio: no output device found, running muted"),
            }
            player
        };

        Emulator {
            console,
            display: Display::new(),
            prev_buttons: 0xFF,
            turbo_speed: DEFAULT_TURBO_SPEED,
            quick_state: None,
            prev_save: false,
            prev_load: false,
            save_path: None,
            #[cfg(feature = "audio")]
            audio,
        }
    }

    /// Set the fast-forward multiplier (clamped to at least 1×).
    pub fn set_turbo_speed(&mut self, speed: f64) {
        self.turbo_speed = speed.max(1.0);
    }

    /// Load a `.gb` ROM from disk and boot into the DMG post-boot state. For a
    /// battery-backed cartridge, restore its save from a sibling `.sav` file if
    /// one exists.
    pub fn load_rom<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        let data = std::fs::read(&path)?;
        let cartridge = Cartridge::from_bytes(data);
        println!("Loaded ROM: \"{}\"", cartridge.title());
        self.console.load_cartridge(cartridge);

        if self.console.get_bus_mut().cartridge().has_battery() {
            let save_path = path.as_ref().with_extension("sav");
            match std::fs::read(&save_path) {
                Ok(saved) => {
                    self.console.get_bus_mut().cartridge_mut().load_ram(&saved);
                    println!("Loaded save: {}", save_path.display());
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => eprintln!("could not read save {}: {e}", save_path.display()),
            }
            self.save_path = Some(save_path);
        }
        Ok(())
    }

    pub fn run(&mut self) {
        let mut frames_since_save = 0u32;
        while self.display.is_open() {
            let frame_start = Instant::now();

            let input = crate::emulator::input::poll(&self.display);
            self.apply_input(&input);
            self.handle_save_state(&input);
            let speed = if input.turbo { self.turbo_speed } else { 1.0 };

            // Emulate one frame in the core, then present it — presentation is a
            // frontend concern, so the core just hands back its framebuffer.
            self.console.run_frame();
            self.display.present(self.console.framebuffer());

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
        match std::fs::write(&path, &ram) {
            Ok(()) => bus.cartridge_mut().clear_ram_dirty(),
            Err(e) => eprintln!("failed to write save {}: {e}", path.display()),
        }
    }

    /// Handle the instant save/load hotkeys (edge-triggered: one press acts
    /// once). F5 captures a snapshot, F7 restores it.
    fn handle_save_state(&mut self, input: &crate::emulator::input::InputState) {
        if input.save && !self.prev_save {
            self.quick_state = Some(self.console.save_state());
            println!("save state stored");
        }
        if input.load && !self.prev_load {
            if let Some(state) = &self.quick_state {
                self.console.load_state(state);
                println!("save state loaded");
            }
        }
        self.prev_save = input.save;
        self.prev_load = input.load;
    }

    fn apply_input(&mut self, input: &crate::emulator::input::InputState) {
        // A bit going 1 (released) -> 0 (pressed) is a new key press.
        let newly_pressed = self.prev_buttons & !input.buttons;
        self.console.set_buttons(input.buttons);
        if newly_pressed != 0 {
            self.console.request_joypad_interrupt();
        }
        self.prev_buttons = input.buttons;
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
