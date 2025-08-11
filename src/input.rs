// Define constants for Game Boy button bits
pub const GB_BUTTON_RIGHT: u8 = 0x01;
pub const GB_BUTTON_LEFT: u8 = 0x02;
pub const GB_BUTTON_UP: u8 = 0x04;
pub const GB_BUTTON_DOWN: u8 = 0x08;
pub const GB_BUTTON_A: u8 = 0x10;
pub const GB_BUTTON_B: u8 = 0x20;
pub const GB_BUTTON_SELECT: u8 = 0x40;
pub const GB_BUTTON_START: u8 = 0x80;

pub struct Input {
    // Raw state of host buttons
    // Each bit represents a Game Boy button, regardless of Game Boy's P1 register selection
    // Bit 0 - Right
    // Bit 1 - Left
    // Bit 2 - Up
    // Bit 3 - Down
    // Bit 4 - A
    // Bit 5 - B
    // Bit 6 - Select
    // Bit 7 - Start
    raw_button_state: u8,
}

impl Input {
    pub fn new() -> Input {
        Input {
            raw_button_state: 0xFF, // All buttons released initially
        }
    }

    // Generic update methods (can be private if only specific methods are exposed)
    fn set_key_state(&mut self, gb_button_bit: u8, is_pressed: bool) {
        if is_pressed {
            self.raw_button_state &= !gb_button_bit;
        } else {
            self.raw_button_state |= gb_button_bit;
        }
    }

    // Specific methods for each button
    pub fn key_down_right(&mut self) { self.set_key_state(GB_BUTTON_RIGHT, true); }
    pub fn key_up_right(&mut self) { self.set_key_state(GB_BUTTON_RIGHT, false); }

    pub fn key_down_left(&mut self) { self.set_key_state(GB_BUTTON_LEFT, true); }
    pub fn key_up_left(&mut self) { self.set_key_state(GB_BUTTON_LEFT, false); }

    pub fn key_down_up(&mut self) { self.set_key_state(GB_BUTTON_UP, true); }
    pub fn key_up_up(&mut self) { self.set_key_state(GB_BUTTON_UP, false); }

    pub fn key_down_down(&mut self) { self.set_key_state(GB_BUTTON_DOWN, true); }
    pub fn key_up_down(&mut self) { self.set_key_state(GB_BUTTON_DOWN, false); }

    pub fn key_down_a(&mut self) { self.set_key_state(GB_BUTTON_A, true); }
    pub fn key_up_a(&mut self) { self.set_key_state(GB_BUTTON_A, false); }

    pub fn key_down_b(&mut self) { self.set_key_state(GB_BUTTON_B, true); }
    pub fn key_up_b(&mut self) { self.set_key_state(GB_BUTTON_B, false); }

    pub fn key_down_select(&mut self) { self.set_key_state(GB_BUTTON_SELECT, true); }
    pub fn key_up_select(&mut self) { self.set_key_state(GB_BUTTON_SELECT, false); }

    pub fn key_down_start(&mut self) { self.set_key_state(GB_BUTTON_START, true); }
    pub fn key_up_start(&mut self) { self.set_key_state(GB_BUTTON_START, false); }

    // Get the current raw button state
    pub fn get_raw_button_state(&self) -> u8 {
        self.raw_button_state
    }
}
