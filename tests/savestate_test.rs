extern crate rgametoy;

use rgametoy::console::bus::Bus;
use rgametoy::console::Console;

/// A full snapshot restores CPU and memory state exactly.
#[test]
fn save_and_load_state_restores_the_machine() {
    let mut console = Console::new();
    // INC A ; JR -3  — the accumulator increments every loop iteration.
    console.load_program(&[0x3C, 0x18, 0xFD]);
    console.get_bus_mut().write_byte(0xC000, 0xAA); // mark WRAM

    for _ in 0..10 {
        console.step();
    }
    let state = console.save_state();
    let a_at_snapshot = console.get_cpu().get_registers().get_a();
    let pc_at_snapshot = console.get_cpu().get_pc();

    // Run further and mutate memory so the machine diverges from the snapshot.
    for _ in 0..20 {
        console.step();
    }
    console.get_bus_mut().write_byte(0xC000, 0xBB);
    assert_ne!(
        console.get_cpu().get_registers().get_a(),
        a_at_snapshot,
        "state actually changed"
    );

    // Restore, and everything should be back at the snapshot.
    console.load_state(&state);
    assert_eq!(console.get_cpu().get_registers().get_a(), a_at_snapshot, "CPU restored");
    assert_eq!(console.get_cpu().get_pc(), pc_at_snapshot, "PC restored");
    assert_eq!(console.get_bus_mut().read_byte(0xC000), 0xAA, "WRAM restored");
}

/// A snapshot is independent of later mutation of the live machine (deep copy).
#[test]
fn snapshot_is_a_deep_copy() {
    let mut console = Console::new();
    console.get_bus_mut().write_byte(0xC000, 0x11);
    let state = console.save_state();

    console.get_bus_mut().write_byte(0xC000, 0x22);
    console.load_state(&state);
    assert_eq!(console.get_bus_mut().read_byte(0xC000), 0x11);
}
