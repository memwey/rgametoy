//! The SM83 register file: the four 16-bit pairs AF/BC/DE/HL plus the stack
//! pointer and program counter. Flags live in the low byte of AF — Z/N/H/C in
//! bits 7/6/5/4 — and its low nibble always reads 0.
//!
//! Fields are `pub(super)` so the CPU core reaches them directly (a register
//! *is* the data), while everything outside the `cpu` module goes through the
//! typed accessors below (the 8-bit halves do the byte extract/insert).

#[cfg(feature = "serialize")]
use crate::state::{write_u16_le, Reader, SaveStateError};

#[derive(Clone)]
pub struct Registers {
    // Accumulator and Flags
    pub(super) af: u16,
    pub(super) bc: u16,
    pub(super) de: u16,
    pub(super) hl: u16,
    // Stack Pointer
    pub(super) sp: u16,
    // Program Counter
    pub(super) pc: u16,
}

const ZERO_FLAG_BYTE_POSITION: u8 = 7; // z
const SUBTRACT_FLAG_BYTE_POSITION: u8 = 6; // n
const HALF_CARRY_FLAG_BYTE_POSITION: u8 = 5; // h
const CARRY_FLAG_BYTE_POSITION: u8 = 4; // c

impl Registers {
    pub fn new() -> Registers {
        Registers {
            af: 0,
            bc: 0,
            de: 0,
            hl: 0,
            pc: 0,
            sp: 0,
        }
    }

    pub fn get_flag_z(&self) -> bool {
        (self.af >> ZERO_FLAG_BYTE_POSITION) & 0b1 != 0
    }

    pub fn set_flag_z(&mut self, value: bool) {
        if value {
            self.af |= 1 << ZERO_FLAG_BYTE_POSITION;
        } else {
            self.af &= !(1 << ZERO_FLAG_BYTE_POSITION);
        }
    }

    pub fn get_flag_n(&self) -> bool {
        (self.af >> SUBTRACT_FLAG_BYTE_POSITION) & 0b1 != 0
    }

    pub fn set_flag_n(&mut self, value: bool) {
        if value {
            self.af |= 1 << SUBTRACT_FLAG_BYTE_POSITION;
        } else {
            self.af &= !(1 << SUBTRACT_FLAG_BYTE_POSITION);
        }
    }

    pub fn get_flag_h(&self) -> bool {
        (self.af >> HALF_CARRY_FLAG_BYTE_POSITION) & 0b1 != 0
    }

    pub fn set_flag_h(&mut self, value: bool) {
        if value {
            self.af |= 1 << HALF_CARRY_FLAG_BYTE_POSITION;
        } else {
            self.af &= !(1 << HALF_CARRY_FLAG_BYTE_POSITION);
        }
    }

    pub fn get_flag_c(&self) -> bool {
        (self.af >> CARRY_FLAG_BYTE_POSITION) & 0b1 != 0
    }

    pub fn set_flag_c(&mut self, value: bool) {
        if value {
            self.af |= 1 << CARRY_FLAG_BYTE_POSITION;
        } else {
            self.af &= !(1 << CARRY_FLAG_BYTE_POSITION);
        }
    }

    pub fn get_af(&self) -> u16 {
        self.af
    }

    pub fn set_af(&mut self, value: u16) {
        self.af = value & 0xFFF0;
    }

    pub fn get_bc(&self) -> u16 {
        self.bc
    }

    pub fn set_bc(&mut self, value: u16) {
        self.bc = value;
    }

    pub fn get_de(&self) -> u16 {
        self.de
    }

    pub fn set_de(&mut self, value: u16) {
        self.de = value;
    }

    pub fn get_hl(&self) -> u16 {
        self.hl
    }

    pub fn set_hl(&mut self, value: u16) {
        self.hl = value;
    }

    pub fn get_sp(&self) -> u16 {
        self.sp
    }

    pub fn get_pc(&self) -> u16 {
        self.pc
    }

    // --- Individual 8-bit register accessors ---
    // The Game Boy CPU exposes each 16-bit pair as two 8-bit registers.

    pub fn get_a(&self) -> u8 {
        (self.af >> 8) as u8
    }

    pub fn set_a(&mut self, value: u8) {
        self.af = (self.af & 0x00FF) | ((value as u16) << 8);
    }

    /// The flags register. Only the upper nibble (Z/N/H/C) is meaningful;
    /// the lower nibble always reads back as zero on real hardware.
    pub fn get_f(&self) -> u8 {
        (self.af & 0x00F0) as u8
    }

    pub fn set_f(&mut self, value: u8) {
        self.af = (self.af & 0xFF00) | ((value & 0xF0) as u16);
    }

    pub fn get_b(&self) -> u8 {
        (self.bc >> 8) as u8
    }

    pub fn set_b(&mut self, value: u8) {
        self.bc = (self.bc & 0x00FF) | ((value as u16) << 8);
    }

    pub fn get_c(&self) -> u8 {
        (self.bc & 0x00FF) as u8
    }

    pub fn set_c(&mut self, value: u8) {
        self.bc = (self.bc & 0xFF00) | (value as u16);
    }

    pub fn get_d(&self) -> u8 {
        (self.de >> 8) as u8
    }

    pub fn set_d(&mut self, value: u8) {
        self.de = (self.de & 0x00FF) | ((value as u16) << 8);
    }

    pub fn get_e(&self) -> u8 {
        (self.de & 0x00FF) as u8
    }

    pub fn set_e(&mut self, value: u8) {
        self.de = (self.de & 0xFF00) | (value as u16);
    }

    pub fn get_h(&self) -> u8 {
        (self.hl >> 8) as u8
    }

    pub fn set_h(&mut self, value: u8) {
        self.hl = (self.hl & 0x00FF) | ((value as u16) << 8);
    }

    pub fn get_l(&self) -> u8 {
        (self.hl & 0x00FF) as u8
    }

    pub fn set_l(&mut self, value: u8) {
        self.hl = (self.hl & 0xFF00) | (value as u16);
    }
}

impl Default for Registers {
    fn default() -> Self {
        Self::new()
    }
}

// -- Save state -------------------------------------------------------------

impl Registers {
    /// Append the six 16-bit pairs (12 bytes total) in the order AF, BC, DE,
    /// HL, SP, PC.
    #[cfg(feature = "serialize")]
    pub fn write_state(&self, out: &mut Vec<u8>) {
        write_u16_le(out, self.af);
        write_u16_le(out, self.bc);
        write_u16_le(out, self.de);
        write_u16_le(out, self.hl);
        write_u16_le(out, self.sp);
        write_u16_le(out, self.pc);
    }

    #[cfg(feature = "serialize")]
    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.af = r.read_u16_le()?;
        if self.af & 0x000F != 0 {
            return Err(SaveStateError::Corrupt); // F's low nibble is hard-wired to zero
        }
        self.bc = r.read_u16_le()?;
        self.de = r.read_u16_le()?;
        self.hl = r.read_u16_le()?;
        self.sp = r.read_u16_le()?;
        self.pc = r.read_u16_le()?;
        Ok(())
    }
}
