extern crate rgametoy_core;

use rgametoy_core::bus::{Bus, BusView, Soc};
use rgametoy_core::cartridge::Cartridge;

/// Run `f` against a fresh bus — the console's [`Soc`] paired with an empty
/// cartridge, borrowed for the closure's lifetime. This is the console-less way
/// to unit-test the bus and OAM-DMA behaviour (which needs `tick(n)` control the
/// public `Console` API deliberately doesn't expose).
fn with_bus(f: impl FnOnce(&mut BusView)) {
    let mut soc = Soc::new();
    let mut cart = Cartridge::new();
    let mut bus = BusView::new(&mut soc, &mut cart);
    f(&mut bus);
}

/// Bring an OAM DMA from `page` to the active state (past the one-M-cycle
/// startup delay), then hand the running bus to `f`.
fn with_active_dma(page: u8, f: impl FnOnce(&mut BusView)) {
    with_bus(|bus| {
        bus.write_byte(0xFF46, page);
        bus.tick(4); // startup delay
        bus.tick(4); // transfer now active
        f(bus);
    });
}

/// While OAM DMA runs, the CPU can only reach HRAM; other reads are open bus
/// (0xFF). The transfer lasts 160 M-cycles (640 T-cycles) and begins one idle
/// M-cycle after the FF46 write — OAM stays accessible during that gap.
#[test]
fn oam_dma_blocks_non_hram_for_the_transfer_window() {
    with_bus(|bus| {
        bus.write_byte(0xC000, 0x42); // WRAM source / probe byte
        bus.write_byte(0xFF80, 0x99); // HRAM byte

        assert_eq!(bus.read_byte(0xC000), 0x42, "WRAM readable before DMA");

        bus.write_byte(0xFF46, 0xC0); // request DMA from 0xC000

        // Startup delay: the transfer has not begun, so the bus is still open.
        bus.tick(4);
        assert_eq!(
            bus.read_byte(0xC000),
            0x42,
            "still accessible during startup delay"
        );

        // Next M-cycle the transfer starts and the bus is blocked.
        bus.tick(4);
        assert_eq!(
            bus.read_byte(0xC000),
            0xFF,
            "WRAM blocked once DMA is active"
        );
        assert_eq!(bus.read_byte(0xFF80), 0x99, "HRAM accessible during DMA");

        bus.tick(200);
        assert_eq!(bus.read_byte(0xC000), 0xFF, "still blocked mid-transfer");

        // Advance past the 640-T-cycle window (tick takes at most a u8 per call).
        for _ in 0..3 {
            bus.tick(200); // +600, past the 640 total
        }
        assert_eq!(
            bus.read_byte(0xC000),
            0x42,
            "WRAM accessible again after DMA"
        );
    });
}

/// The transfer only drives the bus its source lives on. A DMA from VRAM
/// ($80-$9F) occupies the *video* bus (VRAM + OAM), so the CPU can still read
/// ROM / WRAM on the external bus (mooneye source-bus behaviour behind the
/// control-flow read-timing tests).
#[test]
fn vram_source_dma_leaves_the_external_bus_free() {
    with_active_dma(0x80, |bus| {
        // OAM (the destination) is always locked.
        assert_eq!(bus.read_byte(0xFE00), 0xFF, "OAM locked during any DMA");
        // The video bus is busy, so VRAM reads are open bus.
        assert_eq!(
            bus.read_byte(0x8000),
            0xFF,
            "VRAM blocked by a VRAM-source DMA"
        );
        // The external bus is free: a WRAM byte reads back its real value.
        bus.write_byte(0xC000, 0x37);
        assert_eq!(
            bus.read_byte(0xC000),
            0x37,
            "WRAM free during a VRAM-source DMA"
        );
    });
}

/// A DMA from an external-bus source ($00-$7F, $A0-$FD) occupies ROM/RAM/WRAM,
/// so the CPU can still reach VRAM on the video bus.
#[test]
fn external_source_dma_leaves_vram_free() {
    with_bus(|bus| {
        bus.write_byte(0x8000, 0x5A); // seed VRAM before the DMA
        bus.write_byte(0xFF46, 0xC0); // source in WRAM (external bus)
        bus.tick(4);
        bus.tick(4); // active

        assert_eq!(bus.read_byte(0xFE00), 0xFF, "OAM locked during any DMA");
        assert_eq!(
            bus.read_byte(0xC000),
            0xFF,
            "external bus blocked by the DMA"
        );
        assert_eq!(
            bus.read_byte(0x8000),
            0x5A,
            "VRAM free during an external-source DMA"
        );
    });
}

/// OAM-DMA source pages $E0-$FF read the WRAM echo (the transfer never sees the
/// OAM / I/O map), so a DMA from $E0 copies the same bytes as one from $C0.
#[test]
fn dma_source_high_pages_mirror_wram_echo() {
    with_bus(|bus| {
        bus.write_byte(0xC000, 0x11);
        bus.write_byte(0xC001, 0x22);

        bus.write_byte(0xFF46, 0xE0); // echo of 0xC000
        for _ in 0..6 {
            bus.tick(200); // run the whole transfer (startup + 640 T) to completion
        }
        // OAM now holds the echoed WRAM bytes.
        assert_eq!(bus.read_byte(0xFE00), 0x11);
        assert_eq!(bus.read_byte(0xFE01), 0x22);
    });
}
