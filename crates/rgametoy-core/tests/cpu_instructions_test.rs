extern crate rgametoy_core;

use rgametoy_core::bus::Bus;
use rgametoy_core::Console;

/// Build a console with `program` loaded at address 0x0000 and PC/SP at a
/// sensible starting point for instruction-level tests.
fn setup(program: &[u8]) -> Console {
    let mut console = Console::new();
    console.load_program(program);
    console.get_cpu_mut().set_sp(0xD000); // stack lives in WRAM
    console
}

// --------------------------------------------------------------------------
// 8-bit loads
// --------------------------------------------------------------------------

#[test]
fn test_ld_r_d8_and_ld_r_r() {
    // LD B, 0x42 ; LD C, B
    let mut c = setup(&[0x06, 0x42, 0x48]);
    c.step();
    assert_eq!(c.get_cpu().get_registers().get_b(), 0x42);
    c.step();
    assert_eq!(c.get_cpu().get_registers().get_c(), 0x42);
}

#[test]
fn test_ld_hl_mem_roundtrip() {
    // LD (HL), A with HL pointing at WRAM, then LD A back from a different reg.
    let mut c = setup(&[0x77]); // LD (HL), A
    c.get_cpu_mut().set_hl(0xC000);
    c.get_cpu_mut().get_registers_mut().set_a(0x99);
    c.step();
    assert_eq!(c.get_bus_mut().read_byte(0xC000), 0x99);
}

#[test]
fn test_ldi_ldd() {
    // LD (HL+), A ; LD (HL-), A
    let mut c = setup(&[0x22, 0x32]);
    c.get_cpu_mut().set_hl(0xC000);
    c.get_cpu_mut().get_registers_mut().set_a(0x11);
    c.step();
    assert_eq!(c.get_bus_mut().read_byte(0xC000), 0x11);
    assert_eq!(c.get_cpu().get_registers().get_hl(), 0xC001);
    c.step();
    assert_eq!(c.get_bus_mut().read_byte(0xC001), 0x11);
    assert_eq!(c.get_cpu().get_registers().get_hl(), 0xC000);
}

#[test]
fn test_ldh_and_ld_c_indirect() {
    // LDH (0x80), A ; LD A, (0xFF80) via LDH A,(a8)
    let mut c = setup(&[0xE0, 0x80, 0xF0, 0x80]);
    c.get_cpu_mut().get_registers_mut().set_a(0x5A);
    c.step(); // write A -> 0xFF80 (HRAM)
    assert_eq!(c.get_bus_mut().read_byte(0xFF80), 0x5A);
    c.get_cpu_mut().get_registers_mut().set_a(0x00);
    c.step(); // read 0xFF80 back into A
    assert_eq!(c.get_cpu().get_registers().get_a(), 0x5A);
}

// --------------------------------------------------------------------------
// 8-bit arithmetic & flags
// --------------------------------------------------------------------------

#[test]
fn test_add_a_flags() {
    // ADD A, B with A=0x0F, B=0x01 -> 0x10, half-carry set.
    let mut c = setup(&[0x80]);
    c.get_cpu_mut().get_registers_mut().set_a(0x0F);
    c.get_cpu_mut().get_registers_mut().set_b(0x01);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0x10);
    assert!(!r.get_flag_z());
    assert!(!r.get_flag_n());
    assert!(r.get_flag_h());
    assert!(!r.get_flag_c());
}

#[test]
fn test_add_a_carry_and_zero() {
    // ADD A, B with A=0xFF, B=0x01 -> 0x00, carry + half-carry + zero.
    let mut c = setup(&[0x80]);
    c.get_cpu_mut().get_registers_mut().set_a(0xFF);
    c.get_cpu_mut().get_registers_mut().set_b(0x01);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0x00);
    assert!(r.get_flag_z());
    assert!(r.get_flag_h());
    assert!(r.get_flag_c());
}

#[test]
fn test_adc_uses_carry_in() {
    // ADC A, d8: A=0x10, carry set, +0x0F -> 0x20.
    let mut c = setup(&[0xCE, 0x0F]);
    c.get_cpu_mut().get_registers_mut().set_a(0x10);
    c.get_cpu_mut().get_registers_mut().set_flag_c(true);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0x20);
    assert!(r.get_flag_h()); // 0x0 + 0xF + 1 = 0x10
}

