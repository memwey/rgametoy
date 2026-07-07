use crate::console::bus::Bus;
use crate::console::registers::Registers;

/// The Sharp SM83 (LR35902) CPU core.
///
/// Instruction execution is *instruction-stepped* and returns the number of
/// T-cycles consumed. Every opcode of the base and `0xCB`-prefixed instruction
/// sets is implemented with hardware-accurate flag behaviour and M-cycle
/// timing. Interrupt dispatch, the `EI` one-instruction delay, `HALT`
/// (including the halt bug) and `DI`/`EI` are handled in [`Cpu::step`].
pub struct Cpu {
    registers: Registers,
    /// Interrupt Master Enable.
    ime: bool,
    /// Countdown implementing the one-instruction delay of `EI`.
    /// Set to 2 by `EI`; decremented at the start of each step; when it
    /// reaches 0 the IME flag is enabled.
    ei_delay: u8,
    /// Set while the CPU is halted (waiting for an interrupt).
    halted: bool,
    /// Set when the "halt bug" is triggered: the byte following `HALT` is
    /// fetched without advancing the program counter, so it is executed twice.
    halt_bug: bool,
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            registers: Registers::new(),
            ime: false,
            ei_delay: 0,
            halted: false,
            halt_bug: false,
        }
    }

    pub fn get_registers(&self) -> &Registers {
        &self.registers
    }

    pub fn get_registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }

    pub fn set_hl(&mut self, value: u16) {
        self.registers.set_hl(value);
    }

    pub fn set_bc(&mut self, value: u16) {
        self.registers.set_bc(value);
    }

    pub fn set_de(&mut self, value: u16) {
        self.registers.set_de(value);
    }

    pub fn set_af(&mut self, value: u16) {
        self.registers.set_af(value);
    }

    pub fn get_pc(&self) -> u16 {
        self.registers.pc
    }

    pub fn set_pc(&mut self, value: u16) {
        self.registers.pc = value;
    }

    pub fn get_sp(&self) -> u16 {
        self.registers.sp
    }

    pub fn set_sp(&mut self, value: u16) {
        self.registers.sp = value;
    }

    pub fn is_halted(&self) -> bool {
        self.halted
    }

    pub fn ime_enabled(&self) -> bool {
        self.ime
    }

    pub fn enable_interrupts(&mut self) {
        self.ime = true;
    }

    pub fn disable_interrupts(&mut self) {
        self.ime = false;
    }

    /// Execute a single CPU step: service a pending interrupt if one is due,
    /// otherwise fetch and execute one instruction. Returns the number of
    /// T-cycles consumed.
    pub fn step(&mut self, bus: &mut dyn Bus) -> u8 {
        // Apply the delayed effect of a previous `EI`.
        if self.ei_delay > 0 {
            self.ei_delay -= 1;
            if self.ei_delay == 0 {
                self.ime = true;
            }
        }

        // A pending interrupt wakes the CPU from HALT and, if IME is set,
        // is dispatched before the next instruction.
        let pending = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;
        if pending != 0 {
            self.halted = false;
            if self.ime {
                return self.service_interrupt(bus, pending);
            }
        }

        if self.halted {
            // The CPU is idle for one machine cycle while halted.
            return 4;
        }

        let opcode = self.fetch_byte(bus);
        if self.halt_bug {
            // Undo the PC increment so the byte after HALT executes twice.
            self.halt_bug = false;
            self.registers.pc = self.registers.pc.wrapping_sub(1);
        }
        self.execute(opcode, bus)
    }

    /// Dispatch the highest-priority pending interrupt.
    fn service_interrupt(&mut self, bus: &mut dyn Bus, pending: u8) -> u8 {
        self.ime = false;
        // The lowest set bit has the highest priority (VBlank first).
        let bit = pending.trailing_zeros() as u8;
        let if_reg = bus.read_byte(0xFF0F);
        bus.write_byte(0xFF0F, if_reg & !(1 << bit));
        self.push(bus, self.registers.pc);
        self.registers.pc = 0x0040 + (bit as u16) * 8;
        20
    }

    // --- Fetch helpers (advance PC) ---

    fn fetch_byte(&mut self, bus: &mut dyn Bus) -> u8 {
        let byte = bus.read_byte(self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        byte
    }

    fn fetch_word(&mut self, bus: &mut dyn Bus) -> u16 {
        let lo = self.fetch_byte(bus) as u16;
        let hi = self.fetch_byte(bus) as u16;
        (hi << 8) | lo
    }

    // --- Register-index helpers (B,C,D,E,H,L,(HL),A -> 0..=7) ---

    fn read_reg(&self, index: u8, bus: &mut dyn Bus) -> u8 {
        match index {
            0 => self.registers.get_b(),
            1 => self.registers.get_c(),
            2 => self.registers.get_d(),
            3 => self.registers.get_e(),
            4 => self.registers.get_h(),
            5 => self.registers.get_l(),
            6 => bus.read_byte(self.registers.get_hl()),
            7 => self.registers.get_a(),
            _ => unreachable!(),
        }
    }

    fn write_reg(&mut self, index: u8, value: u8, bus: &mut dyn Bus) {
        match index {
            0 => self.registers.set_b(value),
            1 => self.registers.set_c(value),
            2 => self.registers.set_d(value),
            3 => self.registers.set_e(value),
            4 => self.registers.set_h(value),
            5 => self.registers.set_l(value),
            6 => bus.write_byte(self.registers.get_hl(), value),
            7 => self.registers.set_a(value),
            _ => unreachable!(),
        }
    }

    fn set_flags(&mut self, z: bool, n: bool, h: bool, c: bool) {
        self.registers.set_flag_z(z);
        self.registers.set_flag_n(n);
        self.registers.set_flag_h(h);
        self.registers.set_flag_c(c);
    }

    // --- 8-bit ALU (operates on the accumulator) ---

    fn add_a(&mut self, value: u8, use_carry: bool) {
        let a = self.registers.get_a();
        let carry = if use_carry && self.registers.get_flag_c() { 1u8 } else { 0 };
        let result = a.wrapping_add(value).wrapping_add(carry);
        self.set_flags(
            result == 0,
            false,
            (a & 0x0F) + (value & 0x0F) + carry > 0x0F,
            (a as u16) + (value as u16) + (carry as u16) > 0xFF,
        );
        self.registers.set_a(result);
    }

    fn sub_a(&mut self, value: u8, use_carry: bool) {
        let result = self.sub_value(value, use_carry);
        self.registers.set_a(result);
    }

    /// Shared subtraction used by SUB/SBC and CP (which discards the result).
    fn sub_value(&mut self, value: u8, use_carry: bool) -> u8 {
        let a = self.registers.get_a();
        let carry = if use_carry && self.registers.get_flag_c() { 1i16 } else { 0 };
        let result = a.wrapping_sub(value).wrapping_sub(carry as u8);
        self.set_flags(
            result == 0,
            true,
            (a as i16 & 0x0F) - (value as i16 & 0x0F) - carry < 0,
            (a as i16) - (value as i16) - carry < 0,
        );
        result
    }

    fn and_a(&mut self, value: u8) {
        let result = self.registers.get_a() & value;
        self.set_flags(result == 0, false, true, false);
        self.registers.set_a(result);
    }

    fn or_a(&mut self, value: u8) {
        let result = self.registers.get_a() | value;
        self.set_flags(result == 0, false, false, false);
        self.registers.set_a(result);
    }

    fn xor_a(&mut self, value: u8) {
        let result = self.registers.get_a() ^ value;
        self.set_flags(result == 0, false, false, false);
        self.registers.set_a(result);
    }

    fn cp_a(&mut self, value: u8) {
        self.sub_value(value, false);
    }

    fn inc8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_add(1);
        self.registers.set_flag_z(result == 0);
        self.registers.set_flag_n(false);
        self.registers.set_flag_h((value & 0x0F) == 0x0F);
        // Carry flag is unaffected.
        result
    }

    fn dec8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_sub(1);
        self.registers.set_flag_z(result == 0);
        self.registers.set_flag_n(true);
        self.registers.set_flag_h((value & 0x0F) == 0x00);
        // Carry flag is unaffected.
        result
    }

    // --- 16-bit ALU ---

    fn add_hl(&mut self, value: u16) {
        let hl = self.registers.get_hl();
        let (result, carry) = hl.overflowing_add(value);
        self.registers.set_flag_n(false);
        self.registers
            .set_flag_h((hl & 0x0FFF) + (value & 0x0FFF) > 0x0FFF);
        self.registers.set_flag_c(carry);
        // Zero flag is unaffected.
        self.registers.set_hl(result);
    }

    /// Shared by `ADD SP,e8` and `LD HL,SP+e8`; flags come from the low byte.
    fn add_sp_e8(&mut self, offset: i8) -> u16 {
        let sp = self.registers.sp;
        let e = offset as i16 as u16;
        self.set_flags(
            false,
            false,
            (sp & 0x0F) + (e & 0x0F) > 0x0F,
            (sp & 0xFF) + (e & 0xFF) > 0xFF,
        );
        sp.wrapping_add(e)
    }

    // --- Accumulator rotates (these always clear the Zero flag) ---

    fn rlca(&mut self) {
        let a = self.registers.get_a();
        let carry = (a >> 7) & 1;
        self.registers.set_a(a.rotate_left(1));
        self.set_flags(false, false, false, carry == 1);
    }

    fn rrca(&mut self) {
        let a = self.registers.get_a();
        let carry = a & 1;
        self.registers.set_a(a.rotate_right(1));
        self.set_flags(false, false, false, carry == 1);
    }

    fn rla(&mut self) {
        let a = self.registers.get_a();
        let carry_in = self.registers.get_flag_c() as u8;
        let carry = (a >> 7) & 1;
        self.registers.set_a((a << 1) | carry_in);
        self.set_flags(false, false, false, carry == 1);
    }

    fn rra(&mut self) {
        let a = self.registers.get_a();
        let carry_in = self.registers.get_flag_c() as u8;
        let carry = a & 1;
        self.registers.set_a((a >> 1) | (carry_in << 7));
        self.set_flags(false, false, false, carry == 1);
    }

    fn daa(&mut self) {
        let mut a = self.registers.get_a();
        let mut correction: u8 = 0;
        let mut carry = self.registers.get_flag_c();
        if !self.registers.get_flag_n() {
            if self.registers.get_flag_h() || (a & 0x0F) > 0x09 {
                correction |= 0x06;
            }
            if carry || a > 0x99 {
                correction |= 0x60;
                carry = true;
            }
            a = a.wrapping_add(correction);
        } else {
            if self.registers.get_flag_h() {
                correction |= 0x06;
            }
            if carry {
                correction |= 0x60;
            }
            a = a.wrapping_sub(correction);
        }
        self.registers.set_a(a);
        self.registers.set_flag_z(a == 0);
        self.registers.set_flag_h(false);
        self.registers.set_flag_c(carry);
    }

    fn cpl(&mut self) {
        let a = self.registers.get_a();
        self.registers.set_a(!a);
        self.registers.set_flag_n(true);
        self.registers.set_flag_h(true);
    }

    fn scf(&mut self) {
        self.registers.set_flag_n(false);
        self.registers.set_flag_h(false);
        self.registers.set_flag_c(true);
    }

    fn ccf(&mut self) {
        let c = self.registers.get_flag_c();
        self.registers.set_flag_n(false);
        self.registers.set_flag_h(false);
        self.registers.set_flag_c(!c);
    }

    // --- Stack ---

    fn push(&mut self, bus: &mut dyn Bus, value: u16) {
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        bus.write_byte(self.registers.sp, (value >> 8) as u8);
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        bus.write_byte(self.registers.sp, value as u8);
    }

    fn pop(&mut self, bus: &mut dyn Bus) -> u16 {
        let lo = bus.read_byte(self.registers.sp) as u16;
        self.registers.sp = self.registers.sp.wrapping_add(1);
        let hi = bus.read_byte(self.registers.sp) as u16;
        self.registers.sp = self.registers.sp.wrapping_add(1);
        (hi << 8) | lo
    }

    // --- Control flow ---

    fn jr(&mut self, offset: i8) {
        self.registers.pc = self.registers.pc.wrapping_add(offset as i16 as u16);
    }

    fn call(&mut self, bus: &mut dyn Bus, addr: u16) {
        self.push(bus, self.registers.pc);
        self.registers.pc = addr;
    }

    fn ret(&mut self, bus: &mut dyn Bus) {
        self.registers.pc = self.pop(bus);
    }

    fn halt(&mut self, bus: &mut dyn Bus) -> u8 {
        let pending = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;
        if !self.ime && pending != 0 {
            // HALT with interrupts pending but disabled: the CPU does not halt
            // and the next byte is read twice (the halt bug).
            self.halt_bug = true;
        } else {
            self.halted = true;
        }
        4
    }

    /// Execute a single base (non-`CB`) opcode. Returns T-cycles consumed.
    fn execute(&mut self, opcode: u8, bus: &mut dyn Bus) -> u8 {
        match opcode {
            // --- 0x00..=0x3F: misc / loads / 16-bit / jumps ---
            0x00 => 4, // NOP
            0x01 => { let v = self.fetch_word(bus); self.registers.set_bc(v); 12 }
            0x02 => { bus.write_byte(self.registers.get_bc(), self.registers.get_a()); 8 }
            0x03 => { self.registers.set_bc(self.registers.get_bc().wrapping_add(1)); 8 }
            0x04 => { let v = self.inc8(self.registers.get_b()); self.registers.set_b(v); 4 }
            0x05 => { let v = self.dec8(self.registers.get_b()); self.registers.set_b(v); 4 }
            0x06 => { let v = self.fetch_byte(bus); self.registers.set_b(v); 8 }
            0x07 => { self.rlca(); 4 }
            0x08 => {
                let addr = self.fetch_word(bus);
                bus.write_byte(addr, self.registers.sp as u8);
                bus.write_byte(addr.wrapping_add(1), (self.registers.sp >> 8) as u8);
                20
            }
            0x09 => { self.add_hl(self.registers.get_bc()); 8 }
            0x0A => { let v = bus.read_byte(self.registers.get_bc()); self.registers.set_a(v); 8 }
            0x0B => { self.registers.set_bc(self.registers.get_bc().wrapping_sub(1)); 8 }
            0x0C => { let v = self.inc8(self.registers.get_c()); self.registers.set_c(v); 4 }
            0x0D => { let v = self.dec8(self.registers.get_c()); self.registers.set_c(v); 4 }
            0x0E => { let v = self.fetch_byte(bus); self.registers.set_c(v); 8 }
            0x0F => { self.rrca(); 4 }

            0x10 => { self.fetch_byte(bus); 4 } // STOP (consume the following byte)
            0x11 => { let v = self.fetch_word(bus); self.registers.set_de(v); 12 }
            0x12 => { bus.write_byte(self.registers.get_de(), self.registers.get_a()); 8 }
            0x13 => { self.registers.set_de(self.registers.get_de().wrapping_add(1)); 8 }
            0x14 => { let v = self.inc8(self.registers.get_d()); self.registers.set_d(v); 4 }
            0x15 => { let v = self.dec8(self.registers.get_d()); self.registers.set_d(v); 4 }
            0x16 => { let v = self.fetch_byte(bus); self.registers.set_d(v); 8 }
            0x17 => { self.rla(); 4 }
            0x18 => { let e = self.fetch_byte(bus) as i8; self.jr(e); 12 }
            0x19 => { self.add_hl(self.registers.get_de()); 8 }
            0x1A => { let v = bus.read_byte(self.registers.get_de()); self.registers.set_a(v); 8 }
            0x1B => { self.registers.set_de(self.registers.get_de().wrapping_sub(1)); 8 }
            0x1C => { let v = self.inc8(self.registers.get_e()); self.registers.set_e(v); 4 }
            0x1D => { let v = self.dec8(self.registers.get_e()); self.registers.set_e(v); 4 }
            0x1E => { let v = self.fetch_byte(bus); self.registers.set_e(v); 8 }
            0x1F => { self.rra(); 4 }

            0x20 => self.jr_cond(bus, !self.registers.get_flag_z()),
            0x21 => { let v = self.fetch_word(bus); self.registers.set_hl(v); 12 }
            0x22 => {
                let hl = self.registers.get_hl();
                bus.write_byte(hl, self.registers.get_a());
                self.registers.set_hl(hl.wrapping_add(1));
                8
            }
            0x23 => { self.registers.set_hl(self.registers.get_hl().wrapping_add(1)); 8 }
            0x24 => { let v = self.inc8(self.registers.get_h()); self.registers.set_h(v); 4 }
            0x25 => { let v = self.dec8(self.registers.get_h()); self.registers.set_h(v); 4 }
            0x26 => { let v = self.fetch_byte(bus); self.registers.set_h(v); 8 }
            0x27 => { self.daa(); 4 }
            0x28 => self.jr_cond(bus, self.registers.get_flag_z()),
            0x29 => { self.add_hl(self.registers.get_hl()); 8 }
            0x2A => {
                let hl = self.registers.get_hl();
                let v = bus.read_byte(hl);
                self.registers.set_a(v);
                self.registers.set_hl(hl.wrapping_add(1));
                8
            }
            0x2B => { self.registers.set_hl(self.registers.get_hl().wrapping_sub(1)); 8 }
            0x2C => { let v = self.inc8(self.registers.get_l()); self.registers.set_l(v); 4 }
            0x2D => { let v = self.dec8(self.registers.get_l()); self.registers.set_l(v); 4 }
            0x2E => { let v = self.fetch_byte(bus); self.registers.set_l(v); 8 }
            0x2F => { self.cpl(); 4 }

            0x30 => self.jr_cond(bus, !self.registers.get_flag_c()),
            0x31 => { let v = self.fetch_word(bus); self.registers.sp = v; 12 }
            0x32 => {
                let hl = self.registers.get_hl();
                bus.write_byte(hl, self.registers.get_a());
                self.registers.set_hl(hl.wrapping_sub(1));
                8
            }
            0x33 => { self.registers.sp = self.registers.sp.wrapping_add(1); 8 }
            0x34 => {
                let hl = self.registers.get_hl();
                let v = self.inc8(bus.read_byte(hl));
                bus.write_byte(hl, v);
                12
            }
            0x35 => {
                let hl = self.registers.get_hl();
                let v = self.dec8(bus.read_byte(hl));
                bus.write_byte(hl, v);
                12
            }
            0x36 => { let v = self.fetch_byte(bus); bus.write_byte(self.registers.get_hl(), v); 12 }
            0x37 => { self.scf(); 4 }
            0x38 => self.jr_cond(bus, self.registers.get_flag_c()),
            0x39 => { self.add_hl(self.registers.sp); 8 }
            0x3A => {
                let hl = self.registers.get_hl();
                let v = bus.read_byte(hl);
                self.registers.set_a(v);
                self.registers.set_hl(hl.wrapping_sub(1));
                8
            }
            0x3B => { self.registers.sp = self.registers.sp.wrapping_sub(1); 8 }
            0x3C => { let v = self.inc8(self.registers.get_a()); self.registers.set_a(v); 4 }
            0x3D => { let v = self.dec8(self.registers.get_a()); self.registers.set_a(v); 4 }
            0x3E => { let v = self.fetch_byte(bus); self.registers.set_a(v); 8 }
            0x3F => { self.ccf(); 4 }

            // --- 0x76: HALT (must precede the LD r,r' range) ---
            0x76 => self.halt(bus),

            // --- 0x40..=0x7F: LD r, r' ---
            0x40..=0x7F => {
                let dst = (opcode >> 3) & 0x07;
                let src = opcode & 0x07;
                let value = self.read_reg(src, bus);
                self.write_reg(dst, value, bus);
                if src == 6 || dst == 6 { 8 } else { 4 }
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
                if src == 6 { 8 } else { 4 }
            }

            // --- 0xC0..=0xFF: control flow, stack, immediates ---
            0xC0 => self.ret_cond(bus, !self.registers.get_flag_z()),
            0xC1 => { let v = self.pop(bus); self.registers.set_bc(v); 12 }
            0xC2 => self.jp_cond(bus, !self.registers.get_flag_z()),
            0xC3 => { let addr = self.fetch_word(bus); self.registers.pc = addr; 16 }
            0xC4 => self.call_cond(bus, !self.registers.get_flag_z()),
            0xC5 => { self.push(bus, self.registers.get_bc()); 16 }
            0xC6 => { let v = self.fetch_byte(bus); self.add_a(v, false); 8 }
            0xC7 => { self.call(bus, 0x00); 16 }
            0xC8 => self.ret_cond(bus, self.registers.get_flag_z()),
            0xC9 => { self.ret(bus); 16 }
            0xCA => self.jp_cond(bus, self.registers.get_flag_z()),
            0xCB => self.execute_cb(bus),
            0xCC => self.call_cond(bus, self.registers.get_flag_z()),
            0xCD => { let addr = self.fetch_word(bus); self.call(bus, addr); 24 }
            0xCE => { let v = self.fetch_byte(bus); self.add_a(v, true); 8 }
            0xCF => { self.call(bus, 0x08); 16 }

            0xD0 => self.ret_cond(bus, !self.registers.get_flag_c()),
            0xD1 => { let v = self.pop(bus); self.registers.set_de(v); 12 }
            0xD2 => self.jp_cond(bus, !self.registers.get_flag_c()),
            0xD4 => self.call_cond(bus, !self.registers.get_flag_c()),
            0xD5 => { self.push(bus, self.registers.get_de()); 16 }
            0xD6 => { let v = self.fetch_byte(bus); self.sub_a(v, false); 8 }
            0xD7 => { self.call(bus, 0x10); 16 }
            0xD8 => self.ret_cond(bus, self.registers.get_flag_c()),
            0xD9 => { self.ret(bus); self.ime = true; 16 } // RETI
            0xDA => self.jp_cond(bus, self.registers.get_flag_c()),
            0xDC => self.call_cond(bus, self.registers.get_flag_c()),
            0xDE => { let v = self.fetch_byte(bus); self.sub_a(v, true); 8 }
            0xDF => { self.call(bus, 0x18); 16 }

            0xE0 => { let a8 = self.fetch_byte(bus); bus.write_byte(0xFF00 + a8 as u16, self.registers.get_a()); 12 }
            0xE1 => { let v = self.pop(bus); self.registers.set_hl(v); 12 }
            0xE2 => { bus.write_byte(0xFF00 + self.registers.get_c() as u16, self.registers.get_a()); 8 }
            0xE5 => { self.push(bus, self.registers.get_hl()); 16 }
            0xE6 => { let v = self.fetch_byte(bus); self.and_a(v); 8 }
            0xE7 => { self.call(bus, 0x20); 16 }
            0xE8 => { let e = self.fetch_byte(bus) as i8; self.registers.sp = self.add_sp_e8(e); 16 }
            0xE9 => { self.registers.pc = self.registers.get_hl(); 4 } // JP (HL)
            0xEA => { let addr = self.fetch_word(bus); bus.write_byte(addr, self.registers.get_a()); 16 }
            0xEE => { let v = self.fetch_byte(bus); self.xor_a(v); 8 }
            0xEF => { self.call(bus, 0x28); 16 }

            0xF0 => { let a8 = self.fetch_byte(bus); let v = bus.read_byte(0xFF00 + a8 as u16); self.registers.set_a(v); 12 }
            0xF1 => { let v = self.pop(bus); self.registers.set_af(v); 12 }
            0xF2 => { let v = bus.read_byte(0xFF00 + self.registers.get_c() as u16); self.registers.set_a(v); 8 }
            0xF3 => { self.ime = false; self.ei_delay = 0; 4 } // DI
            0xF5 => { self.push(bus, self.registers.get_af()); 16 }
            0xF6 => { let v = self.fetch_byte(bus); self.or_a(v); 8 }
            0xF7 => { self.call(bus, 0x30); 16 }
            0xF8 => { let e = self.fetch_byte(bus) as i8; let v = self.add_sp_e8(e); self.registers.set_hl(v); 12 }
            0xF9 => { self.registers.sp = self.registers.get_hl(); 8 }
            0xFA => { let addr = self.fetch_word(bus); let v = bus.read_byte(addr); self.registers.set_a(v); 16 }
            0xFB => { self.ei_delay = 2; 4 } // EI (enabled after the next instruction)
            0xFE => { let v = self.fetch_byte(bus); self.cp_a(v); 8 }
            0xFF => { self.call(bus, 0x38); 16 }

            // Illegal / unused opcodes lock up real hardware; treat as no-ops.
            0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD => 4,
        }
    }

    /// Conditional relative jump. The operand is always consumed.
    fn jr_cond(&mut self, bus: &mut dyn Bus, take: bool) -> u8 {
        let e = self.fetch_byte(bus) as i8;
        if take {
            self.jr(e);
            12
        } else {
            8
        }
    }

    /// Conditional absolute jump. The operand is always consumed.
    fn jp_cond(&mut self, bus: &mut dyn Bus, take: bool) -> u8 {
        let addr = self.fetch_word(bus);
        if take {
            self.registers.pc = addr;
            16
        } else {
            12
        }
    }

    /// Conditional call. The operand is always consumed.
    fn call_cond(&mut self, bus: &mut dyn Bus, take: bool) -> u8 {
        let addr = self.fetch_word(bus);
        if take {
            self.call(bus, addr);
            24
        } else {
            12
        }
    }

    /// Conditional return.
    fn ret_cond(&mut self, bus: &mut dyn Bus, take: bool) -> u8 {
        if take {
            self.ret(bus);
            20
        } else {
            8
        }
    }

    /// Execute a `0xCB`-prefixed opcode. Returns T-cycles consumed.
    fn execute_cb(&mut self, bus: &mut dyn Bus) -> u8 {
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
