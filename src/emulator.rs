use crate::console::Console;
use crate::input::Input;

pub struct Emulator {
    console: Console,
    input: Input,
}

impl Emulator {
    pub fn new() -> Emulator {
        Emulator {
            console: Console::new(),
            input: Input::new(),
        }
    }

    pub fn run(&mut self) {
        // This will be the main emulation loop
        self.update_input(); // Update input before running a frame
        self.console.run_frame(); // Run a full frame
    }

    pub fn update_input(&mut self) {
        let raw_button_state = self.input.get_raw_button_state();
        self.console.get_bus_mut().get_p1_mut().update_button_state(raw_button_state);
    }

    pub fn get_input_mut(&mut self) -> &mut Input {
        &mut self.input
    }

    pub fn get_console(&self) -> &Console {
        &self.console
    }

    pub fn get_console_mut(&mut self) -> &mut Console {
        &mut self.console
    }

    // You might add methods here for rendering, loading ROMs, etc.
}
