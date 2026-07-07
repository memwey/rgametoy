use crate::cartridge::Cartridge;
use crate::console::Console;
use crate::display::Display;
use std::path::{Path, PathBuf};

/// Flush battery RAM to disk at most this often (in frames, ~2 s at 60 fps)
/// while the game keeps writing saves.
const AUTOSAVE_INTERVAL_FRAMES: u32 = 120;

pub struct Emulator {
    console: Console,
    display: Display,
    prev_buttons: u8,
    /// `<rom>.sav` path, set only for battery-backed cartridges.
    save_path: Option<PathBuf>,
    #[cfg(feature = "audio")]
    audio: Option<crate::audio::AudioPlayer>,
}

impl Emulator {
    pub fn new() -> Emulator {
        let console = Console::new();

        #[cfg(feature = "audio")]
        let audio = {
            let player = crate::audio::AudioPlayer::new();
            match &player {
                Some(p) => {
                    console.get_apu().borrow_mut().set_sample_rate(p.sample_rate());
                    println!("audio: output at {} Hz", p.sample_rate());
                }
                None => eprintln!("audio: no output device found, running muted"),
            }
            player
        };

        Emulator {
            console,
            display: Display::new(),
            prev_buttons: 0xFF,
            save_path: None,
            #[cfg(feature = "audio")]
            audio,
        }
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
            self.update_input();
            self.console.run_frame(&mut self.display);

            // Drain the APU each frame (keeps its buffer bounded even when
            // there is no audio backend).
            let samples = self.console.get_apu().borrow_mut().take_samples();
            #[cfg(feature = "audio")]
            if let Some(player) = &self.audio {
                player.queue(&samples);
            }
            #[cfg(not(feature = "audio"))]
            let _ = samples;

            frames_since_save += 1;
            if frames_since_save >= AUTOSAVE_INTERVAL_FRAMES {
                self.save_ram();
                frames_since_save = 0;
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

    fn update_input(&mut self) {
        let buttons = self.display.poll_buttons();
        // A bit going 1 (released) -> 0 (pressed) is a new key press.
        let newly_pressed = self.prev_buttons & !buttons;
        self.console
            .get_p1()
            .borrow_mut()
            .update_button_state(buttons);
        if newly_pressed != 0 {
            self.console.request_joypad_interrupt();
        }
        self.prev_buttons = buttons;
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
