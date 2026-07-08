//! Headless mooneye-test-suite runner. A mooneye test signals success by
//! loading the Fibonacci sequence 3,5,8,13,21,34 into B,C,D,E,H,L and then
//! spinning; any other final register state is a failure. Prints PASS / FAIL.
//!
//! Run with: `cargo run --example run_mooneye -- test.gb`

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::Console;

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: run_mooneye <rom.gb>");
            std::process::exit(2);
        }
    };
    let data = std::fs::read(&path).expect("read ROM");
    let mut console = Console::new();
    console.load_cartridge(Cartridge::from_bytes(data));

    // mooneye tests finish quickly; run a generous number of frames.
    for _ in 0..240 {
        console.run_frame();
    }

    let r = console.get_cpu().get_registers();
    let pass = r.get_b() == 3
        && r.get_c() == 5
        && r.get_d() == 8
        && r.get_e() == 13
        && r.get_h() == 21
        && r.get_l() == 34;
    println!("{}", if pass { "PASS" } else { "FAIL" });
    std::process::exit(if pass { 0 } else { 1 });
}
