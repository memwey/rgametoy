//! Decode the base (non-`0xCB`) opcode set into one-M-cycle micro-ops (see
//! [`MicroOp`]). Cycle counts are not returned: each bus access / internal
//! delay is its own micro-op, so timing lives in the placement of those
//! micro-ops — verified against the golden table in `tests/cpu_timing_test.rs`.
//!
//! Zero-cycle work — register-only, no bus access, no machine cycle of its own
//! — rides the opcode-fetch M-cycle. Where it is *all* an instruction does it
//! runs inline at the head of `decode` (no micro-op is built); where it is the
//! tail of a longer instruction, e.g. `LD BC, nn` loading the pair after both
//! operand fetches, it is a `MicroOp::zero` riding the M-cycle before it. The
//! scratch latches `tmp8` / `tmp16` carry values between an instruction's
//! micro-ops (an immediate byte, a popped word, …).

use super::{Cpu, MicroOp, OpQueue};
use crate::bus::Bus;

impl Cpu {
    pub(super) fn decode(&mut self, opcode: u8, bus: &mut dyn Bus) {
        // `LD r, r'` and `ALU A, r` with register operands only — by far the
        // most executed instructions — are *zero-cycle*: they do no bus access
        // and occupy no machine cycle of their own. A zero-cycle micro-op is
        // by definition not a cycle, so rather than box one and immediately
        // pop it back off, run the work here, in the fetch M-cycle it rides.
        // The `(HL)` forms (index 6) *are* a bus access and fall through to
        // the queue below. `0x76` is HALT, not `LD (HL), (HL)`.
        let dst = (opcode >> 3) & 0x07;
        let src = opcode & 0x07;
        match opcode {
            0x40..=0x7F if opcode != 0x76 && dst != 6 && src != 6 => {
                let value = self.read_reg(src, bus);
                self.write_reg(dst, value, bus);
                return;
            }
            0x80..=0xBF if src != 6 => {
                let value = self.read_reg(src, bus);
                self.alu_op(dst, value);
                return;
            }
            _ => {}
        }

        // Branch conditions are sampled here, at the end of the opcode fetch.
        // Nothing between this point and the condition test can change the
        // flags, so this is the same value the test M-cycle would read.
        let flag_z = self.registers.get_flag_z();
        let flag_c = self.registers.get_flag_c();
        let ops = &mut self.ops;
        match opcode {
            // --- 0x00..=0x3F: misc / loads / 16-bit / jumps ---
            0x00 => {} // NOP
            0x01 => {
                Self::emit_fetch_word(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.set_bc(cpu.tmp16)));
            }
            0x02 => {
                ops.push_back(MicroOp::new(|cpu, bus| {
                    cpu.write(bus, cpu.registers.get_bc(), cpu.registers.get_a());
                }));
            }
            0x03 => {
                ops.push_back(MicroOp::new(|cpu, _| {
                    cpu.registers.set_bc(cpu.registers.get_bc().wrapping_add(1));
                }));
            }
            0x04 => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.inc8(cpu.registers.get_b());
                cpu.registers.set_b(v);
            })),
            0x05 => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.dec8(cpu.registers.get_b());
                cpu.registers.set_b(v);
            })),
            0x06 => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.registers.set_b(v);
            })),
            0x07 => ops.push_back(MicroOp::zero(|cpu, _| cpu.rlca())),
            0x08 => {
                Self::emit_fetch_word(ops);
                ops.push_back(MicroOp::new(|cpu, bus| {
                    cpu.write(bus, cpu.tmp16, cpu.registers.sp as u8);
                }));
                ops.push_back(MicroOp::new(|cpu, bus| {
                    cpu.write(
                        bus,
                        cpu.tmp16.wrapping_add(1),
                        (cpu.registers.sp >> 8) as u8,
                    );
                }));
            }
            0x09 => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.add_hl(cpu.registers.get_bc());
            })),
            0x0A => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.read(bus, cpu.registers.get_bc());
                cpu.registers.set_a(v);
            })),
            0x0B => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.registers.set_bc(cpu.registers.get_bc().wrapping_sub(1));
            })),
            0x0C => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.inc8(cpu.registers.get_c());
                cpu.registers.set_c(v);
            })),
            0x0D => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.dec8(cpu.registers.get_c());
                cpu.registers.set_c(v);
            })),
            0x0E => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.registers.set_c(v);
            })),
            0x0F => ops.push_back(MicroOp::zero(|cpu, _| cpu.rrca())),

            0x10 => ops.push_back(MicroOp::new(|cpu, bus| {
                cpu.fetch_byte(bus);
            })), // STOP (consume the following byte)
            0x11 => {
                Self::emit_fetch_word(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.set_de(cpu.tmp16)));
            }
            0x12 => ops.push_back(MicroOp::new(|cpu, bus| {
                cpu.write(bus, cpu.registers.get_de(), cpu.registers.get_a());
            })),
            0x13 => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.registers.set_de(cpu.registers.get_de().wrapping_add(1));
            })),
            0x14 => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.inc8(cpu.registers.get_d());
                cpu.registers.set_d(v);
            })),
            0x15 => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.dec8(cpu.registers.get_d());
                cpu.registers.set_d(v);
            })),
            0x16 => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.registers.set_d(v);
            })),
            0x17 => ops.push_back(MicroOp::zero(|cpu, _| cpu.rla())),
            0x18 => {
                ops.push_back(MicroOp::new(|cpu, bus| cpu.tmp8 = cpu.fetch_byte(bus)));
                ops.push_back(MicroOp::new(|cpu, _| {
                    cpu.jr(cpu.tmp8 as i8);
                }));
            }
            0x19 => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.add_hl(cpu.registers.get_de());
            })),
            0x1A => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.read(bus, cpu.registers.get_de());
                cpu.registers.set_a(v);
            })),
            0x1B => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.registers.set_de(cpu.registers.get_de().wrapping_sub(1));
            })),
            0x1C => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.inc8(cpu.registers.get_e());
                cpu.registers.set_e(v);
            })),
            0x1D => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.dec8(cpu.registers.get_e());
                cpu.registers.set_e(v);
            })),
            0x1E => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.registers.set_e(v);
            })),
            0x1F => ops.push_back(MicroOp::zero(|cpu, _| cpu.rra())),

            0x20 => Self::decode_jr(ops, !flag_z),
            0x21 => {
                Self::emit_fetch_word(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.set_hl(cpu.tmp16)));
            }
            0x22 => ops.push_back(MicroOp::new(|cpu, bus| {
                let hl = cpu.registers.get_hl();
                cpu.write(bus, hl, cpu.registers.get_a());
                cpu.registers.set_hl(hl.wrapping_add(1));
            })),
            0x23 => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.registers.set_hl(cpu.registers.get_hl().wrapping_add(1));
            })),
            0x24 => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.inc8(cpu.registers.get_h());
                cpu.registers.set_h(v);
            })),
            0x25 => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.dec8(cpu.registers.get_h());
                cpu.registers.set_h(v);
            })),
            0x26 => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.registers.set_h(v);
            })),
            0x27 => ops.push_back(MicroOp::zero(|cpu, _| cpu.daa())),
            0x28 => Self::decode_jr(ops, flag_z),
            0x29 => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.add_hl(cpu.registers.get_hl());
            })),
            0x2A => ops.push_back(MicroOp::new(|cpu, bus| {
                let hl = cpu.registers.get_hl();
                let v = cpu.read(bus, hl);
                cpu.registers.set_a(v);
                cpu.registers.set_hl(hl.wrapping_add(1));
            })),
            0x2B => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.registers.set_hl(cpu.registers.get_hl().wrapping_sub(1));
            })),
            0x2C => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.inc8(cpu.registers.get_l());
                cpu.registers.set_l(v);
            })),
            0x2D => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.dec8(cpu.registers.get_l());
                cpu.registers.set_l(v);
            })),
            0x2E => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.registers.set_l(v);
            })),
            0x2F => ops.push_back(MicroOp::zero(|cpu, _| cpu.cpl())),

            0x30 => Self::decode_jr(ops, !flag_c),
            0x31 => {
                Self::emit_fetch_word(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.sp = cpu.tmp16));
            }
            0x32 => ops.push_back(MicroOp::new(|cpu, bus| {
                let hl = cpu.registers.get_hl();
                cpu.write(bus, hl, cpu.registers.get_a());
                cpu.registers.set_hl(hl.wrapping_sub(1));
            })),
            0x33 => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
            })),
            0x34 => {
                ops.push_back(MicroOp::new(|cpu, bus| {
                    cpu.tmp8 = cpu.read(bus, cpu.registers.get_hl());
                }));
                ops.push_back(MicroOp::new(|cpu, bus| {
                    let r = cpu.inc8(cpu.tmp8);
                    let hl = cpu.registers.get_hl();
                    cpu.write(bus, hl, r);
                }));
            }
            0x35 => {
                ops.push_back(MicroOp::new(|cpu, bus| {
                    cpu.tmp8 = cpu.read(bus, cpu.registers.get_hl());
                }));
                ops.push_back(MicroOp::new(|cpu, bus| {
                    let r = cpu.dec8(cpu.tmp8);
                    let hl = cpu.registers.get_hl();
                    cpu.write(bus, hl, r);
                }));
            }
            0x36 => {
                ops.push_back(MicroOp::new(|cpu, bus| cpu.tmp8 = cpu.fetch_byte(bus)));
                ops.push_back(MicroOp::new(|cpu, bus| {
                    let hl = cpu.registers.get_hl();
                    cpu.write(bus, hl, cpu.tmp8);
                }));
            }
            0x37 => ops.push_back(MicroOp::zero(|cpu, _| cpu.scf())),
            0x38 => Self::decode_jr(ops, flag_c),
            0x39 => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.add_hl(cpu.registers.sp);
            })),
            0x3A => ops.push_back(MicroOp::new(|cpu, bus| {
                let hl = cpu.registers.get_hl();
                let v = cpu.read(bus, hl);
                cpu.registers.set_a(v);
                cpu.registers.set_hl(hl.wrapping_sub(1));
            })),
            0x3B => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
            })),
            0x3C => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.inc8(cpu.registers.get_a());
                cpu.registers.set_a(v);
            })),
            0x3D => ops.push_back(MicroOp::zero(|cpu, _| {
                let v = cpu.dec8(cpu.registers.get_a());
                cpu.registers.set_a(v);
            })),
            0x3E => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.registers.set_a(v);
            })),
            0x3F => ops.push_back(MicroOp::zero(|cpu, _| cpu.ccf())),

            // --- 0x76: HALT (must precede the LD r,r' range) ---
            0x76 => ops.push_back(MicroOp::zero(|cpu, bus| cpu.halt(bus))),

            // --- 0x40..=0x7F: LD r, r' — only the `(HL)` forms are a machine
            //     cycle; the register-only moves ran inline above. ---
            0x40..=0x7F => {
                ops.push_back(MicroOp::new(move |cpu, bus| {
                    let value = cpu.read_reg(src, bus);
                    cpu.write_reg(dst, value, bus);
                }));
            }

            // --- 0x80..=0xBF: 8-bit ALU A, r — likewise only `A, (HL)`. ---
            0x80..=0xBF => {
                ops.push_back(MicroOp::new(move |cpu, bus| {
                    let value = cpu.read_reg(src, bus);
                    cpu.alu_op(dst, value);
                }));
            }

            // --- 0xC0..=0xFF: control flow, stack, immediates ---
            0xC0 => Self::decode_ret(ops, !flag_z),
            0xC1 => {
                Self::emit_pop(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.set_bc(cpu.tmp16)));
            }
            0xC2 => Self::decode_jp(ops, !flag_z),
            0xC3 => {
                Self::emit_fetch_word(ops);
                Self::emit_tick(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.pc = cpu.tmp16));
            }
            0xC4 => Self::decode_call(ops, !flag_z),
            0xC5 => {
                Self::emit_tick(ops);
                Self::emit_push(ops, |cpu| cpu.registers.get_bc());
            }
            0xC6 => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.add_a(v, false);
            })),
            0xC7 => Self::decode_rst(ops, 0x00),
            0xC8 => Self::decode_ret(ops, flag_z),
            0xC9 => {
                Self::emit_pop(ops);
                Self::emit_tick(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.pc = cpu.tmp16));
            }
            0xCA => Self::decode_jp(ops, flag_z),
            0xCB => ops.push_back(MicroOp::new(|cpu, bus| {
                let cb = cpu.fetch_byte(bus);
                cpu.decode_cb(cb);
            })),
            0xCC => Self::decode_call(ops, flag_z),
            0xCD => {
                Self::emit_fetch_word(ops);
                Self::emit_tick(ops);
                Self::emit_push(ops, |cpu| cpu.registers.pc);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.pc = cpu.tmp16));
            }
            0xCE => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.add_a(v, true);
            })),
            0xCF => Self::decode_rst(ops, 0x08),

            0xD0 => Self::decode_ret(ops, !flag_c),
            0xD1 => {
                Self::emit_pop(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.set_de(cpu.tmp16)));
            }
            0xD2 => Self::decode_jp(ops, !flag_c),
            0xD4 => Self::decode_call(ops, !flag_c),
            0xD5 => {
                Self::emit_tick(ops);
                Self::emit_push(ops, |cpu| cpu.registers.get_de());
            }
            0xD6 => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.sub_a(v, false);
            })),
            0xD7 => Self::decode_rst(ops, 0x10),
            0xD8 => Self::decode_ret(ops, flag_c),
            0xD9 => {
                Self::emit_pop(ops);
                Self::emit_tick(ops);
                ops.push_back(MicroOp::zero(|cpu, _| {
                    cpu.registers.pc = cpu.tmp16;
                    cpu.ime = true;
                }));
            } // RETI
            0xDA => Self::decode_jp(ops, flag_c),
            0xDC => Self::decode_call(ops, flag_c),
            0xDE => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.sub_a(v, true);
            })),
            0xDF => Self::decode_rst(ops, 0x18),

            0xE0 => {
                ops.push_back(MicroOp::new(|cpu, bus| cpu.tmp8 = cpu.fetch_byte(bus)));
                ops.push_back(MicroOp::new(|cpu, bus| {
                    cpu.write(bus, 0xFF00 + cpu.tmp8 as u16, cpu.registers.get_a());
                }));
            }
            0xE1 => {
                Self::emit_pop(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.set_hl(cpu.tmp16)));
            }
            0xE2 => ops.push_back(MicroOp::new(|cpu, bus| {
                cpu.write(
                    bus,
                    0xFF00 + cpu.registers.get_c() as u16,
                    cpu.registers.get_a(),
                );
            })),
            0xE5 => {
                Self::emit_tick(ops);
                Self::emit_push(ops, |cpu| cpu.registers.get_hl());
            }
            0xE6 => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.and_a(v);
            })),
            0xE7 => Self::decode_rst(ops, 0x20),
            0xE8 => {
                ops.push_back(MicroOp::new(|cpu, bus| {
                    let e = cpu.fetch_byte(bus) as i8;
                    cpu.tmp16 = cpu.add_sp_e8(e);
                }));
                Self::emit_tick(ops);
                Self::emit_tick(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.sp = cpu.tmp16));
            }
            0xE9 => ops.push_back(MicroOp::zero(|cpu, _| {
                cpu.registers.pc = cpu.registers.get_hl();
            })), // JP (HL)
            0xEA => {
                Self::emit_fetch_word(ops);
                ops.push_back(MicroOp::new(|cpu, bus| {
                    cpu.write(bus, cpu.tmp16, cpu.registers.get_a());
                }));
            }
            0xEE => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.xor_a(v);
            })),
            0xEF => Self::decode_rst(ops, 0x28),

            0xF0 => {
                ops.push_back(MicroOp::new(|cpu, bus| cpu.tmp8 = cpu.fetch_byte(bus)));
                ops.push_back(MicroOp::new(|cpu, bus| {
                    let v = cpu.read(bus, 0xFF00 + cpu.tmp8 as u16);
                    cpu.registers.set_a(v);
                }));
            }
            0xF1 => {
                Self::emit_pop(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.set_af(cpu.tmp16)));
            }
            0xF2 => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.read(bus, 0xFF00 + cpu.registers.get_c() as u16);
                cpu.registers.set_a(v);
            })),
            0xF3 => ops.push_back(MicroOp::zero(|cpu, _| {
                cpu.ime = false;
                cpu.ime_pending = false;
            })), // DI
            0xF5 => {
                Self::emit_tick(ops);
                Self::emit_push(ops, |cpu| cpu.registers.get_af());
            }
            0xF6 => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.or_a(v);
            })),
            0xF7 => Self::decode_rst(ops, 0x30),
            0xF8 => {
                ops.push_back(MicroOp::new(|cpu, bus| {
                    let e = cpu.fetch_byte(bus) as i8;
                    cpu.tmp16 = cpu.add_sp_e8(e);
                }));
                Self::emit_tick(ops);
                ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.set_hl(cpu.tmp16)));
            }
            0xF9 => ops.push_back(MicroOp::new(|cpu, _| {
                cpu.registers.sp = cpu.registers.get_hl();
            })),
            0xFA => {
                Self::emit_fetch_word(ops);
                ops.push_back(MicroOp::new(|cpu, bus| {
                    let v = cpu.read(bus, cpu.tmp16);
                    cpu.registers.set_a(v);
                }));
            }
            0xFB => ops.push_back(MicroOp::zero(|cpu, _| {
                cpu.ime_pending = true;
            })), // EI (enabled after the next instruction)
            0xFE => ops.push_back(MicroOp::new(|cpu, bus| {
                let v = cpu.fetch_byte(bus);
                cpu.cp_a(v);
            })),
            0xFF => Self::decode_rst(ops, 0x38),

            // Illegal / unused opcodes hang the CPU on real hardware: it stops
            // fetching and only a reset recovers. Model that lock-up.
            0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD => {
                ops.push_back(MicroOp::zero(|cpu, _| cpu.locked = true));
            }
        }
    }

    /// The 8-bit ALU operation selected by bits 5-3 of an `0x80..=0xBF` opcode
    /// (or its `0xC6`-style immediate form): ADD / ADC / SUB / SBC / AND / XOR
    /// / OR / CP.
    fn alu_op(&mut self, alu: u8, value: u8) {
        match alu {
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

    /// `JR cc, e`: the operand byte is always fetched (one M-cycle); a taken
    /// branch costs one extra internal M-cycle.
    fn decode_jr(ops: &mut OpQueue, take: bool) {
        ops.push_back(MicroOp::new(|cpu, bus| cpu.tmp8 = cpu.fetch_byte(bus)));
        if take {
            ops.push_back(MicroOp::new(|cpu, _| {
                cpu.jr(cpu.tmp8 as i8);
            }));
        }
    }

    /// `JP cc, nn`: fetch the 16-bit address; a taken jump adds one internal
    /// M-cycle before loading PC.
    fn decode_jp(ops: &mut OpQueue, take: bool) {
        Self::emit_fetch_word(ops);
        if take {
            Self::emit_tick(ops);
            ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.pc = cpu.tmp16));
        }
    }

    /// `CALL cc, nn`: fetch the target; if taken, one internal M-cycle, push
    /// the return address, then load PC.
    fn decode_call(ops: &mut OpQueue, take: bool) {
        Self::emit_fetch_word(ops);
        if take {
            Self::emit_tick(ops);
            Self::emit_push(ops, |cpu| cpu.registers.pc);
            ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.pc = cpu.tmp16));
        }
    }

    /// `RST vec`: one internal M-cycle, push the return address, jump to `vec`.
    fn decode_rst(ops: &mut OpQueue, vec: u16) {
        Self::emit_tick(ops);
        Self::emit_push(ops, |cpu| cpu.registers.pc);
        ops.push_back(MicroOp::zero(move |cpu, _| cpu.registers.pc = vec));
    }

    /// `RET cc`: one internal M-cycle to test the condition, then a normal RET
    /// if taken.
    fn decode_ret(ops: &mut OpQueue, take: bool) {
        Self::emit_tick(ops);
        if take {
            Self::emit_pop(ops);
            Self::emit_tick(ops);
            ops.push_back(MicroOp::zero(|cpu, _| cpu.registers.pc = cpu.tmp16));
        }
    }
}
