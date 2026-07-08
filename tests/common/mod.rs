//! Shared test scaffolding. Two layers:
//!   * gray-box PPU helpers used by the fast module unit tests, and
//!   * a black-box test-ROM harness used by the (env-gated) `rom_suite`.
//!
//! Included per test crate via `mod common;`, so a given file only exercises a
//! subset — hence the blanket dead-code allow.
#![allow(dead_code)]

use rgametoy::console::cartridge::Cartridge;
use rgametoy::console::ppu::Ppu;
use rgametoy::console::Console;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Gray-box PPU helpers (drive through registers/OAM/VRAM, observe internals).
// ---------------------------------------------------------------------------

/// A PPU with the LCD enabled (LCDC bit 7) so its state machine runs.
pub fn enabled_ppu() -> Ppu {
    let mut ppu = Ppu::new();
    ppu.write_register(0xFF40, 0x80);
    ppu
}

/// Fill an 8x8 tile at VRAM `addr` so every pixel has the given colour index
/// (0-3): the low bit-plane is set for colour bit 0, the high plane for bit 1.
pub fn write_solid_tile(ppu: &mut Ppu, addr: u16, colour: u8) {
    let lo = if colour & 1 != 0 { 0xFF } else { 0x00 };
    let hi = if colour & 2 != 0 { 0xFF } else { 0x00 };
    for row in 0..8u16 {
        ppu.write_vram(addr + row * 2, lo);
        ppu.write_vram(addr + row * 2 + 1, hi);
    }
}

/// Drive the PPU line by line until one frame is ready.
pub fn render_frame(ppu: &mut Ppu) {
    for _ in 0..2000 {
        ppu.tick(200);
        if ppu.take_frame_ready() {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Black-box test-ROM harness (used by the env-gated rom_suite).
// ---------------------------------------------------------------------------

/// Root of the community test-ROM bundle (c-sp/game-boy-test-roms layout),
/// from `GB_TEST_ROMS`. `None` (test skips) when it is not set.
pub fn roms_dir() -> Option<PathBuf> {
    std::env::var_os("GB_TEST_ROMS").map(PathBuf::from)
}

fn boot(rom: &[u8]) -> Console {
    let mut c = Console::new();
    c.load_cartridge(Cartridge::from_bytes(rom.to_vec()));
    c
}

fn mooneye_signature(c: &Console) -> bool {
    let r = c.get_cpu().get_registers();
    r.get_b() == 3 && r.get_c() == 5 && r.get_d() == 8 && r.get_e() == 13 && r.get_h() == 21
        && r.get_l() == 34
}

/// Run a mooneye ROM and report whether it loaded the pass signature
/// (Fibonacci 3,5,8,13,21,34 in B,C,D,E,H,L). Most finish within 240 frames;
/// `intr_2_mode0_timing_sprites` needs ~2900, so fall back to 3000.
pub fn mooneye_passes(rom: &[u8]) -> bool {
    let mut c = boot(rom);
    for _ in 0..240 {
        c.run_frame();
    }
    if mooneye_signature(&c) {
        return true;
    }
    for _ in 0..2760 {
        c.run_frame();
    }
    mooneye_signature(&c)
}

/// Run a Blargg ROM until it reports over the serial port; returns the text.
pub fn blargg_serial(rom: &[u8]) -> String {
    let mut c = boot(rom);
    let mut out = String::new();
    for _ in 0..6000 {
        c.run_frame();
        let bytes = c.take_serial_output();
        if !bytes.is_empty() {
            out.push_str(&String::from_utf8_lossy(&bytes));
        }
        if out.contains("Passed") || out.contains("Failed") {
            break;
        }
    }
    out
}

/// Recursively collect `*.gb` files under `dir`.
pub fn find_roms(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(find_roms(&p));
            } else if p.extension().and_then(|s| s.to_str()) == Some("gb") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
