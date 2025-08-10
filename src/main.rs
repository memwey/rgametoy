use rgametoy::cpu::Cpu;

fn main() {
    let mut cpu = Cpu::new();
    let program = [0x01, 0x34, 0x12, 0x00]; // LD BC, 0x1234; NOP
    cpu.load_program(&program);

    println!("Initial BC: {:04X}", cpu.get_registers().get_bc());
    println!("Initial PC: {:04X}", cpu.get_registers().pc);

    cpu.step();

    println!("BC after step: {:04X}", cpu.get_registers().get_bc());
    println!("PC after step: {:04X}", cpu.get_registers().pc);
}