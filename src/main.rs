use rgametoy::emulator::Emulator;

fn main() {
    let mut emulator = Emulator::new();
    let program = [0x01, 0x34, 0x12, 0x00]; // LD BC, 0x1234; NOP
    emulator.get_console_mut().load_program(&program); // Access console through emulator

    println!("Initial BC: {:04X}", emulator.get_console().get_cpu().get_registers().get_bc());
    println!("Initial PC: {:04X}", emulator.get_console().get_cpu().get_registers().pc);

    emulator.run(); // Run the emulator's loop

    println!("BC after step: {:04X}", emulator.get_console().get_cpu().get_registers().get_bc());
    println!("PC after step: {:04X}", emulator.get_console().get_cpu().get_registers().pc);
}