#[test]
fn test_sub_and_cp_flags() {
    // SUB B: A=0x10, B=0x01 -> 0x0F, half-borrow set, no zero/carry.
    let mut c = setup(&[0x90]);
    c.get_cpu_mut().get_registers_mut().set_a(0x10);
    c.get_cpu_mut().get_registers_mut().set_b(0x01);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0x0F);
    assert!(r.get_flag_n());
    assert!(r.get_flag_h());
    assert!(!r.get_flag_c());

    // CP d8: A == operand -> zero set, A unchanged.
    let mut c = setup(&[0xFE, 0x0F]);
    c.get_cpu_mut().get_registers_mut().set_a(0x0F);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0x0F);
    assert!(r.get_flag_z());
    assert!(r.get_flag_n());
}

#[test]
fn test_logic_ops_flags() {
    // AND sets H, clears C. OR/XOR clear H and C.
    let mut c = setup(&[0xE6, 0x0F]); // AND 0x0F
    c.get_cpu_mut().get_registers_mut().set_a(0xF0);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0x00);
    assert!(r.get_flag_z());
    assert!(r.get_flag_h());
    assert!(!r.get_flag_c());

    let mut c = setup(&[0xEE, 0xFF]); // XOR 0xFF
    c.get_cpu_mut().get_registers_mut().set_a(0x0F);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0xF0);
    assert!(!r.get_flag_h());
}

#[test]
fn test_inc_dec_r8_flags() {
    // INC B: 0x0F -> 0x10, half-carry. Carry flag must be preserved.
    let mut c = setup(&[0x04]);
    c.get_cpu_mut().get_registers_mut().set_b(0x0F);
    c.get_cpu_mut().get_registers_mut().set_flag_c(true);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_b(), 0x10);
    assert!(r.get_flag_h());
    assert!(!r.get_flag_n());
    assert!(r.get_flag_c()); // preserved

    // DEC B: 0x00 -> 0xFF, half-borrow + subtract flag.
    let mut c = setup(&[0x05]);
    c.get_cpu_mut().get_registers_mut().set_b(0x00);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_b(), 0xFF);
    assert!(r.get_flag_n());
    assert!(r.get_flag_h());
}

#[test]
fn test_inc_hl_indirect() {
    // INC (HL) operates on memory.
    let mut c = setup(&[0x34]);
    c.get_cpu_mut().set_hl(0xC000);
    c.get_bus_mut().write_byte(0xC000, 0xFF);
    c.step();
    assert_eq!(c.get_bus_mut().read_byte(0xC000), 0x00);
    assert!(c.get_cpu().get_registers().get_flag_z());
}

// --------------------------------------------------------------------------
// 16-bit arithmetic
// --------------------------------------------------------------------------

#[test]
fn test_add_sp_e8_flags() {
    // ADD SP, -1 : flags come from the low byte arithmetic.
    let mut c = setup(&[0xE8, 0xFF]); // e8 = -1
    c.get_cpu_mut().set_sp(0xD000);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(c.get_cpu().get_sp(), 0xCFFF);
    assert!(!r.get_flag_z());
    assert!(!r.get_flag_n());
}

#[test]
fn test_ld_hl_sp_e8() {
    // LD HL, SP+2
    let mut c = setup(&[0xF8, 0x02]);
    c.get_cpu_mut().set_sp(0xD000);
    c.step();
    assert_eq!(c.get_cpu().get_registers().get_hl(), 0xD002);
    // SP itself is unchanged.
    assert_eq!(c.get_cpu().get_sp(), 0xD000);
}

// --------------------------------------------------------------------------
// Rotates / CB-prefixed ops
// --------------------------------------------------------------------------

#[test]
fn test_rlca_clears_zero() {
    // RLCA of 0x80 -> 0x01, carry set, and Z is always cleared.
    let mut c = setup(&[0x07]);
    c.get_cpu_mut().get_registers_mut().set_a(0x80);
    c.get_cpu_mut().get_registers_mut().set_flag_z(true);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0x01);
    assert!(r.get_flag_c());
    assert!(!r.get_flag_z());
}

#[test]
fn test_cb_rl_sets_zero_from_result() {
    // CB RL B: 0x80 with no carry-in -> 0x00, carry out, Z set (unlike RLCA).
    let mut c = setup(&[0xCB, 0x10]); // RL B
    c.get_cpu_mut().get_registers_mut().set_b(0x80);
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_b(), 0x00);
    assert!(r.get_flag_c());
    assert!(r.get_flag_z());
}

