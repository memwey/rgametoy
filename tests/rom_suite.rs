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
//! The mealybug reference images *are* embedded (decoded to 2bpp shade data),
//! so the pixel comparison is self-contained and needs no PNG decoder.

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

/// Embedded mealybug DMG references (decoded from `*_dmg_blob.png`, MIT ©
/// Matt Currie / mealybug-tearoom-tests) paired with the current pixel-match
/// count as a regression floor. mealybug is the strictest PPU suite; a full
/// PASS needs 160x144 exact, so most of these are ratchets, not passes yet.
const MEALYBUG: &[(&str, &[u8], usize)] = &[
    ("m2_win_en_toggle", include_bytes!("fixtures/mealybug/m2_win_en_toggle.bin"), 23040),
    ("m3_bgp_change", include_bytes!("fixtures/mealybug/m3_bgp_change.bin"), 17956),
    ("m3_bgp_change_sprites", include_bytes!("fixtures/mealybug/m3_bgp_change_sprites.bin"), 17304),
    ("m3_lcdc_bg_en_change", include_bytes!("fixtures/mealybug/m3_lcdc_bg_en_change.bin"), 20354),
    ("m3_lcdc_bg_map_change", include_bytes!("fixtures/mealybug/m3_lcdc_bg_map_change.bin"), 22070),
    ("m3_lcdc_obj_en_change", include_bytes!("fixtures/mealybug/m3_lcdc_obj_en_change.bin"), 22894),
    ("m3_lcdc_obj_en_change_variant", include_bytes!("fixtures/mealybug/m3_lcdc_obj_en_change_variant.bin"), 22042),
    ("m3_lcdc_obj_size_change", include_bytes!("fixtures/mealybug/m3_lcdc_obj_size_change.bin"), 22670),
    ("m3_lcdc_obj_size_change_scx", include_bytes!("fixtures/mealybug/m3_lcdc_obj_size_change_scx.bin"), 22850),
    ("m3_lcdc_tile_sel_change", include_bytes!("fixtures/mealybug/m3_lcdc_tile_sel_change.bin"), 20986),
    ("m3_lcdc_tile_sel_win_change", include_bytes!("fixtures/mealybug/m3_lcdc_tile_sel_win_change.bin"), 21298),
    ("m3_lcdc_win_en_change_multiple", include_bytes!("fixtures/mealybug/m3_lcdc_win_en_change_multiple.bin"), 14724),
    ("m3_lcdc_win_en_change_multiple_wx", include_bytes!("fixtures/mealybug/m3_lcdc_win_en_change_multiple_wx.bin"), 17000),
    ("m3_lcdc_win_map_change", include_bytes!("fixtures/mealybug/m3_lcdc_win_map_change.bin"), 21628),
    ("m3_obp0_change", include_bytes!("fixtures/mealybug/m3_obp0_change.bin"), 22608),
    ("m3_scx_high_5_bits", include_bytes!("fixtures/mealybug/m3_scx_high_5_bits.bin"), 22956),
    ("m3_scx_low_3_bits", include_bytes!("fixtures/mealybug/m3_scx_low_3_bits.bin"), 22500),
    ("m3_scy_change", include_bytes!("fixtures/mealybug/m3_scy_change.bin"), 13252),
    ("m3_window_timing", include_bytes!("fixtures/mealybug/m3_window_timing.bin"), 21577),
    ("m3_window_timing_wx_0", include_bytes!("fixtures/mealybug/m3_window_timing_wx_0.bin"), 22773),
    ("m3_wx_4_change", include_bytes!("fixtures/mealybug/m3_wx_4_change.bin"), 22811),
    ("m3_wx_4_change_sprites", include_bytes!("fixtures/mealybug/m3_wx_4_change_sprites.bin"), 23030),
    ("m3_wx_5_change", include_bytes!("fixtures/mealybug/m3_wx_5_change.bin"), 22402),
    ("m3_wx_6_change", include_bytes!("fixtures/mealybug/m3_wx_6_change.bin"), 9241),
];

/// No mealybug test may render fewer matching pixels than its committed floor
/// (a PPU-accuracy ratchet). `m2_win_en_toggle`'s floor is the full 23040, so
/// this also pins the one test that already passes pixel-perfect.
#[test]
fn mealybug_no_regression_vs_embedded_refs() {
    let root = roms_root!();
    let dir = root.join("mealybug-tearoom-tests").join("ppu");

    let mut regressions = Vec::new();
    for (name, packed, floor) in MEALYBUG {
        let rom = dir.join(format!("{name}.gb"));
        let bytes = std::fs::read(&rom).unwrap_or_else(|_| panic!("missing {rom:?}"));
        let fb = render_rom(&bytes, 30);
        let matches = pixel_matches(&fb, &unpack_ref(packed));
        if matches < *floor {
            regressions.push(format!("{name}: {matches} < floor {floor}"));
        }
    }
    assert!(
        regressions.is_empty(),
        "mealybug pixel regressions:\n  {}",
        regressions.join("\n  ")
    );
}
