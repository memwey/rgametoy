//! Run a ROM headless for N frames and dump the 160x144 framebuffer (shade
//! values 0-3, one byte per pixel) to a raw file. For comparing PPU test ROMs
//! against reference images.  Usage: dump_fb <rom.gb> <out.raw> [frames]

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::Console;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let frames: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60);
    let data = std::fs::read(&args[1]).expect("read ROM");
    let mut console = Console::new();
    console.power_on(Cartridge::from_bytes(data));
    for _ in 0..frames {
        console.run_frame();
    }
    std::fs::File::create(&args[2])
        .expect("create")
        .write_all(console.framebuffer())
        .expect("write");
}
