extern crate rgametoy;
use rgametoy::console::bus::Bus;
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

/// The `ie_push` quirk: when SP is 0x0000, servicing an interrupt pushes the
/// return address so its high byte lands on IE (0xFFFF). That rewrites which
/// interrupt is enabled, and the vector is chosen from IE *after* the push —
/// so a dispatch that began for one interrupt can jump to another's vector
/// (mooneye `ie_push`).
#[test]
fn test_ie_push_reevaluates_the_interrupt_vector() {
    let mut console = Console::new();
    console.load_program(&[0x00]); // ROM content irrelevant; we dispatch first
    console.get_bus_mut().write_byte(0xFFFF, 0x04); // IE: enable Timer only
    console.get_bus_mut().write_byte(0xFF0F, 0x05); // IF: Timer + VBlank pending

    let cpu = console.get_cpu_mut();
    cpu.set_pc(0x0100); // return-address high byte = 0x01
    cpu.set_sp(0x0000); // so the high-byte push writes IE at 0xFFFF
    cpu.enable_interrupts();

    console.step(); // dispatch begins for the Timer (0x50)...

    // ...but the high-byte push wrote 0x01 to IE, so the re-sampled vector is
    // VBlank (0x40), and it is VBlank's IF bit that gets cleared.
    assert_eq!(
        console.get_cpu().get_pc(),
        0x0040,
        "vector retargeted to VBlank after IE was overwritten"
    );
    assert_eq!(
        console.get_bus_mut().read_byte(0xFF0F) & 0x1F,
        0x04,
        "VBlank's IF bit was cleared; the Timer request remains"
    );
}

/// An illegal opcode hangs the CPU: it stops advancing and no longer services
/// interrupts (on real hardware only a reset recovers).
#[test]
fn test_illegal_opcode_locks_up_the_cpu() {
    let mut console = Console::new();
    console.load_program(&[0x00, 0xD3, 0x3C]); // NOP; illegal 0xD3; INC A (never runs)
    console.step(); // NOP
    console.step(); // 0xD3 → lock
    let pc = console.get_cpu().get_pc();

    // A pending, enabled interrupt must not wake a locked CPU (unlike HALT).
    console.get_bus_mut().write_byte(0xFFFF, 0x01); // IE: VBlank
    console.get_bus_mut().write_byte(0xFF0F, 0x01); // IF: VBlank pending
    console.get_cpu_mut().enable_interrupts();

    for _ in 0..100 {
        console.step();
    }
    assert_eq!(console.get_cpu().get_pc(), pc, "PC frozen after the illegal opcode");
    assert_eq!(console.get_cpu().get_registers().get_a(), 0, "the following INC A never ran");
    assert_ne!(console.get_cpu().get_pc(), 0x0040, "a locked CPU ignores interrupts");
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
    assert!(!console.get_cpu().get_registers().get_flag_n());
    assert!(!console.get_cpu().get_registers().get_flag_h());
    assert!(!console.get_cpu().get_registers().get_flag_c());

    // Test case 2: Half-carry
    let mut console = Console::new();
    console.load_program(&[0x09]); // ADD HL, BC
    console.get_cpu_mut().set_hl(0x0F00);
    console.get_cpu_mut().set_bc(0x0100);
    console.step();
    assert_eq!(console.get_cpu().get_registers().get_hl(), 0x1000);
    assert!(!console.get_cpu().get_registers().get_flag_n());
    assert!(console.get_cpu().get_registers().get_flag_h());
    assert!(!console.get_cpu().get_registers().get_flag_c());

    // Test case 3: Full carry
    let mut console = Console::new();
    console.load_program(&[0x09]); // ADD HL, BC
    console.get_cpu_mut().set_hl(0xF000);
    console.get_cpu_mut().set_bc(0x2000);
    console.step();
    assert_eq!(console.get_cpu().get_registers().get_hl(), 0x1000); // 0xF000 + 0x2000 = 0x11000, so 0x1000 with carry
    assert!(!console.get_cpu().get_registers().get_flag_n());
    assert!(!console.get_cpu().get_registers().get_flag_h());
    assert!(console.get_cpu().get_registers().get_flag_c());
}
