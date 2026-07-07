use rgametoy::emulator::Emulator;
use std::process::ExitCode;

fn main() -> ExitCode {
    let rom_path = match std::env::args().nth(1) {
        Some(path) => path,
        None => {
            eprintln!("usage: rgametoy <rom.gb>");
            return ExitCode::FAILURE;
        }
    };

    let mut emulator = Emulator::new();
    if let Err(e) = emulator.load_rom(&rom_path) {
        eprintln!("failed to load ROM '{rom_path}': {e}");
        return ExitCode::FAILURE;
    }

    emulator.run();
    ExitCode::SUCCESS
}
