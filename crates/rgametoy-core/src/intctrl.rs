//! The interrupt controller: the IE (0xFFFF) and IF (0xFF0F) registers and the
//! five maskable interrupt lines that feed IF. Each interrupt source raises its
//! line by OR-ing its bit into IF; the CPU samples `IE & IF` to decide dispatch.
//!
//! IF's top 3 bits read as 1 (only the lower 5 are real lines); both registers
//! are reached by the CPU through their MMIO addresses, so this type is a pure
//! storage / line-gathering unit with no timing of its own.

use crate::interrupts::InterruptType;
#[cfg(feature = "serialize")]
use crate::state::{write_u8, Reader, SaveStateError};

#[derive(Clone)]
pub struct IntCtrl {
    if_register: u8, // 0xFF0F — only the lower 5 bits are meaningful
    ie_register: u8, // 0xFFFF
}

impl IntCtrl {
    pub fn new() -> IntCtrl {
        IntCtrl {
            if_register: 0x00,
            ie_register: 0x00,
        }
    }

    /// Read an interrupt register (0xFF0F or 0xFFFF). IF's top 3 bits read as 1.
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF0F => self.if_register | 0xE0,
            0xFFFF => self.ie_register,
            _ => 0xFF,
        }
    }

    /// Write an interrupt register. IF is masked to its 5 lines.
    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF0F => self.if_register = value & 0x1F,
            0xFFFF => self.ie_register = value,
            _ => {}
        }
    }

    /// Raise one interrupt source's line (OR its bit into IF).
    pub fn request(&mut self, interrupt_type: InterruptType) {
        self.if_register |= interrupt_type.to_bit();
    }

    /// OR a raw 5-bit interrupt bitmask (e.g. the PPU's combined STAT/VBlank
    /// bits) into IF.
    pub fn request_mask(&mut self, mask: u8) {
        self.if_register |= mask & 0x1F;
    }
}

impl Default for IntCtrl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "serialize")]
impl IntCtrl {
    pub fn write_state(&self, out: &mut Vec<u8>) {
        write_u8(out, self.if_register);
        write_u8(out, self.ie_register);
    }

    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.if_register = r.read_u8()?;
        self.ie_register = r.read_u8()?;
        // Only IF has an invariant to check: `write` masks it to its five
        // lines, so anything outside that range means a corrupt blob. IE has
        // no unused bits — all eight are plain writable storage that reads
        // back verbatim, so every byte is a legal IE value.
        if self.if_register & !0x1F != 0 {
            return Err(SaveStateError::Corrupt);
        }
        Ok(())
    }
}
