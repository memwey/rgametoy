//! Black-box save-state restore checks via the observable machine (CPU
//! registers, WRAM) — a complement to the byte-level round-trip in
//! `savestate_bytes_test.rs`. Gated on the `serialize` feature that provides
//! the save-state API.
#![cfg(feature = "serialize")]

extern crate rgametoy_core;

use rgametoy_core::Console;

/// A full snapshot restores CPU and memory state exactly.
#[test]
fn save_and_load_state_restores_the_machine() {
    let mut console = Console::new();
    // INC A ; JR -3  — the accumulator increments every loop iteration.
    console.load_program(&[0x3C, 0x18, 0xFD]);
    console.write_mem(0xC000, 0xAA); // mark WRAM

    for _ in 0..10 {
        console.step();
    }
    let state = console.save_state_bytes();
    let a_at_snapshot = console.cpu().get_registers().get_a();
    let pc_at_snapshot = console.cpu().get_pc();

    // Run further and mutate memory so the machine diverges from the snapshot.
    for _ in 0..20 {
        console.step();
    }
    console.write_mem(0xC000, 0xBB);
    assert_ne!(
        console.cpu().get_registers().get_a(),
        a_at_snapshot,
        "state actually changed"
    );

    // Restore, and everything should be back at the snapshot.
    console.load_state_bytes(&state).expect("load");
    assert_eq!(console.cpu().get_registers().get_a(), a_at_snapshot, "CPU restored");
    assert_eq!(console.cpu().get_pc(), pc_at_snapshot, "PC restored");
    assert_eq!(console.read_mem(0xC000), 0xAA, "WRAM restored");
}

/// A serialized snapshot is independent of later mutation of the live machine.
#[test]
fn snapshot_is_independent_of_later_writes() {
    let mut console = Console::new();
    console.write_mem(0xC000, 0x11);
    let state = console.save_state_bytes();

    console.write_mem(0xC000, 0x22);
    console.load_state_bytes(&state).expect("load");
    assert_eq!(console.read_mem(0xC000), 0x11);
}
