//! Joypad (P1, 0xFF00). Two active-low select lines multiplex two button groups
//! onto four input lines P10-P13 (a pressed button reads 0):
//!
//! - P14 (bit 4, 0 = selected) → the Direction pad (Right/Left/Up/Down)
//! - P15 (bit 5, 0 = selected) → the Action buttons (A/B/Select/Start)
//!
//! The joypad interrupt fires on a high→low edge of a *selected* input line, so
//! a press in the group the game isn't currently reading raises nothing — and
//! selecting a group in which a button is already held is itself an edge.

#[derive(Clone)]
pub struct P1 {
    /// Button state, 0 = pressed. Low nibble = directions (bit 0 Right, 1 Left,
    /// 2 Up, 3 Down); high nibble = actions (bit 4 A, 5 B, 6 Select, 7 Start).
    button_state: u8,
    /// Select lines P14/P15 (bits 4-5), 0 = selected. Only these are writable.
    select: u8,
}

impl P1 {
    pub fn new() -> P1 {
        P1 {
            button_state: 0xFF, // all released
            select: 0x00,       // both groups selected (P1 reads 0xCF)
        }
    }

    /// The four input lines (P10-P13): a line is 0 when a *selected* pressed
    /// button drives it low (wired-AND when both groups are selected).
    fn lines(&self) -> u8 {
        let mut lines = 0x0F;
        if self.select & 0x10 == 0 {
            lines &= self.button_state & 0x0F; // P14: directions
        }
        if self.select & 0x20 == 0 {
            lines &= self.button_state >> 4; // P15: actions
        }
        lines
    }

    /// Read 0xFF00: bits 7-6 read 1, 5-4 are the select lines, 3-0 the inputs.
    pub fn read_register(&self) -> u8 {
        0xC0 | self.select | self.lines()
    }

    /// Set the button state (0 = pressed). Returns `true` if a selected input
    /// line fell high→low, i.e. a joypad interrupt should fire.
    pub fn update_button_state(&mut self, new_button_state: u8) -> bool {
        let before = self.lines();
        self.button_state = new_button_state;
        before & !self.lines() != 0
    }

    /// Write 0xFF00: only the select lines (bits 4-5) latch. Returns `true` if
    /// re-selecting exposed a held button as a high→low edge.
    pub fn write_register(&mut self, value: u8) -> bool {
        let before = self.lines();
        self.select = value & 0x30;
        before & !self.lines() != 0
    }
}
