//! Serial link port (SB/SC). No peer is connected, so a transfer shifts in
//! 0xFF; the bytes the program sends are captured so test ROMs (which print
//! their results over serial) can be observed.

#[cfg(feature = "serialize")]
use crate::state::{write_u16_le, write_u32_le, write_u8, Reader, SaveStateError};

/// T-cycles for one byte: 8 bits at the 8192 Hz DMG serial clock (512 T-cycles
/// per bit).
const TRANSFER_CYCLES: u16 = 512 * 8;

#[derive(Clone)]
pub struct Serial {
    data: u8,    // SB (0xFF01)
    control: u8, // SC (0xFF02)
    /// T-cycles left in the active transfer (0 = idle).
    countdown: u16,
    /// Bytes shifted out by the program, for test-ROM output.
    output: Vec<u8>,
}

impl Serial {
    pub fn new() -> Serial {
        Serial {
            data: 0x00,
            control: 0x00,
            countdown: 0,
            output: Vec::new(),
        }
    }

    /// Advance an in-progress transfer. Returns `true` when a byte completes,
    /// so the caller can raise a Serial interrupt.
    pub fn tick(&mut self, cycles: u8) -> bool {
        if self.countdown == 0 {
            return false;
        }
        let cycles = cycles as u16;
        if self.countdown > cycles {
            self.countdown -= cycles;
            return false;
        }
        // Transfer complete: capture the sent byte; with no peer the received
        // byte is 0xFF. Clear the transfer-start bit.
        self.countdown = 0;
        self.output.push(self.data);
        self.data = 0xFF;
        self.control &= 0x7F;
        true
    }

    pub fn read_register(&self, addr: u16) -> u8 {
        match addr {
            0xFF01 => self.data,
            0xFF02 => self.control | 0x7E, // unused bits 1-6 read as 1
            _ => 0xFF,
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF01 => self.data = value,
            0xFF02 => {
                self.control = value;
                // Transfer start (bit 7) with the internal clock (bit 0).
                if value & 0x81 == 0x81 {
                    self.countdown = TRANSFER_CYCLES;
                }
            }
            _ => {}
        }
    }

    /// Remove and return the bytes shifted out since the last call.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }
}

impl Default for Serial {
    fn default() -> Self {
        Self::new()
    }
}

// -- Save state -------------------------------------------------------------

impl Serial {
    /// Append the serial state: data, control, countdown, then the captured
    /// output bytes (length-prefixed because the buffer is variable-sized).
    #[cfg(feature = "serialize")]
    pub fn write_state(&self, out: &mut Vec<u8>) {
        write_u8(out, self.data);
        write_u8(out, self.control);
        write_u16_le(out, self.countdown);
        write_u32_le(out, self.output.len() as u32);
        for &b in &self.output {
            write_u8(out, b);
        }
    }

    #[cfg(feature = "serialize")]
    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.data = r.read_u8()?;
        self.control = r.read_u8()?;
        self.countdown = r.read_u16_le()?;
        let n = r.read_u32_le()? as usize;
        let bytes = r.read_exact(n)?;
        self.output = bytes.to_vec();
        Ok(())
    }
}
