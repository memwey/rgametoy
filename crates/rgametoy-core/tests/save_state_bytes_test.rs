//! Roundtrip test for the byte-serialised save state. The machine is run for
//! a handful of frames so internal state (PC, framebuffer, APU frame
//! sequencer, APU sample buffer, etc.) is non-trivial, snapshotted, mutated
//! further, and restored — the post-restore machine must match the snapshot
//! exactly.
#![cfg(feature = "serialize")]

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::state::{crc32, SaveStateError, SAVE_STATE_MAGIC, SAVE_STATE_VERSION};
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
    c.power_on(cart);
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
    b.load_state_bytes(&blob)
        .expect("load_state_bytes should succeed");
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
fn load_state_bytes_rejects_trailing_payload_even_with_a_valid_crc() {
    let mut c = new_console();
    let good = c.save_state_bytes();
    let mut blob = good.clone();
    blob.push(0xAA);
    let crc = crc32(&blob[9..]);
    blob[5..9].copy_from_slice(&crc.to_le_bytes());

    assert_eq!(c.load_state_bytes(&blob), Err(SaveStateError::Corrupt));
    assert_eq!(c.save_state_bytes(), good, "rejected load is atomic");
}

#[test]
fn load_state_bytes_rejects_a_different_cartridge_shape() {
    let mut ram_rom = stub_rom();
    ram_rom[0x0147] = 0x03; // MBC1 + RAM + battery
    ram_rom[0x0149] = 0x02; // 8 KiB RAM
    let mut source = Console::new();
    source.power_on(Cartridge::from_bytes(ram_rom));
    let blob = source.save_state_bytes();

    let mut target = new_console(); // no-MBC, no external RAM
    let before = target.save_state_bytes();
    assert_eq!(target.load_state_bytes(&blob), Err(SaveStateError::Corrupt));
    assert_eq!(target.save_state_bytes(), before, "rejected load is atomic");
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
    b.load_state_bytes(&blob)
        .expect("load_state_bytes should succeed");

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

/// Loading a state must mark battery RAM dirty, not clean: the snapshot's RAM
/// may differ from what's persisted (.sav / IDB), so the frontend has to flush
/// it. Clearing the flag on load would drop a snapshot's unsaved battery RAM.
#[test]
fn loading_a_state_marks_ram_dirty_for_flushing() {
    let mut a = new_console();
    for _ in 0..3 {
        a.run_frame();
    }
    let blob = a.save_state_bytes();

    let mut b = new_console();
    b.cartridge_mut().clear_ram_dirty();
    assert!(!b.cartridge().ram_dirty(), "clean before load");
    b.load_state_bytes(&blob).expect("load");
    assert!(
        b.cartridge().ram_dirty(),
        "load must mark RAM dirty so the frontend persists it"
    );
}

/// A snapshot captured while `frame_ready` was set (e.g. taken mid-frame via
/// `step`) must not make the first `run_frame` after loading break immediately
/// and skip a frame — the flag is transient and must load as false.
#[test]
fn loaded_state_runs_a_full_first_frame_even_if_snapshot_was_frame_ready() {
    let mut a = new_console();
    // Step (not run_frame) past a frame boundary so `frame_ready` is set and
    // never consumed — exactly the state a mid-frame snapshot could capture.
    while a.total_cycles() < 70_224 {
        a.step();
    }
    let blob = a.save_state_bytes();

    let mut b = new_console();
    b.load_state_bytes(&blob).expect("load");
    let before = b.total_cycles();
    b.run_frame();
    assert!(
        b.total_cycles() - before > 60_000,
        "first frame after a frame-ready snapshot must still run a full frame"
    );
}

/// `EI` enables IME one instruction late, and that "one instruction" is tracked
/// across an instruction boundary — the exact point a save state is taken. A
/// round-trip inside that window must not shift when the interrupt dispatches.
#[test]
fn ei_delay_survives_a_save_state_round_trip() {
    /// `EI; NOP; …` with a VBlank interrupt already latched, stepped three
    /// times; returns PC (0x0040 once the dispatch happens). With
    /// `round_trip`, a save state is taken and reloaded after step 2 — while
    /// EI's promotion is still pending.
    fn run(round_trip: bool) -> u16 {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0100..0x0105].copy_from_slice(&[0xFB, 0x00, 0x00, 0x00, 0x00]); // EI; NOP×4
        let mut c = Console::new();
        c.power_on(Cartridge::from_bytes(rom));
        c.write_mem(0xFF40, 0x00); // LCD off, so the PPU raises nothing itself
        c.write_mem(0xFFFF, 0x01); // IE = VBlank
        c.write_mem(0xFF0F, 0x01); // IF = VBlank latched

        c.step(); // EI
        c.step(); // NOP — the promotion is now due at the next boundary
        if round_trip {
            let blob = c.save_state_bytes();
            c.load_state_bytes(&blob).expect("load");
        }
        c.step(); // dispatches the interrupt
        c.cpu().get_pc()
    }

    assert_eq!(run(false), 0x0040, "baseline: dispatch on the third step");
    assert_eq!(
        run(true),
        0x0040,
        "a save state taken in the EI delay window delayed the dispatch"
    );
}

/// IE (0xFFFF) is a plain 8-bit register — its top 3 bits are writable and read
/// back — so a state holding any IE value must reload, not be judged corrupt.
#[test]
fn ie_with_high_bits_set_survives_a_save_state_round_trip() {
    let mut a = new_console();
    a.write_mem(0xFFFF, 0xFF);
    assert_eq!(a.read_mem(0xFFFF), 0xFF, "IE reads back all 8 bits");
    let blob = a.save_state_bytes();

    let mut b = new_console();
    b.load_state_bytes(&blob)
        .expect("IE with high bits set must load");
    assert_eq!(b.read_mem(0xFFFF), 0xFF, "IE restored verbatim");
}
