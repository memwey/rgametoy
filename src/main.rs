use rgametoy::console::Console;

fn main() {
    let mut console = Console::new();
    let program = [0x01, 0x34, 0x12, 0x00]; // LD BC, 0x1234; NOP
    console.load_program(&program);

    println!("Initial BC: {:04X}", console.get_cpu().get_registers().get_bc());
    println!("Initial PC: {:04X}", console.get_cpu().get_registers().pc);

    console.step();

    println!("BC after step: {:04X}", console.get_cpu().get_registers().get_bc());
    println!("PC after step: {:04X}", console.get_cpu().get_registers().pc);
}