// P1 Register is at 0xFF00, and is used to read the state of the buttons
#[derive(Clone)]
pub struct P1 {
    // State of the buttons
    // Bit 0 - Right
    // Bit 1 - Left
    // Bit 2 - Up
    // Bit 3 - Down
    // Bit 4 - A
    // Bit 5 - B
    // Bit 6 - Select
    // Bit 7 - Start
    button_state: u8,
    // Selects which buttons are read
    // Bit 4 - P14 Select Button Keys (A, B, Select, Start)
    // Bit 5 - P15 Select Direction Keys (Right, Left, Up, Down)
    select_state: u8,
}

impl P1 {
    pub fn new() -> P1 {
        P1 {
            button_state: 0xFF, // All buttons released initially
            select_state: 0xCF, // Initial state for P1 register
        }
    }

    // Read from Joypad register (0xFF00)
    pub fn read_register(&self) -> u8 {
        // Combine select_state and button_state
        // Only selected buttons are returned
        let mut result = self.select_state;
        if result & 0x10 == 0 { // P14 selected (Button Keys)
            result &= (self.button_state >> 4) | 0xF0; // Mask out direction keys
        }
        if result & 0x20 == 0 { // P15 selected (Direction Keys)
            result &= self.button_state | 0xF0; // Mask out button keys
        }
        result
    }

    // Write to Joypad register (0xFF00)
    pub fn write_register(&mut self, value: u8) {
        // Only bits 4 and 5 are writable
        self.select_state = (self.select_state & 0xCF) | (value & 0x30);
    }

    // Method to update button_state from Input
    pub fn update_button_state(&mut self, new_button_state: u8) {
        self.button_state = new_button_state;
    }
}
