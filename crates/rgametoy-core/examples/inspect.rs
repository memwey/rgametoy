//! Reusable machine inspector for calibrating against hardware test ROMs.
//! Build with the `debug` feature:
//!
//! ```sh
//! cargo run --release --features debug --example inspect -- <rom> <cmd> [args]
//! ```
//!
//! Commands:
//!   regs   <rom> [frames]            run N frames (default 240), print a snapshot
//!   break  <rom> <PChex>             run until PC == <PC>, print a snapshot
//!   watch  <rom> <PChex> [count]     print a snapshot each time PC == <PC> (default 20)
//!   line   <rom> <LY>                print PPU mode/dot changes while on scanline LY
//!   dumpat <rom> <PChex> <addr> <len> [count]  at each PC hit, hex-dump a memory range
//!   oamat  <rom> <PChex> [LY]        at the PC hit (optionally when LY matches), dump OAM
//!   oam    <rom>                     run until sprites are on-screen, list them

#[cfg(not(feature = "debug"))]
fn main() {
    eprintln!("inspect requires the `debug` feature: cargo run --features debug --example inspect -- ...");
    std::process::exit(2);
}

#[cfg(feature = "debug")]
fn main() {
    use rgametoy_core::cartridge::Cartridge;
    use rgametoy_core::Console;

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: inspect <rom> <regs|break|watch|line|dumpat|oamat|oam> [args]");
        std::process::exit(2);
    }
    let rom = &args[1];
    let cmd = args[2].as_str();
    let load = || {
        let mut c = Console::new();
        c.load_cartridge(Cartridge::from_bytes(std::fs::read(rom).expect("read ROM")));
        c
    };
    let hex = |s: &str| u16::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex");

    match cmd {
        "regs" => {
            let frames: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(240);
            let mut c = load();
            for _ in 0..frames {
                c.run_frame();
            }
            println!("{}", c.snapshot());
        }
        "break" => {
            let pc = hex(&args[3]);
            let mut c = load();
            if c.run_until(20_000_000, |c| c.get_cpu().get_pc() == pc) {
                println!("{}", c.snapshot());
            } else {
                eprintln!("PC {:04X} not reached", pc);
            }
        }
        "watch" => {
            let pc = hex(&args[3]);
            let count: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(20);
            let mut c = load();
            for _ in 0..count {
                if !c.run_until(20_000_000, |c| c.get_cpu().get_pc() == pc) {
                    break;
                }
                println!("{}", c.snapshot());
                c.step(); // move off the breakpoint
            }
        }
        "line" => {
            let ly: u8 = args[3].parse().expect("LY");
            let mut c = load();
            c.run_until(20_000_000, |c| c.peek(0xFF44) == ly);
            let (mut last_mode, mut last_dot) = (0xFFu8, 0xFFFFu16);
            while c.peek(0xFF44) == ly {
                let s = c.snapshot();
                if s.ppu_mode != last_mode || s.dots != last_dot {
                    println!(
                        "  LY={} dot={:>3} mode={} STAT={:02X} lyc_m={}",
                        s.ly, s.dots, s.ppu_mode, s.stat, s.lyc_match as u8
                    );
                    last_mode = s.ppu_mode;
                    last_dot = s.dots;
                }
                c.step();
            }
        }
        "dumpat" => {
            // Break at a PC (repeatedly) and hex-dump a memory range each hit.
            // Usage: dumpat <PChex> <addrhex> <len> [count]
            let pc = hex(&args[3]);
            let addr = hex(&args[4]);
            let len: u16 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(16);
            let count: u32 = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(1);
            let mut c = load();
            for _ in 0..count {
                if !c.run_until(50_000_000, |c| c.get_cpu().get_pc() == pc) {
                    break;
                }
                let bytes: Vec<String> = (0..len)
                    .map(|i| format!("{:02X}", c.peek(addr.wrapping_add(i))))
                    .collect();
                println!("PC={:04X} {:04X}: {}", pc, addr, bytes.join(" "));
                c.step(); // move off the breakpoint
            }
        }
        "oamat" => {
            // Break at a PC (optionally when LY matches), then dump OAM.
            let pc = hex(&args[3]);
            let want_ly: Option<u8> = args.get(4).and_then(|s| s.parse().ok());
            let mut c = load();
            if c.run_until(20_000_000, |c| {
                c.get_cpu().get_pc() == pc && want_ly.is_none_or(|ly| c.peek(0xFF44) == ly)
            }) {
                let s = c.snapshot();
                println!("at PC {:04X}, LY={} SCX={}:", pc, s.ly, c.peek(0xFF43));
                for i in 0..40u16 {
                    let y = c.peek(0xFE00 + i * 4);
                    let x = c.peek(0xFE00 + i * 4 + 1);
                    if y != 0 && y != 255 {
                        println!("  obj{:>2}: Y={:>3} X={:>3}", i, y, x);
                    }
                }
            }
        }
        "oam" => {
            let mut c = load();
            let on_screen = |c: &Console| {
                (0..40).any(|i| {
                    let y = c.peek(0xFE00 + i * 4);
                    (16..160).contains(&y)
                })
            };
            if c.run_until(20_000_000, on_screen) {
                let s = c.snapshot();
                println!("LY={} — on-screen OAM entries (Y,X,tile,attr):", s.ly);
                for i in 0..40u16 {
                    let y = c.peek(0xFE00 + i * 4);
                    if y != 255 {
                        println!(
                            "  obj{:>2}: Y={:>3} X={:>3} tile={:02X} attr={:02X}",
                            i,
                            y,
                            c.peek(0xFE00 + i * 4 + 1),
                            c.peek(0xFE00 + i * 4 + 2),
                            c.peek(0xFE00 + i * 4 + 3)
                        );
                    }
                }
            }
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
}
