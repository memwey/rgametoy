extern crate rgametoy;

use rgametoy::console::bus::{Bus, MemoryBus};

// Button bits (0 = pressed): low nibble directions, high nibble actions.
const RIGHT: u8 = 0x01;
const A: u8 = 0x10;

// Select writes to 0xFF00 (0 = selected): P14 bit 4 = directions, P15 bit 5 =
// actions.
const SELECT_DIRECTIONS: u8 = 0x20; // P14=0, P15=1
const SELECT_ACTIONS: u8 = 0x10; // P15=0, P14=1

fn press(bits: u8) -> u8 {
    !bits
}

/// Whether the Joypad interrupt (IF bit 4) is currently requested.
fn joypad_irq(bus: &MemoryBus) -> bool {
    bus.read_byte(0xFF0F) & 0x10 != 0
}

fn clear_if(bus: &mut MemoryBus) {
    bus.write_byte(0xFF0F, 0x00);
}

/// Selecting the direction pad reads the D-pad, not the action buttons.
#[test]
fn selecting_directions_reads_the_dpad() {
    let mut bus = MemoryBus::new();
    bus.set_buttons(press(RIGHT | A)); // hold Right and A
    bus.write_byte(0xFF00, SELECT_DIRECTIONS);
    assert_eq!(bus.read_byte(0xFF00) & 0x01, 0, "Right (a direction) reads pressed");
}

/// Selecting the action buttons reads the buttons, not the D-pad — so a held
/// direction is invisible while actions are selected.
#[test]
fn selecting_actions_reads_the_buttons() {
    let mut bus = MemoryBus::new();
    bus.set_buttons(press(A));
    bus.write_byte(0xFF00, SELECT_ACTIONS);
    assert_eq!(bus.read_byte(0xFF00) & 0x01, 0, "A (an action) reads pressed on P10");

    bus.set_buttons(press(RIGHT)); // only a direction held
    assert_eq!(
        bus.read_byte(0xFF00) & 0x0F,
        0x0F,
        "no action pressed → all input lines read high"
    );
}

/// A press in the selected group raises the joypad interrupt.
#[test]
fn press_in_the_selected_group_interrupts() {
    let mut bus = MemoryBus::new();
    bus.write_byte(0xFF00, SELECT_DIRECTIONS);
    clear_if(&mut bus);
    bus.set_buttons(press(RIGHT));
    assert!(joypad_irq(&bus), "pressing Right while directions are selected interrupts");
}

/// A press in an *unselected* group raises nothing — the reported bug: polling
/// the D-pad must not be disturbed by A/B presses.
#[test]
fn press_in_an_unselected_group_does_not_interrupt() {
    let mut bus = MemoryBus::new();
    bus.write_byte(0xFF00, SELECT_DIRECTIONS); // only directions selected
    clear_if(&mut bus);
    bus.set_buttons(press(A));
    assert!(!joypad_irq(&bus), "pressing A while directions are selected must not interrupt");
}

/// Releasing a button is a low→high edge and raises nothing.
#[test]
fn releasing_a_button_does_not_interrupt() {
    let mut bus = MemoryBus::new();
    bus.write_byte(0xFF00, SELECT_DIRECTIONS);
    bus.set_buttons(press(RIGHT));
    clear_if(&mut bus);
    bus.set_buttons(0xFF); // release
    assert!(!joypad_irq(&bus), "releasing is a low→high edge, no interrupt");
}

/// Re-selecting a group in which a button is already held is itself a high→low
/// edge, so the FF00 write raises the interrupt.
#[test]
fn selecting_a_group_with_a_held_button_interrupts() {
    let mut bus = MemoryBus::new();
    bus.set_buttons(press(RIGHT)); // hold Right
    bus.write_byte(0xFF00, SELECT_ACTIONS); // Right not visible
    clear_if(&mut bus);
    bus.write_byte(0xFF00, SELECT_DIRECTIONS); // exposes the held Right
    assert!(joypad_irq(&bus), "switching to directions exposes the held Right");
}
