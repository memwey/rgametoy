//! Decode and execute the `0xCB`-prefixed opcode set (rotates, shifts, swap,
//! BIT/RES/SET) and its bit-manipulation primitives.

use super::Cpu;
use crate::console::bus::Bus;

impl Cpu {
    /// Execute a `0xCB`-prefixed opcode. Returns T-cycles consumed.
    pub(super) fn execute_cb(&mut self, bus: &mut dyn Bus) -> u8 {
        let cb = self.fetch_byte(bus);
        let index = cb & 0x07;
        let value = self.read_reg(index, bus);

        let (result, is_bit) = match cb {
            0x00..=0x07 => (self.cb_rlc(value), false),
            0x08..=0x0F => (self.cb_rrc(value), false),
            0x10..=0x17 => (self.cb_rl(value), false),
            0x18..=0x1F => (self.cb_rr(value), false),
            0x20..=0x27 => (self.cb_sla(value), false),
            0x28..=0x2F => (self.cb_sra(value), false),
            0x30..=0x37 => (self.cb_swap(value), false),
            0x38..=0x3F => (self.cb_srl(value), false),
            0x40..=0x7F => { self.cb_bit(value, (cb >> 3) & 0x07); (value, true) }
            0x80..=0xBF => (value & !(1 << ((cb >> 3) & 0x07)), false),
            0xC0..=0xFF => (value | (1 << ((cb >> 3) & 0x07)), false),
        };

        if !is_bit {
            self.write_reg(index, result, bus);
        }

        if index == 6 {
            if is_bit { 12 } else { 16 }
        } else {
            8
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
        let carry = v & 1;
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
