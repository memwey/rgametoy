//! Decode the `0xCB`-prefixed opcode set (rotates, shifts, swap, BIT/RES/SET)
//! into micro-ops, plus its bit-manipulation primitives.

use super::{Cpu, MicroOp};
use std::collections::VecDeque;

impl Cpu {
    /// Decode a `0xCB`-prefixed opcode into micro-ops. The `0xCB` fetch and the
    /// second-byte fetch are already two M-cycles; this appends only what the
    /// operand needs. A register operand does all its work in the fetch M-cycle
    /// (a zero-cycle micro-op); a `(HL)` operand reads on the next M-cycle and,
    /// unless BIT, writes back on the one after.
    pub(super) fn decode_cb(&self, cb: u8, ops: &mut VecDeque<MicroOp>) {
        let index = cb & 0x07;
        if index == 6 {
            // (HL): read the operand on its own M-cycle.
            ops.push_back(MicroOp::new(|cpu, bus, _| {
                cpu.tmp8 = cpu.read(bus, cpu.registers.get_hl());
            }));
            if (0x40..=0x7F).contains(&cb) {
                // BIT sets flags only, no write-back (rides the read M-cycle's end).
                ops.push_back(MicroOp::zero(move |cpu, _, _| {
                    cpu.cb_bit(cpu.tmp8, (cb >> 3) & 0x07);
                }));
            } else {
                // Rotate/shift/RES/SET: transform and write back on the next M-cycle.
                ops.push_back(MicroOp::new(move |cpu, bus, _| {
                    let (result, _) = cpu.cb_alu(cb, cpu.tmp8);
                    let hl = cpu.registers.get_hl();
                    cpu.write(bus, hl, result);
                }));
            }
        } else {
            // Register operand: read / transform / write are all register-only
            // (read_reg/write_reg only tick for index 6), so they ride the
            // second-byte fetch M-cycle.
            ops.push_back(MicroOp::zero(move |cpu, bus, _| {
                let value = cpu.read_reg(index, bus);
                let (result, is_bit) = cpu.cb_alu(cb, value);
                if !is_bit {
                    cpu.write_reg(index, result, bus);
                }
            }));
        }
    }

    /// The CB transform shared by every operand kind: returns the result and
    /// whether the opcode is BIT (which writes nothing back).
    fn cb_alu(&mut self, cb: u8, value: u8) -> (u8, bool) {
        match cb {
            0x00..=0x07 => (self.cb_rlc(value), false),
            0x08..=0x0F => (self.cb_rrc(value), false),
            0x10..=0x17 => (self.cb_rl(value), false),
            0x18..=0x1F => (self.cb_rr(value), false),
            0x20..=0x27 => (self.cb_sla(value), false),
            0x28..=0x2F => (self.cb_sra(value), false),
            0x30..=0x37 => (self.cb_swap(value), false),
            0x38..=0x3F => (self.cb_srl(value), false),
            0x40..=0x7F => {
                self.cb_bit(value, (cb >> 3) & 0x07);
                (value, true)
            }
            0x80..=0xBF => (value & !(1 << ((cb >> 3) & 0x07)), false),
            0xC0..=0xFF => (value | (1 << ((cb >> 3) & 0x07)), false),
        }
    }

    // --- CB rotate / shift primitives (Zero flag set from result) ---

    fn cb_rlc(&mut self, v: u8) -> u8 {
        let carry = (v >> 7) & 1;
        let r = v.rotate_left(1);
        self.set_flags(r == 0, false, false, carry == 1);
        r
    }

    fn cb_rrc(&mut self, v: u8) -> u8 {
        let carry = v & 1;
        let r = v.rotate_right(1);
        self.set_flags(r == 0, false, false, carry == 1);
        r
    }

    fn cb_rl(&mut self, v: u8) -> u8 {
        let carry_in = self.registers.get_flag_c() as u8;
        let carry = (v >> 7) & 1;
        let r = (v << 1) | carry_in;
        self.set_flags(r == 0, false, false, carry == 1);
        r
    }

    fn cb_rr(&mut self, v: u8) -> u8 {
        let carry_in = self.registers.get_flag_c() as u8;
        let carry = v & 1;
        let r = (v >> 1) | (carry_in << 7);
        self.set_flags(r == 0, false, false, carry == 1);
        r
    }

    fn cb_sla(&mut self, v: u8) -> u8 {
        let carry = (v >> 7) & 1;
        let r = v << 1;
        self.set_flags(r == 0, false, false, carry == 1);
        r
    }

    fn cb_sra(&mut self, v: u8) -> u8 {
        let carry = (v >> 7) & 1;
        let r = (v >> 1) | (v & 0x80); // preserve sign bit (arithmetic shift)
        self.set_flags(r == 0, false, false, carry == 1);
        r
    }

    fn cb_swap(&mut self, v: u8) -> u8 {
        let r = v.rotate_left(4); // swap the two nibbles
        self.set_flags(r == 0, false, false, false);
        r
    }

    fn cb_srl(&mut self, v: u8) -> u8 {
        let carry = v & 1;
        let r = v >> 1;
        self.set_flags(r == 0, false, false, carry == 1);
        r
    }

    fn cb_bit(&mut self, v: u8, bit: u8) {
        self.registers.set_flag_z((v >> bit) & 1 == 0);
        self.registers.set_flag_n(false);
        self.registers.set_flag_h(true);
        // Carry flag is unaffected.
    }
}
