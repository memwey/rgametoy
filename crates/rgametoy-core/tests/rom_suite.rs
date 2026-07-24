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

/// GBMicrotest: a large set of very small, very sharp timing probes. We pass
/// 333 of 513. Like `dmg_sound` this is a **ratchet**, not a target: the
/// passing set is listed so it cannot silently regress, and the failures are
/// documented rather than asserted.
///
/// The failures cluster hard, and the shape is the useful part: ~25 are
/// `poweron_*` (they need a real boot ROM, out of scope exactly like mooneye's
/// `boot*` set), and almost all the rest are PPU/STAT interrupt timing —
/// `hblank_int_*` alone is 37. See docs/testing.md §2.6.
#[test]
fn gbmicrotest_known_passing() {
    let root = roms_root!();
    let dir = root.join("gbmicrotest");
    const PASSING: &[&str] = &[
        "div_inc_timing_a",
        "div_inc_timing_b",
        "dma_0x1000",
        "dma_0x9000",
        "dma_0xA000",
        "dma_0xC000",
        "dma_0xE000",
        "dma_timing_a",
        "halt_bug",
        "halt_op_dupe",
        "hblank_int_di_timing_a",
        "hblank_int_di_timing_b",
        "hblank_int_if_b",
        "hblank_int_l1",
        "hblank_int_l2",
        "hblank_int_scx0",
        "hblank_int_scx0_if_d",
        "hblank_int_scx1_if_d",
        "hblank_int_scx1_nops_a",
        "hblank_int_scx2_if_d",
        "hblank_int_scx2_nops_a",
        "hblank_int_scx3",
        "hblank_int_scx3_if_d",
        "hblank_int_scx3_nops_a",
        "hblank_int_scx4",
        "hblank_int_scx4_if_d",
        "hblank_int_scx4_nops_a",
        "hblank_int_scx5_if_d",
        "hblank_int_scx5_nops_a",
        "hblank_int_scx6_if_d",
        "hblank_int_scx6_nops_a",
        "hblank_int_scx7",
        "hblank_int_scx7_if_d",
        "hblank_int_scx7_nops_a",
        "hblank_scx2_if_a",
        "hblank_scx3_if_a",
        "hblank_scx3_if_c",
        "hblank_scx3_if_d",
        "hblank_scx3_int_a",
        "hblank_scx3_int_b",
        "int_hblank_halt_bug_a",
        "int_hblank_halt_bug_b",
        "int_oam_halt",
        "int_timer_halt",
        "int_timer_halt_div_a",
        "int_timer_halt_div_b",
        "int_timer_nops_div_b",
        "int_vblank1_halt",
        "int_vblank1_incs",
        "int_vblank1_nops",
        "int_vblank2_halt",
        "int_vblank2_incs",
        "int_vblank2_nops",
        "lcdon_halt_to_vblank_int_a",
        "lcdon_halt_to_vblank_int_b",
        "lcdon_nops_to_vblank_int_a",
        "lcdon_nops_to_vblank_int_b",
        "lcdon_to_if_oam_a",
        "lcdon_to_if_oam_b",
        "lcdon_to_ly1_a",
        "lcdon_to_ly1_b",
        "lcdon_to_ly2_a",
        "lcdon_to_ly2_b",
        "lcdon_to_ly3_a",
        "lcdon_to_ly3_b",
        "lcdon_to_oam_unlock_a",
        "lcdon_to_oam_unlock_b",
        "lcdon_to_oam_unlock_c",
        "lcdon_to_oam_unlock_d",
        "lcdon_to_stat0_a",
        "lcdon_to_stat0_b",
        "lcdon_to_stat0_c",
        "lcdon_to_stat0_d",
        "lcdon_to_stat1_a",
        "lcdon_to_stat1_b",
        "lcdon_to_stat1_e",
        "lcdon_to_stat2_a",
        "lcdon_to_stat2_b",
        "lcdon_to_stat2_c",
        "lcdon_to_stat2_d",
        "lcdon_to_stat3_a",
        "lcdon_to_stat3_b",
        "lcdon_to_stat3_c",
        "lcdon_to_stat3_d",
        "line_144_oam_int_a",
        "line_153_ly_a",
        "line_153_ly_b",
        "line_153_ly_e",
        "line_153_ly_f",
        "line_153_lyc0_stat_timing_a",
        "line_153_lyc0_stat_timing_b",
        "line_153_lyc0_stat_timing_c",
        "line_153_lyc0_stat_timing_g",
        "line_153_lyc0_stat_timing_h",
        "line_153_lyc0_stat_timing_i",
        "line_153_lyc0_stat_timing_j",
        "line_153_lyc0_stat_timing_k",
        "line_153_lyc0_stat_timing_l",
        "line_153_lyc0_stat_timing_m",
        "line_153_lyc0_stat_timing_n",
        "line_153_lyc153_stat_timing_a",
        "line_153_lyc153_stat_timing_b",
        "line_153_lyc153_stat_timing_f",
        "line_153_lyc_a",
        "line_153_lyc_b",
        "line_153_lyc_int_b",
        "lyc1_int_halt_b",
        "lyc1_int_if_edge_a",
        "lyc1_int_if_edge_b",
        "lyc1_int_if_edge_c",
        "lyc1_int_if_edge_d",
        "lyc1_int_nops_b",
        "lyc1_write_timing_a",
        "lyc1_write_timing_b",
        "lyc1_write_timing_c",
        "lyc1_write_timing_d",
        "lyc2_int_halt_b",
        "lyc_int_halt_b",
        "mbc1_ram_banks",
        "mbc1_rom_banks",
        "oam_int_if_edge_a",
        "oam_int_if_edge_b",
        "oam_int_if_edge_c",
        "oam_int_if_edge_d",
        "oam_int_if_level_c",
        "oam_read_l0_a",
        "oam_read_l0_b",
        "oam_read_l0_c",
        "oam_read_l0_d",
        "oam_read_l1_a",
        "oam_read_l1_b",
        "oam_read_l1_c",
        "oam_read_l1_d",
        "oam_read_l1_e",
        "oam_read_l1_f",
        "oam_write_l0_a",
        "oam_write_l0_b",
        "oam_write_l0_c",
        "oam_write_l0_d",
        "oam_write_l0_e",
        "oam_write_l1_a",
        "oam_write_l1_b",
        "oam_write_l1_c",
        "oam_write_l1_d",
        "oam_write_l1_e",
        "oam_write_l1_f",
        "poweron_bgp_000",
        "poweron_div_000",
        "poweron_div_004",
        "poweron_div_005",
        "poweron_if_000",
        "poweron_joy_000",
        "poweron_lcdc_000",
        "poweron_ly_000",
        "poweron_ly_120",
        "poweron_ly_234",
        "poweron_lyc_000",
        "poweron_oam_000",
        "poweron_oam_005",
        "poweron_oam_070",
        "poweron_oam_120",
        "poweron_oam_121",
        "poweron_oam_184",
        "poweron_oam_234",
        "poweron_oam_235",
        "poweron_obp0_000",
        "poweron_obp1_000",
        "poweron_sb_000",
        "poweron_sc_000",
        "poweron_scx_000",
        "poweron_scy_000",
        "poweron_stat_006",
        "poweron_stat_027",
        "poweron_stat_070",
        "poweron_stat_121",
        "poweron_stat_141",
        "poweron_stat_184",
        "poweron_stat_235",
        "poweron_tac_000",
        "poweron_tima_000",
        "poweron_tma_000",
        "poweron_vram_000",
        "poweron_vram_026",
        "poweron_vram_070",
        "poweron_vram_140",
        "poweron_vram_184",
        "poweron_wx_000",
        "poweron_wy_000",
        "ppu_sprite0_scx0_a",
        "ppu_sprite0_scx0_b",
        "ppu_sprite0_scx1_a",
        "ppu_sprite0_scx1_b",
        "ppu_sprite0_scx2_a",
        "ppu_sprite0_scx2_b",
        "ppu_sprite0_scx3_a",
        "ppu_sprite0_scx3_b",
        "ppu_sprite0_scx4_a",
        "ppu_sprite0_scx4_b",
        "ppu_sprite0_scx5_a",
        "ppu_sprite0_scx5_b",
        "ppu_sprite0_scx6_a",
        "ppu_sprite0_scx6_b",
        "ppu_sprite0_scx7_a",
        "ppu_sprite0_scx7_b",
        "sprite4_0_a",
        "sprite4_0_b",
        "sprite4_1_a",
        "sprite4_1_b",
        "sprite4_2_a",
        "sprite4_2_b",
        "sprite4_3_a",
        "sprite4_3_b",
        "sprite4_4_a",
        "sprite4_4_b",
        "sprite4_5_a",
        "sprite4_5_b",
        "sprite4_6_a",
        "sprite4_6_b",
        "sprite4_7_a",
        "sprite4_7_b",
        "sprite_0_a",
        "sprite_0_b",
        "sprite_1_a",
        "sprite_1_b",
        "stat_write_glitch_l0_c",
        "stat_write_glitch_l143_a",
        "stat_write_glitch_l154_c",
        "stat_write_glitch_l1_a",
        "stat_write_glitch_l1_d",
        "timer_div_phase_c",
        "timer_div_phase_d",
        "timer_tima_inc_256k_a",
        "timer_tima_inc_256k_b",
        "timer_tima_inc_256k_c",
        "timer_tima_inc_256k_d",
        "timer_tima_inc_256k_e",
        "timer_tima_inc_256k_f",
        "timer_tima_inc_256k_g",
        "timer_tima_inc_256k_h",
        "timer_tima_inc_256k_i",
        "timer_tima_inc_256k_j",
        "timer_tima_inc_256k_k",
        "timer_tima_inc_64k_a",
        "timer_tima_inc_64k_b",
        "timer_tima_inc_64k_c",
        "timer_tima_inc_64k_d",
        "timer_tima_phase_a",
        "timer_tima_phase_b",
        "timer_tima_phase_c",
        "timer_tima_phase_d",
        "timer_tima_phase_e",
        "timer_tima_phase_f",
        "timer_tima_phase_g",
        "timer_tima_phase_h",
        "timer_tima_phase_i",
        "timer_tima_phase_j",
        "timer_tima_reload_256k_a",
        "timer_tima_reload_256k_b",
        "timer_tima_reload_256k_c",
        "timer_tima_reload_256k_d",
        "timer_tima_reload_256k_e",
        "timer_tima_reload_256k_f",
        "timer_tima_reload_256k_g",
        "timer_tima_reload_256k_h",
        "timer_tima_reload_256k_i",
        "timer_tima_reload_256k_j",
        "timer_tima_reload_256k_k",
        "timer_tima_write_a",
        "timer_tima_write_b",
        "timer_tima_write_c",
        "timer_tima_write_d",
        "timer_tima_write_e",
        "timer_tima_write_f",
        "timer_tma_write_a",
        "timer_tma_write_b",
        "vblank2_int_halt_a",
        "vblank2_int_halt_b",
        "vblank2_int_if_b",
        "vblank2_int_if_d",
        "vblank2_int_inc_sled",
        "vblank2_int_nops_a",
        "vblank2_int_nops_b",
        "vblank_int_halt_a",
        "vblank_int_halt_b",
        "vblank_int_if_b",
        "vblank_int_if_d",
        "vblank_int_inc_sled",
        "vblank_int_nops_a",
        "vblank_int_nops_b",
        "vram_read_l0_a",
        "vram_read_l0_b",
        "vram_read_l0_c",
        "vram_read_l0_d",
        "vram_read_l1_a",
        "vram_read_l1_b",
        "vram_read_l1_c",
        "vram_read_l1_d",
        "vram_write_l0_a",
        "vram_write_l0_b",
        "vram_write_l0_c",
        "vram_write_l0_d",
        "vram_write_l1_a",
        "vram_write_l1_b",
        "vram_write_l1_c",
        "vram_write_l1_d",
        "win0_a",
        "win0_scx3_a",
        "win10_a",
        "win10_b",
        "win10_scx3_a",
        "win10_scx3_b",
        "win11_a",
        "win11_b",
        "win12_a",
        "win12_b",
        "win13_a",
        "win13_b",
        "win14_a",
        "win14_b",
        "win15_a",
        "win15_b",
        "win1_a",
        "win2_a",
        "win3_a",
        "win4_a",
        "win5_a",
        "win6_a",
        "win7_a",
        "win7_b",
        "win8_a",
        "win8_b",
        "win9_a",
        "win9_b",
    ];
    let mut regressed = Vec::new();
    for name in PASSING {
        let rom = dir.join(format!("{name}.gb"));
        let bytes = std::fs::read(&rom).unwrap_or_else(|_| panic!("missing {rom:?}"));
        if !gbmicrotest_passes(&bytes) {
            regressed.push(*name);
        }
    }
    assert!(
        regressed.is_empty(),
        "gbmicrotest regressions: {regressed:?}"
    );
}
