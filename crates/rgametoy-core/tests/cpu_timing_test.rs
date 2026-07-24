//! Golden CPU timing reference: pins the exact T-cycle count of every opcode
//! (plus both paths of every conditional) against a table captured from the
//! current known-good implementation.
//!
//! This is the local stand-in for Blargg `mem_timing` — which needs the
//! `GB_TEST_ROMS` bundle and so can't run in CI or on a machine without it. The
//! cycle counts are the observable essence of instruction timing; any refactor
//! that shifts them (e.g. the micro-op rewrite toward the crystal-driven model)
//! trips this net immediately. Access *ordering within* an instruction is
//! pinned separately by the PPU/timer unit tests and `cpu_integration_test`.

extern crate rgametoy_core;

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::Console;

/// Post-boot console with `program` at 0x100, SP in WRAM, HL pointing at WRAM,
/// and A=0 / F=`f`. Operands in `program` supply n=0x34 / nn=0x1234.
fn console(program: &[u8], f: u8) -> Console {
    let mut c = Console::new();
    c.power_on(Cartridge::from_bytes({
        let mut rom = vec![0u8; 0x200];
        rom[0x100..0x100 + program.len()].copy_from_slice(program);
        rom
    }));
    c.cpu_mut().set_sp(0xD000);
    c.cpu_mut().set_hl(0xC000);
    c.cpu_mut().set_af(f as u16); // A = 0, F = f
    c
}

/// T-cycles consumed executing base opcode `op` with F = `f`.
fn cycles_base(op: u8, f: u8) -> u8 {
    console(&[op, 0x34, 0x12], f).step()
}

/// T-cycles consumed executing CB-prefixed opcode `op`.
fn cycles_cb(op: u8) -> u8 {
    console(&[0xCB, op], 0).step()
}

// Captured from the current implementation with F = 0x00 (so NZ / NC conditions
// are taken, Z / C are not; the conditional paths are pinned separately below).
const BASE_CYCLES: [u8; 256] = [
    4, 12, 8, 8, 4, 4, 8, 4, 20, 8, 8, 8, 4, 4, 8, 4, //
    8, 12, 8, 8, 4, 4, 8, 4, 12, 8, 8, 8, 4, 4, 8, 4, //
    12, 12, 8, 8, 4, 4, 8, 4, 8, 8, 8, 8, 4, 4, 8, 4, //
    12, 12, 8, 8, 12, 12, 12, 4, 8, 8, 8, 8, 4, 4, 8, 4, //
    4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4, //
    4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4, //
    4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4, //
    8, 8, 8, 8, 8, 8, 4, 8, 4, 4, 4, 4, 4, 4, 8, 4, //
    4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4, //
    4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4, //
    4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4, //
    4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 8, 4, //
    20, 12, 16, 16, 24, 16, 8, 16, 8, 16, 12, 8, 12, 24, 8, 16, //
    20, 12, 16, 4, 24, 16, 8, 16, 8, 16, 12, 4, 12, 4, 8, 16, //
    12, 12, 8, 4, 4, 16, 8, 16, 16, 4, 16, 4, 4, 4, 8, 16, //
    12, 12, 8, 4, 4, 16, 8, 16, 12, 8, 16, 4, 4, 4, 8, 16, //
];

const CB_CYCLES: [u8; 256] = [
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 12, 8, //
    8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 12, 8, //
    8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 12, 8, //
    8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 12, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
    8, 8, 8, 8, 8, 8, 16, 8, 8, 8, 8, 8, 8, 8, 16, 8, //
];

#[test]
fn base_opcode_cycle_counts_match_golden() {
    for op in 0..=255u8 {
        let got = cycles_base(op, 0x00);
        assert_eq!(
            got, BASE_CYCLES[op as usize],
            "base opcode {op:#04x}: expected {} T-cycles, got {got}",
            BASE_CYCLES[op as usize]
        );
    }
}

#[test]
fn cb_opcode_cycle_counts_match_golden() {
    for op in 0..=255u8 {
        let got = cycles_cb(op);
        assert_eq!(
            got, CB_CYCLES[op as usize],
            "CB opcode {op:#04x}: expected {} T-cycles, got {got}",
            CB_CYCLES[op as usize]
        );
    }
}

// --- Conditional instructions: pin BOTH paths (taken adds cycles) -----------
// Z flag = 0x80 (bit 7), C flag = 0x10 (bit 4). NZ/NC conditions are true with
// F=0x00; Z/C are true with F=0x80 / F=0x10 respectively.

/// `(opcode, F making the condition true, F making it false)`.
const CONDITIONALS: &[(u8, u8, u8)] = &[
    (0x20, 0x00, 0x80), // JR NZ
    (0x28, 0x80, 0x00), // JR Z
    (0x30, 0x00, 0x10), // JR NC
    (0x38, 0x10, 0x00), // JR C
    (0xC0, 0x00, 0x80), // RET NZ
    (0xC8, 0x80, 0x00), // RET Z
    (0xD0, 0x00, 0x10), // RET NC
    (0xD8, 0x10, 0x00), // RET C
    (0xC2, 0x00, 0x80), // JP NZ
    (0xCA, 0x80, 0x00), // JP Z
    (0xD2, 0x00, 0x10), // JP NC
    (0xDA, 0x10, 0x00), // JP C
    (0xC4, 0x00, 0x80), // CALL NZ
    (0xCC, 0x80, 0x00), // CALL Z
    (0xD4, 0x00, 0x10), // CALL NC
    (0xDC, 0x10, 0x00), // CALL C
];

/// Canonical DMG taken / not-taken cycle counts per conditional group.
fn conditional_cycles(op: u8) -> (u8, u8) {
    match op {
        0x20 | 0x28 | 0x30 | 0x38 => (12, 8),  // JR cc
        0xC0 | 0xC8 | 0xD0 | 0xD8 => (20, 8),  // RET cc
        0xC2 | 0xCA | 0xD2 | 0xDA => (16, 12), // JP cc
        0xC4 | 0xCC | 0xD4 | 0xDC => (24, 12), // CALL cc
        _ => unreachable!("not a conditional opcode: {op:#04x}"),
    }
}

#[test]
fn conditional_opcodes_have_correct_taken_and_not_taken_cycles() {
    for &(op, f_true, f_false) in CONDITIONALS {
        let (taken, not_taken) = conditional_cycles(op);
        assert_eq!(
            cycles_base(op, f_true),
            taken,
            "opcode {op:#04x} taken: expected {taken}"
        );
        assert_eq!(
            cycles_base(op, f_false),
            not_taken,
            "opcode {op:#04x} not-taken: expected {not_taken}"
        );
    }
}
