//! High RAM (HRAM, 0xFF80-0xFFFE): 127 bytes of RAM on the CPU die. It stays
//! accessible during OAM DMA (when the rest of the bus is blocked), which is why
//! DMA-wait routines are copied here and run from HRAM.

#[cfg(feature = "serialize")]
use crate::state::{write_bytes, Reader, SaveStateError};

const HRAM_SIZE: usize = 0x007F; // 127 bytes

#[derive(Clone)]
pub struct Hram {
    data: [u8; HRAM_SIZE],
}

impl Hram {
    pub fn new() -> Hram {
        Hram {
            data: [0; HRAM_SIZE],
        }
    }

    pub fn read_byte(&self, addr: u16) -> u8 {
        // HRAM addresses are 0xFF80-0xFFFE, mapped to 0x00-0x7E internally
        self.data[(addr - 0xFF80) as usize]
    }

    pub fn write_byte(&mut self, addr: u16, value: u8) {
        self.data[(addr - 0xFF80) as usize] = value;
    }
}

impl Default for Hram {
    fn default() -> Self {
        Self::new()
    }
}

// -- Save state -------------------------------------------------------------

impl Hram {
    #[cfg(feature = "serialize")]
    pub fn write_state(&self, out: &mut Vec<u8>) {
        write_bytes(out, &self.data);
    }

    #[cfg(feature = "serialize")]
    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        let bytes = r.read_exact(HRAM_SIZE)?;
        self.data.copy_from_slice(bytes);
        Ok(())
    }
}