#[test]
fn test_cb_swap() {
    // CB SWAP A: 0xAB -> 0xBA.
    let mut c = setup(&[0xCB, 0x37]); // SWAP A
    c.get_cpu_mut().get_registers_mut().set_a(0xAB);
    c.step();
    assert_eq!(c.get_cpu().get_registers().get_a(), 0xBA);
}

#[test]
fn test_cb_bit_set_res() {
    // BIT 7, A on 0x00 -> Z set; SET 7, A -> 0x80; RES 7, A -> 0x00.
    let mut c = setup(&[0xCB, 0x7F, 0xCB, 0xFF, 0xCB, 0xBF]);
    c.get_cpu_mut().get_registers_mut().set_a(0x00);
    c.step(); // BIT 7, A
    assert!(c.get_cpu().get_registers().get_flag_z());
    assert!(c.get_cpu().get_registers().get_flag_h());
    c.step(); // SET 7, A
    assert_eq!(c.get_cpu().get_registers().get_a(), 0x80);
    c.step(); // RES 7, A
    assert_eq!(c.get_cpu().get_registers().get_a(), 0x00);
}

// --------------------------------------------------------------------------
// Control flow
// --------------------------------------------------------------------------

#[test]
fn test_jr_taken_and_negative() {
    // JR -2 (0xFE) with the branch not taken vs taken.
    // JR NZ, +2 : Z clear -> taken, skips over the next 2 bytes.
    let mut c = setup(&[0x20, 0x02, 0x00, 0x00]);
    c.get_cpu_mut().get_registers_mut().set_flag_z(false);
    let cycles = c.step();
    assert_eq!(cycles, 12); // taken
    assert_eq!(c.get_cpu().get_pc(), 0x0004);

    // Not taken: Z set.
    let mut c = setup(&[0x20, 0x02]);
    c.get_cpu_mut().get_registers_mut().set_flag_z(true);
    let cycles = c.step();
    assert_eq!(cycles, 8); // not taken
    assert_eq!(c.get_cpu().get_pc(), 0x0002);
}

#[test]
fn test_jp_and_jp_hl() {
    // JP 0x1234
    let mut c = setup(&[0xC3, 0x34, 0x12]);
    c.step();
    assert_eq!(c.get_cpu().get_pc(), 0x1234);

    // JP (HL)
    let mut c = setup(&[0xE9]);
    c.get_cpu_mut().set_hl(0x0200);
    c.step();
    assert_eq!(c.get_cpu().get_pc(), 0x0200);
}

#[test]
fn test_call_ret() {
    // 0x0000: CALL 0x0006   (return address 0x0003)
    // 0x0006: RET
    let mut c = setup(&[0xCD, 0x06, 0x00, 0x00, 0x00, 0x00, 0xC9]);
    c.step(); // CALL
    assert_eq!(c.get_cpu().get_pc(), 0x0006);
    assert_eq!(c.get_cpu().get_sp(), 0xCFFE);
    assert_eq!(c.get_bus_mut().read_byte(0xCFFE), 0x03); // low byte of 0x0003
    assert_eq!(c.get_bus_mut().read_byte(0xCFFF), 0x00);

    c.step(); // RET
    assert_eq!(c.get_cpu().get_pc(), 0x0003);
    assert_eq!(c.get_cpu().get_sp(), 0xD000);
}

#[test]
fn test_rst() {
    // RST 0x38 pushes PC and jumps to 0x0038.
    let mut c = setup(&[0xFF]);
    c.step();
    assert_eq!(c.get_cpu().get_pc(), 0x0038);
    assert_eq!(c.get_cpu().get_sp(), 0xCFFE);
}

#[test]
fn test_push_pop_af_masks_low_nibble() {
    // POP AF must clear the low nibble of F.
    let mut c = setup(&[0xF1]); // POP AF
    c.get_cpu_mut().set_sp(0xC000);
    c.get_bus_mut().write_byte(0xC000, 0xFF); // F
    c.get_bus_mut().write_byte(0xC001, 0x42); // A
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0x42);
    assert_eq!(r.get_f(), 0xF0); // low nibble cleared
    assert_eq!(r.get_af(), 0x42F0);
}

