//! Run a ROM headless for a number of frames and write the final frame to a
//! 24-bit BMP. Useful for eyeballing PPU test ROMs (e.g. dmg-acid2).
//!
//! Run with: `cargo run --example screenshot -- rom.gb out.bmp [frames]`

use rgametoy::console::cartridge::Cartridge;
use rgametoy::console::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use rgametoy::console::Console;
use std::fs::File;
use std::io::{BufWriter, Write};

const PALETTE: [(u8, u8, u8); 4] = [
    (0xE0, 0xF8, 0xD0),
    (0x88, 0xC0, 0x70),
    (0x34, 0x68, 0x56),
    (0x08, 0x18, 0x20),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: screenshot <rom.gb> <out.bmp> [frames]");
        std::process::exit(1);
    }
    let frames: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);

    let data = std::fs::read(&args[1]).expect("read ROM");
    let mut console = Console::new();
    console.load_cartridge(Cartridge::from_bytes(data));
    for _ in 0..frames {
        console.run_frame();
    }

    write_bmp(&args[2], console.framebuffer()).expect("write BMP");
    println!("wrote {} ({} frames)", args[2], frames);
}

/// Minimal uncompressed 24-bit BMP writer (rows are stored bottom-up).
fn write_bmp(path: &str, framebuffer: &[u8]) -> std::io::Result<()> {
    let (w, h) = (SCREEN_WIDTH, SCREEN_HEIGHT);
    let row_bytes = w * 3; // 480, already 4-byte aligned
    let pixel_data = row_bytes * h;
    let mut out = BufWriter::new(File::create(path)?);

    out.write_all(b"BM")?;
    out.write_all(&((54 + pixel_data) as u32).to_le_bytes())?;
    out.write_all(&0u32.to_le_bytes())?;
    out.write_all(&54u32.to_le_bytes())?;
    out.write_all(&40u32.to_le_bytes())?;
    out.write_all(&(w as i32).to_le_bytes())?;
    out.write_all(&(h as i32).to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?;
    out.write_all(&24u16.to_le_bytes())?;
    out.write_all(&0u32.to_le_bytes())?;
    out.write_all(&(pixel_data as u32).to_le_bytes())?;
    out.write_all(&2835u32.to_le_bytes())?;
    out.write_all(&2835u32.to_le_bytes())?;
    out.write_all(&0u32.to_le_bytes())?;
    out.write_all(&0u32.to_le_bytes())?;

    for y in (0..h).rev() {
        for x in 0..w {
            let (r, g, b) = PALETTE[(framebuffer[y * w + x] & 0x03) as usize];
            out.write_all(&[b, g, r])?;
        }
    }
    out.flush()
}
