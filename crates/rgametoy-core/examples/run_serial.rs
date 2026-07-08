//! Headless test-ROM runner: loads a ROM, runs it without a window and prints
//! whatever it shifts out over the serial port. Blargg's test ROMs report
//! their results this way, so this is a quick way to check them. Stops when the
//! output contains "Passed"/"Failed" or a frame cap is reached.
//!
//! Run with: `cargo run --example run_serial -- path/to/test.gb`

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::Console;
use std::io::Write;

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: run_serial <rom.gb>");
            std::process::exit(1);
        }
    };
    let data = std::fs::read(&path).expect("read ROM");
    let mut console = Console::new();
    console.power_on(Cartridge::from_bytes(data));

    let mut output = String::new();
    for _ in 0..10_000 {
        console.run_frame();
        let bytes = console.take_serial_output();
        if !bytes.is_empty() {
            let text = String::from_utf8_lossy(&bytes);
            print!("{text}");
            let _ = std::io::stdout().flush();
            output.push_str(&text);
            if output.contains("Passed") || output.contains("Failed") {
                break;
            }
        }
    }
    println!();
}