// --------------------------------------------------------------------------
// DAA
// --------------------------------------------------------------------------

#[test]
fn test_daa_after_addition() {
    // 0x45 + 0x38 = 0x7D, DAA -> 0x83 (BCD 45 + 38 = 83).
    let mut c = setup(&[0x80, 0x27]); // ADD A, B ; DAA
    c.get_cpu_mut().get_registers_mut().set_a(0x45);
    c.get_cpu_mut().get_registers_mut().set_b(0x38);
    c.step();
    c.step();
    let r = c.get_cpu().get_registers();
    assert_eq!(r.get_a(), 0x83);
    assert!(!r.get_flag_z());
    assert!(!r.get_flag_c());
}

// --------------------------------------------------------------------------
// Interrupts / EI / HALT
// --------------------------------------------------------------------------

#[test]
fn test_interrupt_dispatch_priority() {
    // With LCDStat (bit1) and Timer (bit2) both pending, LCDStat wins.
    let mut c = setup(&[0x00]);
    c.get_cpu_mut().enable_interrupts();
    c.get_bus_mut().write_byte(0xFFFF, 0xFF); // IE: all enabled
    c.get_bus_mut().write_byte(0xFF0F, 0x06); // IF: LCDStat + Timer
    let cycles = c.step();
    assert_eq!(cycles, 20);
    assert_eq!(c.get_cpu().get_pc(), 0x0048); // LCDStat handler
    assert!(!c.get_cpu().ime_enabled()); // IME cleared on dispatch
    assert_eq!(c.get_bus_mut().read_byte(0xFF0F) & 0x1F, 0x04); // LCDStat bit cleared
}

#[test]
fn test_ei_has_one_instruction_delay() {
    // EI ; NOP ; NOP  — the interrupt may only fire after the instruction
    // following EI has executed.
    let mut c = setup(&[0xFB, 0x00, 0x00]);
    c.get_bus_mut().write_byte(0xFFFF, 0x01); // IE: VBlank
    c.get_bus_mut().write_byte(0xFF0F, 0x01); // IF: VBlank

    c.step(); // EI
    assert!(!c.get_cpu().ime_enabled());
    c.step(); // NOP (IME still off during this instruction)
    assert!(!c.get_cpu().ime_enabled() || c.get_cpu().get_pc() == 0x0002);
    let pc_after_nop = c.get_cpu().get_pc();
    assert_eq!(pc_after_nop, 0x0002);

    c.step(); // interrupt is now serviced
    assert_eq!(c.get_cpu().get_pc(), 0x0040);
}

#[test]
fn test_halt_wakes_on_interrupt() {
    // HALT with IME on: the CPU idles until an interrupt is requested.
    let mut c = setup(&[0x76]);
    c.get_cpu_mut().enable_interrupts();
    c.get_bus_mut().write_byte(0xFFFF, 0x01); // IE: VBlank

    c.step(); // HALT
    assert!(c.get_cpu().is_halted());
    c.step(); // still halted, nothing pending
    assert!(c.get_cpu().is_halted());

    c.get_bus_mut().write_byte(0xFF0F, 0x01); // request VBlank
    c.step();
    assert!(!c.get_cpu().is_halted());
    assert_eq!(c.get_cpu().get_pc(), 0x0040);
}

#[test]
fn test_halt_bug_executes_next_byte_twice() {
    // HALT with IME off and an interrupt pending triggers the halt bug:
    // the byte after HALT is executed twice.
    let mut c = setup(&[0x76, 0x3C]); // HALT ; INC A
    c.get_cpu_mut().disable_interrupts();
    c.get_bus_mut().write_byte(0xFFFF, 0x01);
    c.get_bus_mut().write_byte(0xFF0F, 0x01);
    c.get_cpu_mut().get_registers_mut().set_a(0x00);

    c.step(); // HALT (does not actually halt; arms the bug)
    assert!(!c.get_cpu().is_halted());
    c.step(); // INC A, but PC does not advance past it
    assert_eq!(c.get_cpu().get_registers().get_a(), 0x01);
    assert_eq!(c.get_cpu().get_pc(), 0x0001);
    c.step(); // INC A again
    assert_eq!(c.get_cpu().get_registers().get_a(), 0x02);
    assert_eq!(c.get_cpu().get_pc(), 0x0002);
}
