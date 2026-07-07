extern crate rgametoy;

use rgametoy::console::bus::{Bus, MemoryBus};

/// While OAM DMA runs, the CPU can only reach HRAM; other reads are open bus
/// (0xFF). The transfer lasts 160 M-cycles (640 T-cycles).
#[test]
fn oam_dma_blocks_non_hram_for_the_transfer_window() {
    let mut bus = MemoryBus::new();
    bus.write_byte(0xC000, 0x42); // WRAM source / probe byte
    bus.write_byte(0xFF80, 0x99); // HRAM byte

    assert_eq!(bus.read_byte(0xC000), 0x42, "WRAM readable before DMA");

    bus.write_byte(0xFF46, 0xC0); // start DMA from 0xC000

    assert_eq!(bus.read_byte(0xC000), 0xFF, "WRAM blocked during DMA");
    assert_eq!(bus.read_byte(0xFF80), 0x99, "HRAM accessible during DMA");

    bus.tick(200);
    assert_eq!(bus.read_byte(0xC000), 0xFF, "still blocked mid-transfer");

    // Advance past the 640-T-cycle window (tick takes at most a u8 per call).
    for _ in 0..3 {
        bus.tick(200); // +600, 800 total
    }
    assert_eq!(bus.read_byte(0xC000), 0x42, "WRAM accessible again after DMA");
}
