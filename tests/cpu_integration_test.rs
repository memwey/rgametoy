extern crate rgametoy;
use rgametoy::cpu::Cpu;

#[test]
fn test_add_hl_bc_instruction() {
    // Test case 1: No carry, no half-carry
    let mut cpu = Cpu::new();
    cpu.load_program(&[0x09]); // ADD HL, BC
    cpu.set_hl(0x1000);
    cpu.set_bc(0x0001);
    cpu.step();
    assert_eq!(cpu.get_registers().get_hl(), 0x1001);
    assert_eq!(cpu.get_registers().get_flag_n(), false);
    assert_eq!(cpu.get_registers().get_flag_h(), false);
    assert_eq!(cpu.get_registers().get_flag_c(), false);

    // Test case 2: Half-carry
    let mut cpu = Cpu::new();
    cpu.load_program(&[0x09]); // ADD HL, BC
    cpu.set_hl(0x0F00);
    cpu.set_bc(0x0100);
    cpu.step();
    assert_eq!(cpu.get_registers().get_hl(), 0x1000);
    assert_eq!(cpu.get_registers().get_flag_n(), false);
    assert_eq!(cpu.get_registers().get_flag_h(), true);
    assert_eq!(cpu.get_registers().get_flag_c(), false);

    // Test case 3: Full carry
    let mut cpu = Cpu::new();
    cpu.load_program(&[0x09]); // ADD HL, BC
    cpu.set_hl(0xF000);
    cpu.set_bc(0x2000);
    cpu.step();
    assert_eq!(cpu.get_registers().get_hl(), 0x1000); // 0xF000 + 0x2000 = 0x11000, so 0x1000 with carry
    assert_eq!(cpu.get_registers().get_flag_n(), false);
    assert_eq!(cpu.get_registers().get_flag_h(), false);
    assert_eq!(cpu.get_registers().get_flag_c(), true);
}
