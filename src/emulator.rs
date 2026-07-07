use crate::cartridge::Cartridge;
use crate::console::Console;
use crate::display::Display;
use std::path::Path;

pub struct Emulator {
    console: Console,
    display: Display,
    prev_buttons: u8,
}

impl Emulator {
    pub fn new() -> Emulator {
        Emulator {
            console: Console::new(),
            display: Display::new(),
            prev_buttons: 0xFF,
        }
    }

    /// Load a `.gb` ROM from disk and boot into the DMG post-boot state.
    pub fn load_rom<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<()> {
        let data = std::fs::read(path)?;
        let cartridge = Cartridge::from_bytes(data);
        println!("Loaded ROM: \"{}\"", cartridge.title());
        self.console.load_cartridge(cartridge);
        Ok(())
    }

    pub fn run(&mut self) {
        while self.display.is_open() {
            self.update_input();
            self.console.run_frame(&mut self.display);
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
