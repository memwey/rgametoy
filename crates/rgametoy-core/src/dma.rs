//! The OAM DMA controller (the 0xFF46 unit). It is a bus master, distinct from
//! the PPU: writing $FF46 schedules a 160-byte copy from `source << 8` into OAM
//! ($FE00-$FE9F). While the transfer runs the CPU can reach only HRAM (and, on
//! the *other* bus, whichever the transfer is not driving) — see
//! `DmaController::conflicts`.
//!
//! This unit owns only the *timing*: the one-M-cycle startup delay, the 640-T
//! busy window, and the source-page register. The copy itself is performed
//! atomically by the bus owner that can reach the cartridge — OAM is locked for
//! the whole window, so the CPU cannot distinguish an atomic copy from a
//! byte-by-byte transfer. (In the crystal-driven model the controller becomes a
//! true per-cycle bus master fetching through the bus matrix; for now the
//! bulk-copy is observationally equivalent.)

#[cfg(feature = "serialize")]
use crate::state::{write_u16_le, write_u8, Reader, SaveStateError};

#[derive(Clone)]
pub struct DmaController {
    /// T-cycles remaining in an active OAM DMA transfer (0 = idle). While it
    /// runs the CPU can only reach HRAM (and the bus the transfer is not
    /// driving).
    remaining: u16,
    /// T-cycles until a just-requested transfer actually starts. Writing FF46
    /// does not begin the transfer immediately: there is a one-M-cycle idle gap
    /// (OAM stays accessible) before the busy window opens.
    delay: u8,
    /// High byte of the pending/active source address — i.e. the 0xFF46
    /// register itself, which the DMA unit owns (it is a bus master, not part
    /// of the PPU, so the register lives here rather than in the PPU's register
    /// file).
    source: u8,
}

impl DmaController {
    pub fn new() -> DmaController {
        DmaController {
            remaining: 0,
            delay: 0,
            source: 0,
        }
    }

    /// The 0xFF46 register read-back (the latched source page).
    pub(crate) fn source(&self) -> u8 {
        self.source
    }

    /// Write to 0xFF46: latch `value` as the source page and arm the startup
    /// delay. A request while a previous transfer runs lets that one keep
    /// blocking until the new one takes over.
    pub(crate) fn request(&mut self, value: u8) {
        self.source = value;
        self.delay = 8;
    }

    /// Does a CPU access to `addr` collide with the running transfer? The
    /// transfer drives one of the two buses depending on its source: a VRAM
    /// source ($80-$9F) drives the *video* bus (VRAM + OAM), any other source
    /// drives the *external* bus (ROM / cart RAM / WRAM + echo). The CPU may
    /// freely use the other bus, plus I/O and HRAM; only OAM (the destination)
    /// is locked regardless.
    pub(crate) fn conflicts(&self, addr: u16) -> bool {
        if self.remaining == 0 {
            return false;
        }
        let video_dma = (0x80..=0x9F).contains(&self.source);
        match addr {
            0xFE00..=0xFE9F => true,                         // OAM (destination)
            0x8000..=0x9FFF => video_dma,                    // VRAM (video bus)
            0x0000..=0x7FFF | 0xA000..=0xFDFF => !video_dma, // external bus
            _ => false,                                      // FEA0-FEFF, I/O, HRAM
        }
    }

    /// Advance the controller by `cycles` T-cycles. Returns `true` the moment a
    /// just-requested transfer finishes its startup delay and should begin —
    /// the caller then performs the 160-byte copy and calls [`begin`](Self::begin).
    pub(crate) fn tick(&mut self, cycles: u8) -> bool {
        if self.remaining > 0 {
            self.remaining = self.remaining.saturating_sub(cycles as u16);
        }
        let mut start_now = false;
        if self.delay > 0 {
            self.delay = self.delay.saturating_sub(cycles);
            if self.delay == 0 {
                start_now = true;
            }
        }
        start_now
    }

    /// Open the 640-T-cycle busy window, called by the bus owner once it has
    /// performed the copy.
    pub(crate) fn begin(&mut self) {
        self.remaining = 160 * 4; // 160 M-cycles
    }

    #[cfg(feature = "debug")]
    pub(crate) fn debug_state(&self) -> (bool, u8) {
        (self.remaining > 0, self.source)
    }
}

impl Default for DmaController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "serialize")]
impl DmaController {
    pub fn write_state(&self, out: &mut Vec<u8>) {
        write_u16_le(out, self.remaining);
        write_u8(out, self.delay);
        write_u8(out, self.source);
    }

    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.remaining = r.read_u16_le()?;
        self.delay = r.read_u8()?;
        self.source = r.read_u8()?;
        if self.remaining > 160 * 4 || self.delay > 8 {
            return Err(SaveStateError::Corrupt);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dma_with(source: u8, remaining: u16) -> DmaController {
        let mut d = DmaController::new();
        d.source = source;
        d.remaining = remaining;
        d
    }

    /// Idle DMA conflicts with nothing.
    #[test]
    fn no_conflict_when_dma_is_idle() {
        let dma = dma_with(0xC0, 0);
        for addr in [0x0000u16, 0x8000, 0xA000, 0xFE00, 0xFF80] {
            assert!(!dma.conflicts(addr), "{addr:#06x}");
        }
    }

    /// DMA from WRAM (0xC0) drives the *external* bus: OAM and the external bus
    /// are blocked; VRAM (a different bus) and HRAM stay accessible.
    #[test]
    fn dma_from_external_bus_blocks_external_and_oam() {
        let dma = dma_with(0xC0, 100);
        assert!(dma.conflicts(0xFE00), "OAM (destination)");
        assert!(dma.conflicts(0x4000), "ROM (external bus)");
        assert!(dma.conflicts(0xA000), "cart RAM (external bus)");
        assert!(!dma.conflicts(0x8000), "VRAM readable (video bus)");
        assert!(!dma.conflicts(0xFF80), "HRAM always accessible");
    }

    /// DMA from VRAM (0x80) drives the *video* bus: OAM and VRAM are blocked;
    /// the external bus and HRAM stay accessible.
    #[test]
    fn dma_from_video_bus_blocks_video_and_oam() {
        let dma = dma_with(0x80, 100);
        assert!(dma.conflicts(0xFE00), "OAM (destination)");
        assert!(dma.conflicts(0x9000), "VRAM (video bus)");
        assert!(!dma.conflicts(0x4000), "ROM readable (external bus)");
        assert!(!dma.conflicts(0xA000), "cart RAM readable (external bus)");
        assert!(!dma.conflicts(0xFF80), "HRAM always accessible");
    }
}
