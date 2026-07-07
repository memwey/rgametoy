//! PPU timing calibration probe: measure mode-3 length across SCX / sprites,
//! and the line-relative dots at which the mode-2 STAT interrupt fires and the
//! mode transitions happen. Used to calibrate against hardware reference values
//! (mode 3 = 172 + (SCX&7) dots at DMG; mode 2 / mode 3 start at dot 0 / 80).

use rgametoy::console::ppu::{Ppu, PpuMode};

fn mode3_len(scx: u8, sprite: bool) -> u32 {
    let mut ppu = Ppu::new();
    if sprite {
        ppu.write_oam(0xFE00, 16);
        ppu.write_oam(0xFE01, 8);
        ppu.write_register(0xFF40, 0x93); // LCD+BG+OBJ
    } else {
        ppu.write_register(0xFF40, 0x91); // LCD+BG
    }
    ppu.write_register(0xFF43, scx);
    let (mut o, mut d) = (0u32, 0u32);
    for dot in 1..=456u32 {
        let prev = ppu.get_mode();
        ppu.tick(1);
        if prev == PpuMode::OamScan && ppu.get_mode() == PpuMode::Drawing {
            o = dot;
        }
        if prev == PpuMode::Drawing && ppu.get_mode() == PpuMode::HBlank {
            d = dot;
        }
    }
    d - o
}

fn main() {
    for scx in [0u8, 1, 2, 3, 4, 5, 7] {
        println!("SCX={} : mode3 = {} dots (hw {})", scx, mode3_len(scx, false), 172 + (scx & 7));
    }
    println!("1 sprite @x0 : mode3 = {} dots (hw ~180)", mode3_len(0, true));
}
