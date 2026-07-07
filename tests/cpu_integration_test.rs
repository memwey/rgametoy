extern crate rgametoy;
use rgametoy::console::Console;

/// End-to-end: execute a small program with a backward loop and verify the
/// computed result. This exercises fetch/decode/execute, immediate loads,
/// the ALU, DEC flag behaviour and a taken/not-taken conditional jump.
#[test]
fn test_sum_loop_program() {
    // A = 0; B = 10; loop: A += B; B -= 1; JR NZ loop  => A = 55 (0x37)
    let program = [
        0x3E, 0x00, // 0x0000: LD A, 0
        0x06, 0x0A, // 0x0002: LD B, 10
        0x80, //       0x0004: ADD A, B      (loop target)
        0x05, //       0x0005: DEC B
        0x20, 0xFC, // 0x0006: JR NZ, -4     (back to 0x0004)
        0x76, //       0x0008: HALT
    ];
    let mut console = Console::new();
    console.load_program(&program);

    // Run until the CPU halts (with a generous step cap as a safety net).
    for _ in 0..10_000 {
        console.step();
        if console.get_cpu().is_halted() {
            break;
        }
    }

    assert!(console.get_cpu().is_halted());
    assert_eq!(console.get_cpu().get_registers().get_a(), 55);
    assert_eq!(console.get_cpu().get_registers().get_b(), 0);
    assert!(console.get_cpu().get_registers().get_flag_z());
}

#[test]
fn test_add_hl_bc_instruction() {
    // Test case 1: No carry, no half-carry
    let mut console = Console::new();
    console.load_program(&[0x09]); // ADD HL, BC
    console.get_cpu_mut().set_hl(0x1000);
    console.get_cpu_mut().set_bc(0x0001);
    console.step();
    assert_eq!(console.get_cpu().get_registers().get_hl(), 0x1001);
    assert_eq!(console.get_cpu().get_registers().get_flag_n(), false);
    assert_eq!(console.get_cpu().get_registers().get_flag_h(), false);
    assert_eq!(console.get_cpu().get_registers().get_flag_c(), false);

    // Test case 2: Half-carry
    let mut console = Console::new();
    console.load_program(&[0x09]); // ADD HL, BC
    console.get_cpu_mut().set_hl(0x0F00);
    console.get_cpu_mut().set_bc(0x0100);
    console.step();
    assert_eq!(console.get_cpu().get_registers().get_hl(), 0x1000);
    assert_eq!(console.get_cpu().get_registers().get_flag_n(), false);
    assert_eq!(console.get_cpu().get_registers().get_flag_h(), true);
    assert_eq!(console.get_cpu().get_registers().get_flag_c(), false);

    // Test case 3: Full carry
    let mut console = Console::new();
    console.load_program(&[0x09]); // ADD HL, BC
    console.get_cpu_mut().set_hl(0xF000);
    console.get_cpu_mut().set_bc(0x2000);
    console.step();
    assert_eq!(console.get_cpu().get_registers().get_hl(), 0x1000); // 0xF000 + 0x2000 = 0x11000, so 0x1000 with carry
    assert_eq!(console.get_cpu().get_registers().get_flag_n(), false);
    assert_eq!(console.get_cpu().get_registers().get_flag_h(), false);
    assert_eq!(console.get_cpu().get_registers().get_flag_c(), true);
}
