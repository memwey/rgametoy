//! Decode and execute the base (non-`0xCB`) opcode set. Cycle counts are not
//! returned: each memory access and internal delay ticks the machine, so the
//! timing lives in the placement of `read`/`write`/`tick` calls.

use super::Cpu;
use crate::bus::Bus;

impl Cpu {
    pub(super) fn execute(&mut self, opcode: u8, bus: &mut impl Bus) {
        match opcode {
            // --- 0x00..=0x3F: misc / loads / 16-bit / jumps ---
            0x00 => {} // NOP
            0x01 => {
                let v = self.fetch_word(bus);
                self.registers.set_bc(v);
            }
            0x02 => {
                self.write(bus, self.registers.get_bc(), self.registers.get_a());
            }
            0x03 => {
                self.tick(bus);
                self.registers
                    .set_bc(self.registers.get_bc().wrapping_add(1));
            }
            0x04 => {
                let v = self.inc8(self.registers.get_b());
                self.registers.set_b(v);
            }
            0x05 => {
                let v = self.dec8(self.registers.get_b());
                self.registers.set_b(v);
            }
            0x06 => {
                let v = self.fetch_byte(bus);
                self.registers.set_b(v);
            }
            0x07 => self.rlca(),
            0x08 => {
                let addr = self.fetch_word(bus);
                self.write(bus, addr, self.registers.sp as u8);
                self.write(bus, addr.wrapping_add(1), (self.registers.sp >> 8) as u8);
            }
            0x09 => {
                self.tick(bus);
                self.add_hl(self.registers.get_bc());
            }
            0x0A => {
                let v = self.read(bus, self.registers.get_bc());
                self.registers.set_a(v);
            }
            0x0B => {
                self.tick(bus);
                self.registers
                    .set_bc(self.registers.get_bc().wrapping_sub(1));
            }
            0x0C => {
                let v = self.inc8(self.registers.get_c());
                self.registers.set_c(v);
            }
            0x0D => {
                let v = self.dec8(self.registers.get_c());
                self.registers.set_c(v);
            }
            0x0E => {
                let v = self.fetch_byte(bus);
                self.registers.set_c(v);
            }
            0x0F => self.rrca(),

            0x10 => {
                self.fetch_byte(bus);
            } // STOP (consume the following byte)
            0x11 => {
                let v = self.fetch_word(bus);
                self.registers.set_de(v);
            }
            0x12 => {
                self.write(bus, self.registers.get_de(), self.registers.get_a());
            }
            0x13 => {
                self.tick(bus);
                self.registers
                    .set_de(self.registers.get_de().wrapping_add(1));
            }
            0x14 => {
                let v = self.inc8(self.registers.get_d());
                self.registers.set_d(v);
            }
            0x15 => {
                let v = self.dec8(self.registers.get_d());
                self.registers.set_d(v);
            }
            0x16 => {
                let v = self.fetch_byte(bus);
                self.registers.set_d(v);
            }
            0x17 => self.rla(),
            0x18 => {
                let e = self.fetch_byte(bus) as i8;
                self.tick(bus);
                self.jr(e);
            }
            0x19 => {
                self.tick(bus);
                self.add_hl(self.registers.get_de());
            }
            0x1A => {
                let v = self.read(bus, self.registers.get_de());
                self.registers.set_a(v);
            }
            0x1B => {
                self.tick(bus);
                self.registers
                    .set_de(self.registers.get_de().wrapping_sub(1));
            }
            0x1C => {
                let v = self.inc8(self.registers.get_e());
                self.registers.set_e(v);
            }
            0x1D => {
                let v = self.dec8(self.registers.get_e());
                self.registers.set_e(v);
            }
            0x1E => {
                let v = self.fetch_byte(bus);
                self.registers.set_e(v);
            }
            0x1F => self.rra(),

            0x20 => self.jr_cond(bus, !self.registers.get_flag_z()),
            0x21 => {
                let v = self.fetch_word(bus);
                self.registers.set_hl(v);
            }
            0x22 => {
                let hl = self.registers.get_hl();
                self.write(bus, hl, self.registers.get_a());
                self.registers.set_hl(hl.wrapping_add(1));
            }
            0x23 => {
                self.tick(bus);
                self.registers
                    .set_hl(self.registers.get_hl().wrapping_add(1));
            }
            0x24 => {
                let v = self.inc8(self.registers.get_h());
                self.registers.set_h(v);
            }
            0x25 => {
                let v = self.dec8(self.registers.get_h());
                self.registers.set_h(v);
            }
            0x26 => {
                let v = self.fetch_byte(bus);
                self.registers.set_h(v);
            }
            0x27 => self.daa(),
            0x28 => self.jr_cond(bus, self.registers.get_flag_z()),
            0x29 => {
                self.tick(bus);
                self.add_hl(self.registers.get_hl());
            }
            0x2A => {
                let hl = self.registers.get_hl();
                let v = self.read(bus, hl);
                self.registers.set_a(v);
                self.registers.set_hl(hl.wrapping_add(1));
            }
            0x2B => {
                self.tick(bus);
                self.registers
                    .set_hl(self.registers.get_hl().wrapping_sub(1));
            }
            0x2C => {
                let v = self.inc8(self.registers.get_l());
                self.registers.set_l(v);
            }
            0x2D => {
                let v = self.dec8(self.registers.get_l());
                self.registers.set_l(v);
            }
            0x2E => {
                let v = self.fetch_byte(bus);
                self.registers.set_l(v);
            }
            0x2F => self.cpl(),

            0x30 => self.jr_cond(bus, !self.registers.get_flag_c()),
            0x31 => {
                let v = self.fetch_word(bus);
                self.registers.sp = v;
            }
            0x32 => {
                let hl = self.registers.get_hl();
                self.write(bus, hl, self.registers.get_a());
                self.registers.set_hl(hl.wrapping_sub(1));
            }
            0x33 => {
                self.tick(bus);
                self.registers.sp = self.registers.sp.wrapping_add(1);
            }
            0x34 => {
                let hl = self.registers.get_hl();
                let v = self.read(bus, hl);
                let r = self.inc8(v);
                self.write(bus, hl, r);
            }
            0x35 => {
                let hl = self.registers.get_hl();
                let v = self.read(bus, hl);
                let r = self.dec8(v);
                self.write(bus, hl, r);
            }
            0x36 => {
                let v = self.fetch_byte(bus);
                self.write(bus, self.registers.get_hl(), v);
            }
            0x37 => self.scf(),
            0x38 => self.jr_cond(bus, self.registers.get_flag_c()),
            0x39 => {
                self.tick(bus);
                self.add_hl(self.registers.sp);
            }
            0x3A => {
                let hl = self.registers.get_hl();
                let v = self.read(bus, hl);
                self.registers.set_a(v);
                self.registers.set_hl(hl.wrapping_sub(1));
            }
            0x3B => {
                self.tick(bus);
                self.registers.sp = self.registers.sp.wrapping_sub(1);
            }
            0x3C => {
                let v = self.inc8(self.registers.get_a());
                self.registers.set_a(v);
            }
            0x3D => {
                let v = self.dec8(self.registers.get_a());
                self.registers.set_a(v);
            }
            0x3E => {
                let v = self.fetch_byte(bus);
                self.registers.set_a(v);
            }
            0x3F => self.ccf(),

            // --- 0x76: HALT (must precede the LD r,r' range) ---
            0x76 => self.halt(bus),

            // --- 0x40..=0x7F: LD r, r' (a (HL) operand ticks via read/write_reg) ---
            0x40..=0x7F => {
                let dst = (opcode >> 3) & 0x07;
                let src = opcode & 0x07;
                let value = self.read_reg(src, bus);
                self.write_reg(dst, value, bus);
            }

            // --- 0x80..=0xBF: 8-bit ALU A, r ---
            0x80..=0xBF => {
                let src = opcode & 0x07;
                let value = self.read_reg(src, bus);
                match (opcode >> 3) & 0x07 {
                    0 => self.add_a(value, false),
                    1 => self.add_a(value, true),
                    2 => self.sub_a(value, false),
                    3 => self.sub_a(value, true),
                    4 => self.and_a(value),
                    5 => self.xor_a(value),
                    6 => self.or_a(value),
                    7 => self.cp_a(value),
                    _ => unreachable!(),
                }
            }

            // --- 0xC0..=0xFF: control flow, stack, immediates ---
            0xC0 => self.ret_cond(bus, !self.registers.get_flag_z()),
            0xC1 => {
                let v = self.pop(bus);
                self.registers.set_bc(v);
            }
            0xC2 => self.jp_cond(bus, !self.registers.get_flag_z()),
            0xC3 => {
                let addr = self.fetch_word(bus);
                self.tick(bus);
                self.registers.pc = addr;
            }
            0xC4 => self.call_cond(bus, !self.registers.get_flag_z()),
            0xC5 => {
                self.tick(bus);
                self.push(bus, self.registers.get_bc());
            }
            0xC6 => {
                let v = self.fetch_byte(bus);
                self.add_a(v, false);
            }
            0xC7 => self.call(bus, 0x00),
            0xC8 => self.ret_cond(bus, self.registers.get_flag_z()),
            0xC9 => self.ret(bus),
            0xCA => self.jp_cond(bus, self.registers.get_flag_z()),
            0xCB => self.execute_cb(bus),
            0xCC => self.call_cond(bus, self.registers.get_flag_z()),
            0xCD => {
                let addr = self.fetch_word(bus);
                self.call(bus, addr);
            }
            0xCE => {
                let v = self.fetch_byte(bus);
                self.add_a(v, true);
            }
            0xCF => self.call(bus, 0x08),

            0xD0 => self.ret_cond(bus, !self.registers.get_flag_c()),
            0xD1 => {
                let v = self.pop(bus);
                self.registers.set_de(v);
            }
            0xD2 => self.jp_cond(bus, !self.registers.get_flag_c()),
            0xD4 => self.call_cond(bus, !self.registers.get_flag_c()),
            0xD5 => {
                self.tick(bus);
                self.push(bus, self.registers.get_de());
            }
            0xD6 => {
                let v = self.fetch_byte(bus);
                self.sub_a(v, false);
            }
            0xD7 => self.call(bus, 0x10),
            0xD8 => self.ret_cond(bus, self.registers.get_flag_c()),
            0xD9 => {
                self.ret(bus);
                self.ime = true;
            } // RETI
            0xDA => self.jp_cond(bus, self.registers.get_flag_c()),
            0xDC => self.call_cond(bus, self.registers.get_flag_c()),
            0xDE => {
                let v = self.fetch_byte(bus);
                self.sub_a(v, true);
            }
            0xDF => self.call(bus, 0x18),

            0xE0 => {
                let a8 = self.fetch_byte(bus);
                self.write(bus, 0xFF00 + a8 as u16, self.registers.get_a());
            }
            0xE1 => {
                let v = self.pop(bus);
                self.registers.set_hl(v);
            }
            0xE2 => {
                self.write(
                    bus,
                    0xFF00 + self.registers.get_c() as u16,
                    self.registers.get_a(),
                );
            }
            0xE5 => {
                self.tick(bus);
                self.push(bus, self.registers.get_hl());
            }
            0xE6 => {
                let v = self.fetch_byte(bus);
                self.and_a(v);
            }
            0xE7 => self.call(bus, 0x20),
            0xE8 => {
                let e = self.fetch_byte(bus) as i8;
                let v = self.add_sp_e8(e);
                self.tick(bus);
                self.tick(bus);
                self.registers.sp = v;
            }
            0xE9 => {
                self.registers.pc = self.registers.get_hl();
            } // JP (HL)
            0xEA => {
                let addr = self.fetch_word(bus);
                self.write(bus, addr, self.registers.get_a());
            }
            0xEE => {
                let v = self.fetch_byte(bus);
                self.xor_a(v);
            }
            0xEF => self.call(bus, 0x28),

            0xF0 => {
                let a8 = self.fetch_byte(bus);
                let v = self.read(bus, 0xFF00 + a8 as u16);
                self.registers.set_a(v);
            }
            0xF1 => {
                let v = self.pop(bus);
                self.registers.set_af(v);
            }
            0xF2 => {
                let v = self.read(bus, 0xFF00 + self.registers.get_c() as u16);
                self.registers.set_a(v);
            }
            0xF3 => {
                self.ime = false;
                self.ime_pending = false;
            } // DI
            0xF5 => {
                self.tick(bus);
                self.push(bus, self.registers.get_af());
            }
            0xF6 => {
                let v = self.fetch_byte(bus);
                self.or_a(v);
            }
            0xF7 => self.call(bus, 0x30),
            0xF8 => {
                let e = self.fetch_byte(bus) as i8;
                let v = self.add_sp_e8(e);
                self.tick(bus);
                self.registers.set_hl(v);
            }
            0xF9 => {
                self.tick(bus);
                self.registers.sp = self.registers.get_hl();
            }
            0xFA => {
                let addr = self.fetch_word(bus);
                let v = self.read(bus, addr);
                self.registers.set_a(v);
            }
            0xFB => {
                self.ime_pending = true;
            } // EI (enabled after the next instruction)
            0xFE => {
                let v = self.fetch_byte(bus);
                self.cp_a(v);
            }
            0xFF => self.call(bus, 0x38),

            // Illegal / unused opcodes hang the CPU on real hardware: it stops
            // fetching and only a reset recovers. Model that lock-up.
            0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD => {
                self.locked = true;
            }
        }
    }
}
