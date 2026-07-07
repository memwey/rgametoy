//! Headless rendering demo: draws a background checkerboard plus a sprite
//! directly through the PPU and writes the resulting frame to a 24-bit BMP.
//!
//! Run with: `cargo run --example render_demo -- out.bmp`

use rgametoy::console::ppu::{Ppu, SCREEN_HEIGHT, SCREEN_WIDTH};
use std::fs::File;
use std::io::{BufWriter, Write};

const PALETTE: [(u8, u8, u8); 4] = [
    (0xE0, 0xF8, 0xD0),
    (0x88, 0xC0, 0x70),
    (0x34, 0x68, 0x56),
    (0x08, 0x18, 0x20),
];

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "out.bmp".to_string());
    let mut ppu = Ppu::new();

    // Tile 1: solid colour 3 (dark). Tile 2: solid colour 1 (light-mid).
    for r in 0..8u16 {
        ppu.write_vram(0x8010 + r * 2, 0xFF);
        ppu.write_vram(0x8010 + r * 2 + 1, 0xFF);
        ppu.write_vram(0x8020 + r * 2, 0xFF);
        ppu.write_vram(0x8020 + r * 2 + 1, 0x00);
    }
    // Background tile map: 32×32 checkerboard of tiles 1 and 2.
    for ty in 0..32u16 {
        for tx in 0..32u16 {
            let tile = if (tx + ty) % 2 == 0 { 1 } else { 2 };
            ppu.write_vram(0x9800 + ty * 32 + tx, tile);
        }
    }

    // Sprite tile 3: solid colour 3, placed near the centre.
    for r in 0..8u16 {
        ppu.write_vram(0x8030 + r * 2, 0xFF);
        ppu.write_vram(0x8030 + r * 2 + 1, 0xFF);
    }
    ppu.write_oam(0xFE00, 60 + 16); // Y
    ppu.write_oam(0xFE01, 76 + 8); // X
    ppu.write_oam(0xFE02, 3); // tile
    ppu.write_oam(0xFE03, 0x00); // attributes

    ppu.write_register(0xFF47, 0xE4); // BGP identity
    ppu.write_register(0xFF48, 0xE4); // OBP0 identity
    ppu.write_register(0xFF40, 0x93); // LCD on, OBJ on, BG on, tile data 0x8000

    // Render a full frame.
    for _ in 0..100_000 {
        ppu.tick(200);
        if ppu.take_frame_ready() {
            break;
        }
    }

    write_bmp(&path, ppu.framebuffer()).expect("write BMP");
    println!("wrote {path}");
}

/// Minimal uncompressed 24-bit BMP writer (rows are stored bottom-up).
fn write_bmp(path: &str, framebuffer: &[u8]) -> std::io::Result<()> {
    let w = SCREEN_WIDTH;
    let h = SCREEN_HEIGHT;
    let row_bytes = w * 3; // 160*3 = 480, already 4-byte aligned
    let pixel_data = row_bytes * h;
    let file_size = 54 + pixel_data;

    let mut out = BufWriter::new(File::create(path)?);

    // BITMAPFILEHEADER (14 bytes)
    out.write_all(b"BM")?;
    out.write_all(&(file_size as u32).to_le_bytes())?;
    out.write_all(&0u32.to_le_bytes())?; // reserved
    out.write_all(&54u32.to_le_bytes())?; // pixel data offset

    // BITMAPINFOHEADER (40 bytes)
    out.write_all(&40u32.to_le_bytes())?;
    out.write_all(&(w as i32).to_le_bytes())?;
    out.write_all(&(h as i32).to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // planes
    out.write_all(&24u16.to_le_bytes())?; // bits per pixel
    out.write_all(&0u32.to_le_bytes())?; // BI_RGB, no compression
    out.write_all(&(pixel_data as u32).to_le_bytes())?;
    out.write_all(&2835u32.to_le_bytes())?; // x px/m
    out.write_all(&2835u32.to_le_bytes())?; // y px/m
    out.write_all(&0u32.to_le_bytes())?; // colours in palette
    out.write_all(&0u32.to_le_bytes())?; // important colours

    // Pixel rows, bottom-up, BGR order.
    for y in (0..h).rev() {
        for x in 0..w {
            let shade = framebuffer[y * w + x] & 0x03;
            let (r, g, b) = PALETTE[shade as usize];
            out.write_all(&[b, g, r])?;
        }
    }
    out.flush()
}
