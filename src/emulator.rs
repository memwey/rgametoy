use crate::console::Console;
use crate::display::Display;
use crate::input::Input;

pub struct Emulator {
    console: Console,
    input: Input,
    display: Display,
}

impl Emulator {
    pub fn new() -> Emulator {
        Emulator {
            console: Console::new(),
            input: Input::new(),
            display: Display::new(),
        }
    }

    pub fn run(&mut self) {
        while self.display.is_open() {
            self.update_input();
            self.console.run_frame(&mut self.display);
        }
    }

    pub fn update_input(&mut self) {
        let raw_button_state = self.input.get_raw_button_state();
        self.console.get_p1().borrow_mut().update_button_state(raw_button_state);
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
