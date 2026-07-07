//! Frontend input mapping: turns host keyboard state into the Game Boy joypad
//! byte the core's P1 register consumes, plus the fast-forward key. This is the
//! only place that knows which host key means which console button, keeping the
//! key bindings separate from both the window (`Display`) and the emulated
//! joypad hardware (`P1`).

use crate::emulator::display::Display;
use minifb::Key;

// Game Boy button bits, matching the byte P1 consumes: a set bit means the
// button is released, a cleared bit means pressed.
pub const RIGHT: u8 = 0x01;
pub const LEFT: u8 = 0x02;
pub const UP: u8 = 0x04;
pub const DOWN: u8 = 0x08;
pub const A: u8 = 0x10;
pub const B: u8 = 0x20;
pub const SELECT: u8 = 0x40;
pub const START: u8 = 0x80;

/// Host key → Game Boy button binding.
const KEY_MAP: &[(Key, u8)] = &[
    (Key::Right, RIGHT),
    (Key::Left, LEFT),
    (Key::Up, UP),
    (Key::Down, DOWN),
    (Key::Z, A),
    (Key::X, B),
    (Key::Backspace, SELECT),
    (Key::Enter, START),
];

/// Host key that holds fast-forward.
const TURBO_KEY: Key = Key::Tab;
/// Host keys for the instant save / load save-state slot.
const SAVE_KEY: Key = Key::F5;
const LOAD_KEY: Key = Key::F7;

/// A frame's worth of host input, decoded into console-facing values.
pub struct InputState {
    /// Joypad byte for `P1::update_button_state` (0 = pressed).
    pub buttons: u8,
    /// Whether fast-forward is held this frame.
    pub turbo: bool,
    /// Whether the save-state / load-state keys are down this frame (the
    /// emulator edge-detects them so one press acts once).
    pub save: bool,
    pub load: bool,
}

/// Read the current host keyboard and map it to console input.
pub fn poll(display: &Display) -> InputState {
    let mut buttons = 0xFF;
    for &(key, bit) in KEY_MAP {
        if display.is_key_down(key) {
            buttons &= !bit;
        }
    }
    InputState {
        buttons,
        turbo: display.is_key_down(TURBO_KEY),
        save: display.is_key_down(SAVE_KEY),
        load: display.is_key_down(LOAD_KEY),
    }
}
