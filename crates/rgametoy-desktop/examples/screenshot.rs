//! Run a ROM headless for a number of frames and write the final frame to a
//! 24-bit BMP. Useful for eyeballing PPU test ROMs (e.g. dmg-acid2).
//!
//! Run with: `cargo run --example screenshot -- rom.gb out.bmp [frames]`
//!
//! The BMP encoding is shared with the interactive emulator's F2 screenshot
//! (`rgametoy_desktop::screenshot`), so both produce identical images.

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::Console;
use rgametoy_desktop::palette;
use rgametoy_desktop::screenshot::encode_bmp;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: screenshot <rom.gb> <out.bmp> [frames]");
        std::process::exit(1);
    }
    let frames: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);

    let data = std::fs::read(&args[1]).expect("read ROM");
    let mut console = Console::new();
    console.power_on(Cartridge::from_bytes(data));
    for _ in 0..frames {
        console.run_frame();
    }

    std::fs::write(&args[2], encode_bmp(console.framebuffer(), palette::DEFAULT)).expect("write BMP");
    println!("wrote {} ({} frames)", args[2], frames);
}
