//! Headless throughput benchmark: run a ROM as fast as possible for N frames and
//! report how many times real-time the emulator achieves. Dependency-free (no
//! criterion), so it answers "how fast do we run?" without a dev-dependency.
//!
//! Run with: `cargo run --release --example benchmark -- rom.gb [frames]`
//! (use `--release` — a debug build is many times slower and not meaningful).

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::Console;
use std::time::Instant;

/// DMG frame rate: 4.194304 MHz / 70224 dots per frame ≈ 59.7275 Hz.
const DMG_FPS: f64 = 4_194_304.0 / 70224.0;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: benchmark <rom.gb> [frames]");
        std::process::exit(2);
    }
    let frames: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10_000);
    let data = std::fs::read(&args[1]).expect("read ROM");

    let mut console = Console::new();
    console.load_cartridge(Cartridge::from_bytes(data));

    let start = Instant::now();
    for _ in 0..frames {
        console.run_frame();
        // Drain the outputs each frame, exactly like the real loop, so the
        // buffers stay bounded and the audio work isn't skipped.
        let _ = console.take_audio_samples();
        let _ = console.take_serial_output();
    }
    let elapsed = start.elapsed().as_secs_f64();

    // Sanity checksum so the frame work can't be optimised away.
    let checksum: u64 = console.framebuffer().iter().map(|&p| p as u64).sum();

    let fps = frames as f64 / elapsed;
    println!("{frames} frames in {elapsed:.3}s  (framebuffer checksum {checksum})");
    println!(
        "  {fps:.0} fps  =  {:.1}x realtime  (DMG = {DMG_FPS:.2} Hz)",
        fps / DMG_FPS
    );
    println!("  {:.1} µs/frame", elapsed * 1e6 / frames as f64);
}
