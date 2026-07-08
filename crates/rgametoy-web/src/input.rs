//! Frontend keyboard mapping: translates DOM `KeyboardEvent.code` values into
//! the Game Boy joypad byte the core's P1 register consumes, plus edge-detected
//! meta keys (save/load/screenshot/palette-cycle). The mapping is the *only*
//! place that knows which host key means which console button, keeping the
//! bindings separate from both the canvas and the emulated joypad hardware.
//!
//! We use `event.code` (physical key identifier, e.g. `KeyZ`, `ArrowRight`)
//! rather than `event.key` (post-layout string, e.g. `z`, `Z`, `Z` on a
//! German keyboard) so the bindings stay the same regardless of the user's
//! keyboard layout. The user can still type `Z` to chat while playing.
//!
//! The hotkeys for save/load/screenshot/palette are `Digit5`, `Digit7`,
//! `Digit2`, `Digit3` respectively — we deliberately don't reuse the desktop
//! crate's F-keys so this frontend can be driven from a laptop without
//! modifier acrobatics.

/// Game Boy button bits, matching the byte P1 consumes. A set bit means the
/// button is *released*; a cleared bit means pressed.
pub const RIGHT: u8 = 0x01;
pub const LEFT: u8 = 0x02;
pub const UP: u8 = 0x04;
pub const DOWN: u8 = 0x08;
pub const A: u8 = 0x10;
pub const B: u8 = 0x20;
pub const SELECT: u8 = 0x40;
pub const START: u8 = 0x80;

/// Host `event.code` → Game Boy button bit.
const KEY_MAP: &[(&str, u8)] = &[
    ("ArrowRight", RIGHT),
    ("ArrowLeft", LEFT),
    ("ArrowUp", UP),
    ("ArrowDown", DOWN),
    ("KeyZ", A),
    ("KeyX", B),
    ("Backspace", SELECT),
    ("Enter", START),
];

/// A frame's worth of decoded host input.
#[derive(Default, Clone, Copy, Debug)]
pub struct InputState {
    /// Joypad byte for `P1::update_button_state` (0 = pressed). Starts at
    /// 0xFF (all released) so unmapped keys can't accidentally press a
    /// button.
    pub buttons: u8,
    /// Fast-forward: while held, the rAF loop runs more than one frame
    /// per animation tick. Empty by default.
    pub turbo: bool,
    /// `Digit5` / `Digit7`: save / load the instant save-state slot.
    pub save: bool,
    pub load: bool,
    /// `Digit2`: download a PNG screenshot of the current frame.
    pub screenshot: bool,
    /// `Digit3`: cycle the display palette.
    pub palette_cycle: bool,
}

/// Compute the [`InputState`] for a single key event. The caller is
/// responsible for edge-detecting the meta keys (we just report which are
/// down right now); the joypad byte is computed by OR-clearing every
/// matching mapped key.
pub fn from_keydown(code: &str) -> InputState {
    let mut state = InputState {
        buttons: 0xFF,
        ..Default::default()
    };
    for &(key, bit) in KEY_MAP {
        if key == code {
            state.buttons &= !bit;
        }
    }
    state.turbo = code == "Tab";
    state.save = code == "Digit5";
    state.load = code == "Digit7";
    state.screenshot = code == "Digit2";
    state.palette_cycle = code == "Digit3";
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_keys_clear_directions() {
        let s = from_keydown("ArrowRight");
        assert_eq!(s.buttons, 0xFF & !RIGHT);
        assert!(!s.turbo);
        assert!(!s.save);
    }

    #[test]
    fn zxa_clear_action_buttons() {
        assert_eq!(from_keydown("KeyZ").buttons, 0xFF & !A);
        assert_eq!(from_keydown("KeyX").buttons, 0xFF & !B);
    }

    #[test]
    fn digit_keys_set_meta_flags() {
        assert!(from_keydown("Digit5").save);
        assert!(!from_keydown("Digit5").load);
        assert!(from_keydown("Digit7").load);
        assert!(!from_keydown("Digit7").save);
        assert!(from_keydown("Digit2").screenshot);
        assert!(from_keydown("Digit3").palette_cycle);
        assert!(from_keydown("Tab").turbo);
    }

    #[test]
    fn unknown_key_is_no_op() {
        let s = from_keydown("KeyQ");
        assert_eq!(s.buttons, 0xFF);
        assert!(!s.turbo && !s.save && !s.load && !s.screenshot && !s.palette_cycle);
    }
}
