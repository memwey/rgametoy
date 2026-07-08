//! Black-box test-ROM suite. These drive real community test ROMs end to end,
//! the ground truth the gray-box unit tests approximate.
//!
//! The ROMs are large and not redistributed here, so the suite is gated on the
//! `GB_TEST_ROMS` environment variable pointing at a c-sp/game-boy-test-roms
//! bundle root; without it every test skips (prints a note and returns). Run:
//!
//! ```sh
//! GB_TEST_ROMS=/path/to/game-boy-test-roms cargo test --release --test rom_suite
//! ```
//!
//! Only the self-signalling suites live here — mooneye (register signature)
//! and Blargg (serial). mealybug needs a reference image per test, which lives
//! next to each ROM in the bundle, so it is a manual scaffold instead:
//! `tools/mealybug_compare.py` (see docs/testing.md §2.4).

mod common;
use common::*;

fn skip() {
    eprintln!("rom_suite: set GB_TEST_ROMS to a game-boy-test-roms bundle root to run");
}

macro_rules! roms_root {
    () => {
        match roms_dir() {
            Some(d) => d,
            None => {
                skip();
                return;
            }
        }
    };
}

/// Every mooneye acceptance test that is in scope for a DMG core passes. The
/// out-of-scope failures are exactly the boot-ROM / other-model tests (their
/// file names start with `boot`), which need a real boot ROM we do not run.
#[test]
fn mooneye_acceptance_non_boot_all_pass() {
    let root = roms_root!();
    let dir = root.join("mooneye-test-suite").join("acceptance");
    let roms = find_roms(&dir);
    assert!(!roms.is_empty(), "no mooneye ROMs under {dir:?}");

    let mut failed = Vec::new();
    for rom in &roms {
        let name = rom.file_stem().unwrap().to_string_lossy().into_owned();
        if name.starts_with("boot") {
            continue; // needs a real boot ROM / specific model
        }
        let bytes = std::fs::read(rom).unwrap();
        if !mooneye_passes(&bytes) {
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "mooneye non-boot failures: {failed:?}");
}

/// Blargg's CPU and timing suites report "Passed" over the serial port.
#[test]
fn blargg_cpu_and_timing_pass() {
    let root = roms_root!();
    for sub in ["cpu_instrs", "instr_timing", "mem_timing"] {
        let rom = root.join("blargg").join(sub).join(format!("{sub}.gb"));
        let bytes = std::fs::read(&rom).unwrap_or_else(|_| panic!("missing {rom:?}"));
        let out = blargg_serial(&bytes);
        assert!(out.contains("Passed"), "{sub}: got {out:?}");
    }
}

/// Blargg's `dmg_sound` APU suite (reports via the `$A000` memory protocol, not
/// serial). We pass 9/12 — the fundamentals plus the length-counter obscure
/// behaviour (extra clock on enable, across power), sweep negate-mode disable,
/// and NR41-after-power. The 3 that remain are the wave-channel RAM access
/// quirks (read/trigger/write while the channel is playing), which need
/// cycle-exact wave-read timing — see docs/testing.md §3.9. Ratchet the passing
/// set so it can't regress; the failing set is documented, not asserted.
#[test]
fn blargg_dmg_sound_known_passing() {
    let root = roms_root!();
    let dir = root.join("blargg").join("dmg_sound").join("rom_singles");
    let passing = [
        "01-registers",
        "02-len ctr",
        "03-trigger",
        "04-sweep",
        "05-sweep details",
        "06-overflow on trigger",
        "07-len sweep period sync",
        "08-len ctr during power",
        "11-regs after power",
    ];
    let mut regressed = Vec::new();
    for name in passing {
        let rom = dir.join(format!("{name}.gb"));
        let bytes = std::fs::read(&rom).unwrap_or_else(|_| panic!("missing {rom:?}"));
        if blargg_ram_status(&bytes) != 0x00 {
            regressed.push(name);
        }
    }
    assert!(regressed.is_empty(), "dmg_sound regressions: {regressed:?}");
}
