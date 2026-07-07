extern crate rgametoy;

use rgametoy::cartridge::Cartridge;
use rgametoy::console::Console;

/// End-to-end: a hand-assembled ROM that boots at 0x0100, turns the LCD off,
/// writes a solid tile and a tile-map entry into VRAM through the bus, sets the
/// palette, turns the LCD back on and spins. Running it headless must produce a
/// rendered background in the PPU framebuffer — exercising cartridge loading,
/// the DMG boot state, the CPU, VRAM writes and the scanline renderer together.
#[test]
fn rom_boots_and_renders_background() {
    // Program placed at the 0x0100 entry point.
    #[rustfmt::skip]
    let program = [
        0x3E, 0x00,        // LD A, 0x00
        0xE0, 0x40,        // LDH (0x40), A     ; LCDC = 0 (LCD off)
        0x21, 0x10, 0x80,  // LD HL, 0x8010     ; tile 1 data
        0x3E, 0xFF,        // LD A, 0xFF
        0x06, 0x10,        // LD B, 16
        0x22,              // LD (HL+), A       ; loop: fill 16 bytes
        0x05,              // DEC B
        0x20, 0xFC,        // JR NZ, -4
        0x21, 0x00, 0x98,  // LD HL, 0x9800     ; tile map (0,0)
        0x3E, 0x01,        // LD A, 0x01
        0x77,              // LD (HL), A         ; -> tile 1
        0x3E, 0xE4,        // LD A, 0xE4
        0xE0, 0x47,        // LDH (0x47), A     ; BGP = identity
        0x3E, 0x91,        // LD A, 0x91
        0xE0, 0x40,        // LDH (0x40), A     ; LCDC = LCD on + BG on
        0x18, 0xFE,        // JR -2             ; spin forever
    ];

    let mut rom = vec![0u8; 0x8000];
    rom[0x0147] = 0x00; // ROM only (no MBC)
    rom[0x0100..0x0100 + program.len()].copy_from_slice(&program);

    let mut console = Console::new();
    console.load_cartridge(Cartridge::from_bytes(rom));

    // A few frames: the first sets up VRAM and re-enables the LCD; subsequent
    // frames render it.
    for _ in 0..5 {
        console.run_frame_headless();
    }

    let ppu = console.get_ppu();
    let ppu = ppu.borrow();
    let fb = ppu.framebuffer();

    // Top-left 8×8 tile is solid colour 3 -> shade 3.
    assert_eq!(fb[0], 3, "top-left pixel rendered");
    assert_eq!(fb[7], 3);
    assert_eq!(fb[160 * 7 + 7], 3, "bottom-right of the tile rendered");
    // The neighbouring tile (map entry (1,0) = 0) is blank -> shade 0.
    assert_eq!(fb[8], 0, "adjacent blank tile");
}
