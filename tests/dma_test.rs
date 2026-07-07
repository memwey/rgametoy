extern crate rgametoy;

use rgametoy::console::bus::{Bus, MemoryBus};

/// While OAM DMA runs, the CPU can only reach HRAM; other reads are open bus
/// (0xFF). The transfer lasts 160 M-cycles (640 T-cycles) and begins one idle
/// M-cycle after the FF46 write — OAM stays accessible during that gap.
#[test]
fn oam_dma_blocks_non_hram_for_the_transfer_window() {
    let mut bus = MemoryBus::new();
    bus.write_byte(0xC000, 0x42); // WRAM source / probe byte
    bus.write_byte(0xFF80, 0x99); // HRAM byte

    assert_eq!(bus.read_byte(0xC000), 0x42, "WRAM readable before DMA");

    bus.write_byte(0xFF46, 0xC0); // request DMA from 0xC000

    // Startup delay: the transfer has not begun yet, so the bus is still open.
    bus.tick(4);
    assert_eq!(bus.read_byte(0xC000), 0x42, "still accessible during startup delay");

    // Next M-cycle the transfer starts and the bus is blocked.
    bus.tick(4);
    assert_eq!(bus.read_byte(0xC000), 0xFF, "WRAM blocked once DMA is active");
    assert_eq!(bus.read_byte(0xFF80), 0x99, "HRAM accessible during DMA");

    bus.tick(200);
    assert_eq!(bus.read_byte(0xC000), 0xFF, "still blocked mid-transfer");

    // Advance past the 640-T-cycle window (tick takes at most a u8 per call).
    for _ in 0..3 {
        bus.tick(200); // +600, past the 640 total
    }
    assert_eq!(bus.read_byte(0xC000), 0x42, "WRAM accessible again after DMA");
}
