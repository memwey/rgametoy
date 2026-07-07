use rgametoy::emulator::Emulator;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = match args.get(1) {
        Some(path) => path.clone(),
        None => {
            eprintln!("usage: rgametoy <rom.gb> [turbo_multiplier]");
            eprintln!("keys: arrows=D-pad  Z=A  X=B  Enter=Start  Backspace=Select");
            eprintln!("      hold Tab=fast-forward  Esc=quit");
            return ExitCode::FAILURE;
        }
    };

    let mut emulator = Emulator::new();
    if let Some(speed) = args.get(2).and_then(|s| s.parse::<f64>().ok()) {
        emulator.set_turbo_speed(speed);
        println!("fast-forward (Tab) speed: {speed}x");
    }

    if let Err(e) = emulator.load_rom(&rom_path) {
        eprintln!("failed to load ROM '{rom_path}': {e}");
        return ExitCode::FAILURE;
    }

    emulator.run();
    ExitCode::SUCCESS
}
