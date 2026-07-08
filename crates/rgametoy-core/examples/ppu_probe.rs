//! Measure mode-3 length on line 2 (a normal line; line 0 after enable is
//! special) for various sprite configurations. Uses the PPU's internal-mode
//! accessor, so it needs the `debug` feature:
//!
//! ```sh
//! cargo run --release --features debug --example ppu_probe
//! ```

#[cfg(not(feature = "debug"))]
fn main() {
    eprintln!("ppu_probe requires the `debug` feature: cargo run --features debug --example ppu_probe");
    std::process::exit(2);
}

#[cfg(feature = "debug")]
fn main() {
    use rgametoy_core::ppu::{Ppu, PpuMode};

    fn mode3(scx: u8, sprite_xs: &[u8]) -> u32 {
        let mut ppu = Ppu::new();
        for (i, &x) in sprite_xs.iter().enumerate() {
            let base = 0xFE00 + i as u16 * 4;
            ppu.write_oam(base, 18); // Y=18 -> screen line 2
            ppu.write_oam(base + 1, x);
            ppu.write_oam(base + 2, 0);
            ppu.write_oam(base + 3, 0);
        }
        let lcdc = if sprite_xs.is_empty() { 0x91 } else { 0x93 };
        ppu.write_register(0xFF40, lcdc);
        ppu.write_register(0xFF43, scx);
        while ppu.read_register(0xFF44) != 2 {
            ppu.tick(1);
        }
        let (mut o, mut d, mut dot) = (0u32, 0u32, 0u32);
        while ppu.read_register(0xFF44) == 2 {
            let p = ppu.get_mode();
            ppu.tick(1);
            dot += 1;
            if p == PpuMode::OamScan && ppu.get_mode() == PpuMode::Drawing {
                o = dot;
            }
            if p == PpuMode::Drawing && ppu.get_mode() == PpuMode::HBlank {
                d = dot;
            }
        }
        d - o
    }

    // Hardware: first sprite at a position pays 11 - min(5, (x+SCX)%8)
    // (X=0 always 11); further sprites at the same position pay 6 each.
    println!("no sprite, SCX=0: mode3={} (hw 172)", mode3(0, &[]));
    for x in [0u8, 8, 9, 12, 15] {
        let hw = 172 + if x == 0 { 11 } else { 11 - (x as u32 % 8).min(5) };
        println!("1 sprite  X={:<3}: mode3={} (hw {})", x, mode3(0, &[x]), hw);
    }
    for n in [2usize, 3, 10] {
        let hw = 172 + 11 + 6 * (n as u32 - 1);
        println!("{} stacked X=0 : mode3={} (hw {})", n, mode3(0, &vec![0u8; n]), hw);
    }
    println!("2 sprites X=0,8: mode3={} (hw {})", mode3(0, &[0, 8]), 172 + 22);
    println!(
        "10 spread X=0..72: mode3={} (hw {})",
        mode3(0, &[0, 8, 16, 24, 32, 40, 48, 56, 64, 72]),
        172 + 110
    );
    println!(
        "10 spread X=4..76: mode3={} (hw {})",
        mode3(0, &[4, 12, 20, 28, 36, 44, 52, 60, 68, 76]),
        172 + 70
    );
}
