//! Arithmetic/logic helpers for the accumulator and 16-bit ops, plus the
//! accumulator rotates and BCD/flag instructions. These add methods to `Cpu`.

use super::Cpu;

impl Cpu {
    // --- 8-bit ALU (operates on the accumulator) ---

    pub(super) fn add_a(&mut self, value: u8, use_carry: bool) {
        let a = self.registers.get_a();
        let carry = if use_carry && self.registers.get_flag_c() {
            1u8
        } else {
            0
        };
        let result = a.wrapping_add(value).wrapping_add(carry);
        self.set_flags(
            result == 0,
            false,
            (a & 0x0F) + (value & 0x0F) + carry > 0x0F,
            (a as u16) + (value as u16) + (carry as u16) > 0xFF,
        );
        self.registers.set_a(result);
    }

    pub(super) fn sub_a(&mut self, value: u8, use_carry: bool) {
        let result = self.sub_value(value, use_carry);
        self.registers.set_a(result);
    }

    /// Shared subtraction used by SUB/SBC and CP (which discards the result).
    fn sub_value(&mut self, value: u8, use_carry: bool) -> u8 {
        let a = self.registers.get_a();
        let carry = if use_carry && self.registers.get_flag_c() {
            1i16
        } else {
            0
        };
        let result = a.wrapping_sub(value).wrapping_sub(carry as u8);
        self.set_flags(
            result == 0,
            true,
            (a as i16 & 0x0F) - (value as i16 & 0x0F) - carry < 0,
            (a as i16) - (value as i16) - carry < 0,
        );
        result
    }

    pub(super) fn and_a(&mut self, value: u8) {
        let result = self.registers.get_a() & value;
        self.set_flags(result == 0, false, true, false);
        self.registers.set_a(result);
    }

    pub(super) fn or_a(&mut self, value: u8) {
        let result = self.registers.get_a() | value;
        self.set_flags(result == 0, false, false, false);
        self.registers.set_a(result);
    }

    pub(super) fn xor_a(&mut self, value: u8) {
        let result = self.registers.get_a() ^ value;
        self.set_flags(result == 0, false, false, false);
        self.registers.set_a(result);
    }

    pub(super) fn cp_a(&mut self, value: u8) {
        self.sub_value(value, false);
    }

    pub(super) fn inc8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_add(1);
        self.registers.set_flag_z(result == 0);
        self.registers.set_flag_n(false);
        self.registers.set_flag_h((value & 0x0F) == 0x0F);
        // Carry flag is unaffected.
        result
    }

    pub(super) fn dec8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_sub(1);
        self.registers.set_flag_z(result == 0);
        self.registers.set_flag_n(true);
        self.registers.set_flag_h((value & 0x0F) == 0x00);
        // Carry flag is unaffected.
        result
    }

    // --- 16-bit ALU ---

    pub(super) fn add_hl(&mut self, value: u16) {
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
    pub(super) fn add_sp_e8(&mut self, offset: i8) -> u16 {
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

    pub(super) fn rlca(&mut self) {
        let a = self.registers.get_a();
        let carry = (a >> 7) & 1;
        self.registers.set_a(a.rotate_left(1));
        self.set_flags(false, false, false, carry == 1);
    }

    pub(super) fn rrca(&mut self) {
        let a = self.registers.get_a();
        let carry = a & 1;
        self.registers.set_a(a.rotate_right(1));
        self.set_flags(false, false, false, carry == 1);
    }

    pub(super) fn rla(&mut self) {
        let a = self.registers.get_a();
        let carry_in = self.registers.get_flag_c() as u8;
        let carry = (a >> 7) & 1;
        self.registers.set_a((a << 1) | carry_in);
        self.set_flags(false, false, false, carry == 1);
    }

    pub(super) fn rra(&mut self) {
        let a = self.registers.get_a();
        let carry_in = self.registers.get_flag_c() as u8;
        let carry = a & 1;
        self.registers.set_a((a >> 1) | (carry_in << 7));
        self.set_flags(false, false, false, carry == 1);
    }

    pub(super) fn daa(&mut self) {
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

    pub(super) fn cpl(&mut self) {
        let a = self.registers.get_a();
        self.registers.set_a(!a);
        self.registers.set_flag_n(true);
        self.registers.set_flag_h(true);
    }

    pub(super) fn scf(&mut self) {
        self.registers.set_flag_n(false);
        self.registers.set_flag_h(false);
        self.registers.set_flag_c(true);
    }

    pub(super) fn ccf(&mut self) {
        let c = self.registers.get_flag_c();
        self.registers.set_flag_n(false);
        self.registers.set_flag_h(false);
        self.registers.set_flag_c(!c);
    }
}
