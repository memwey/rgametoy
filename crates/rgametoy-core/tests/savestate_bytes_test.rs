//! Roundtrip test for the byte-serialised save state. The machine is run for
//! a handful of frames so internal state (PC, framebuffer, APU frame
//! sequencer, APU sample buffer, etc.) is non-trivial, snapshotted, mutated
//! further, and restored — the post-restore machine must match the snapshot
//! exactly.

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::state::{SAVE_STATE_MAGIC, SAVE_STATE_VERSION};
use rgametoy_core::Console;

/// A no-MBC 32 KB ROM filled with `RST 38h` (0xFF). RST 38h jumps to 0x0038,
/// which the boot ROM redirects back to 0x0100 (the entry point) on DMG —
/// the CPU therefore makes forward progress (PC wraps) without depending on
/// the bus being initialised.
fn stub_rom() -> Vec<u8> {
    let mut rom = vec![0xFFu8; 0x8000];
    // Entry point: NOP, then jump to self, so the CPU can keep stepping
    // without us needing to model the boot ROM's memory layout.
    rom[0x0100] = 0x00; // NOP
    rom[0x0101] = 0xC3; // JP nn
    rom[0x0102] = 0x00;
    rom[0x0103] = 0x01;
    rom
}

fn new_console() -> Console {
    let mut c = Console::new();
    let cart = Cartridge::from_bytes(stub_rom());
    c.load_cartridge(cart);
    c
}

#[test]
fn save_state_bytes_roundtrips_a_running_machine() {
    let mut a = new_console();
    for _ in 0..10 {
        a.run_frame();
    }
    let blob = a.save_state_bytes();

    // Header sanity.
    assert_eq!(&blob[0..4], &SAVE_STATE_MAGIC);
    assert_eq!(blob[4], SAVE_STATE_VERSION);

    // Restore into a fresh machine and re-serialise: the definitive round-trip
    // check. Every field that was written must read back and re-write
    // identically, so a dropped, reordered or defaulted-on-load field shows up
    // as a byte diff here — a framebuffer compare would miss it for a ROM that
    // renders nothing.
    let mut b = new_console();
    b.load_state_bytes(&blob).expect("load_state_bytes should succeed");
    assert_eq!(
        b.save_state_bytes(),
        blob,
        "re-serialised state differs from the snapshot => a field does not round-trip"
    );
}

#[test]
fn load_state_bytes_rejects_bad_magic() {
    let mut c = new_console();
    let mut blob = c.save_state_bytes();
    blob[0] = b'X';
    assert!(c.load_state_bytes(&blob).is_err());
}

#[test]
fn load_state_bytes_rejects_truncated_body() {
    let mut c = new_console();
    let blob = c.save_state_bytes();
    let truncated = &blob[..blob.len() - 50];
    assert!(c.load_state_bytes(truncated).is_err());
}

#[test]
fn load_state_bytes_rejects_corrupt_crc() {
    let mut c = new_console();
    for _ in 0..5 {
        c.run_frame();
    }
    let good = c.save_state_bytes();
    let mut blob = good.clone();
    // Flip a byte in the middle of the body.
    let mid = blob.len() / 2;
    blob[mid] ^= 0x01;
    assert!(c.load_state_bytes(&blob).is_err());
    // A rejected load must leave the live machine untouched (atomic apply).
    assert_eq!(
        c.save_state_bytes(),
        good,
        "a rejected load mutated the live machine"
    );
}

#[test]
fn save_state_bytes_keeps_machines_in_lockstep() {
    // Two consoles that diverge after snapshot+load should produce the same
    // output for as long as we run them — the snapshot is a true checkpoint,
    // not a one-shot approximation.
    let mut a = new_console();
    for _ in 0..7 {
        a.run_frame();
    }
    let blob = a.save_state_bytes();

    let mut b = new_console();
    b.load_state_bytes(&blob).expect("load_state_bytes should succeed");

    // Compare the *whole* serialised machine each frame (not just the pixels),
    // so a restore that got any internal state wrong — cycle counter, APU
    // sequencer, PPU dot, timer — diverges immediately even for a ROM that
    // never draws anything.
    for i in 0..30 {
        a.run_frame();
        b.run_frame();
        assert_eq!(
            a.save_state_bytes(),
            b.save_state_bytes(),
            "machines diverged at frame {i}"
        );
    }
}
