extern crate rgametoy;

use rgametoy::console::ppu::{Ppu, PpuMode};

/// Enable the LCD (LCDC bit 7) so the PPU state machine runs.
fn enabled_ppu() -> Ppu {
    let mut ppu = Ppu::new();
    ppu.write_register(0xFF40, 0x80);
    ppu
}

/// #2 regression: `tick` advances one PPU dot per T-cycle (no ×4). OAM scan
/// (mode 2) lasts exactly 80 dots.
#[test]
fn ppu_oam_scan_lasts_80_dots() {
    let mut ppu = enabled_ppu();
    // Skip the special first line after enable: no OAM scan and only
    // 452 dots (the PPU starts late). Line 1 is a normal line.
    ppu.tick(200); ppu.tick(200); ppu.tick(52);
    assert_eq!(ppu.ly, 1);
    assert_eq!(ppu.get_mode(), PpuMode::OamScan);

    ppu.tick(79);
    assert_eq!(ppu.get_mode(), PpuMode::OamScan, "still OAM scan after 79 dots");

    ppu.tick(1);
    assert_eq!(ppu.get_mode(), PpuMode::Drawing, "80th dot enters pixel drawing");
}

/// The first scanline after the LCD is enabled is special (mooneye
/// `lcdon_timing`): it starts in mode 0 and goes straight to mode 3 with no
/// OAM scan, and it is 4 dots short, so LY reaches 1 at dot 452 not 456.
#[test]
fn ppu_first_line_after_enable_is_short_and_skips_oam_scan() {
    let mut ppu = enabled_ppu();
    assert_eq!(ppu.ly, 0);
    assert_eq!(
        ppu.get_mode(),
        PpuMode::HBlank,
        "line 0 begins in mode 0, not OAM scan"
    );

    ppu.tick(80);
    assert_eq!(
        ppu.get_mode(),
        PpuMode::Drawing,
        "line 0 enters drawing at dot 80 without an OAM-scan phase"
    );

    ppu.tick(200);
    ppu.tick(171); // dot 451
    assert_eq!(ppu.ly, 0, "still on the short line 0 at dot 451");
    ppu.tick(1); // dot 452
    assert_eq!(ppu.ly, 1, "line 0 ends 4 dots early, at dot 452");
    assert_eq!(
        ppu.get_mode(),
        PpuMode::OamScan,
        "line 1 is a normal line and starts an OAM scan"
    );
}

/// Mode 3 stretches by the OBJ penalty, and stacked sprites at the same X pay
/// the background-fetch abort only once: the first sprite at X=0 costs 11 dots
/// and each further one at the same X costs just the 6-dot fetch (mooneye
/// `intr_2_mode0_timing_sprites`).
#[test]
fn ppu_stacked_sprite_penalty_aggregates() {
    // Measure mode-3 length on line 2 (a normal line) with the given sprite Xs.
    fn mode3_len(sprite_xs: &[u8]) -> u32 {
        let mut ppu = Ppu::new();
        for (i, &x) in sprite_xs.iter().enumerate() {
            let base = 0xFE00 + i as u16 * 4;
            ppu.write_oam(base, 18); // Y=18 -> screen line 2
            ppu.write_oam(base + 1, x);
            ppu.write_oam(base + 2, 0);
            ppu.write_oam(base + 3, 0);
        }
        let lcdc = if sprite_xs.is_empty() { 0x91 } else { 0x93 };
        ppu.write_register(0xFF40, lcdc);
        while ppu.ly != 2 {
            ppu.tick(1);
        }
        let (mut start, mut dot, mut len) = (0u32, 0u32, 0u32);
        while ppu.ly == 2 {
            let mode = ppu.get_mode();
            ppu.tick(1);
            dot += 1;
            if mode == PpuMode::OamScan && ppu.get_mode() == PpuMode::Drawing {
                start = dot;
            }
            if mode == PpuMode::Drawing && ppu.get_mode() == PpuMode::HBlank {
                len = dot - start;
            }
        }
        len
    }

    assert_eq!(mode3_len(&[]), 172, "baseline mode 3 is 172 dots");
    assert_eq!(mode3_len(&[0]), 183, "one X=0 sprite adds the full 11");
    assert_eq!(mode3_len(&[0, 0]), 189, "a second stacked sprite adds only 6");
    assert_eq!(mode3_len(&[0, 0, 0]), 195, "a third stacked sprite adds only 6");
}

/// A window positioned with WX < 7 has its left edge off-screen, so its first
/// (7 - WX) pixels are clipped (mealybug `m3_wx_*_change`). With WX=5 the two
/// leftmost window pixels are dropped, so screen x0 shows the window's third
/// pixel.
#[test]
fn ppu_window_with_wx_below_7_clips_its_left_edge() {
    let mut ppu = Ppu::new();

    // Window/BG tile 1: pixels 0-1 are colour 3, pixels 2-7 are colour 1.
    // low plane all 1s (colour bit 0), high plane only pixels 0-1 (colour bit 1).
    for row in 0..8 {
        ppu.write_vram(0x8010 + row * 2, 0xFF);
        ppu.write_vram(0x8010 + row * 2 + 1, 0xC0);
    }
    ppu.write_vram(0x9800, 0x01); // shared BG/window map (0,0) -> tile 1

    ppu.write_register(0xFF47, 0xE4); // BGP identity: colour n -> shade n
    ppu.write_register(0xFF4A, 0); // WY = 0 (window from the top)
    ppu.write_register(0xFF4B, 5); // WX = 5 -> clip the first 2 window pixels
                                   // LCD on, window on, BG on, tile data 0x8000, both maps 0x9800.
    ppu.write_register(0xFF40, 0xB1);

    for _ in 0..2000 {
        ppu.tick(200);
        if ppu.take_frame_ready() {
            break;
        }
    }

    // Without clipping, x0 would show the window's pixel 0 (colour 3 -> shade 3).
    // With the (7 - 5) = 2 pixel clip, x0 shows window pixel 2 (colour 1 -> shade 1).
    assert_eq!(
        ppu.framebuffer()[0],
        1,
        "WX<7 drops the first (7-WX) window pixels"
    );
}

/// A disabled LCD does not advance the PPU at all.
#[test]
fn ppu_does_not_tick_when_lcd_off() {
    let mut ppu = Ppu::new(); // LCDC = 0 -> LCD off
    assert_eq!(ppu.tick(200), 0);
    assert_eq!(ppu.ly, 0);
}

/// #1 regression: the frame-ready flag is set once, at the transition into
/// VBlank (ly == 144), and is cleared when consumed.
#[test]
fn ppu_frame_ready_fires_once_at_vblank() {
    let mut ppu = enabled_ppu();
    assert!(!ppu.take_frame_ready(), "not ready before any ticking");

    let mut fired = 0;
    let mut ly_at_fire = 0xFF;
    for _ in 0..2000 {
        ppu.tick(200);
        if ppu.take_frame_ready() {
            fired += 1;
            ly_at_fire = ppu.ly;
            break;
        }
    }

    assert_eq!(fired, 1, "frame-ready fires exactly once per frame");
    assert_eq!(ly_at_fire, 144, "frame becomes ready as VBlank begins");
    assert!(!ppu.take_frame_ready());
}

/// The VBlank interrupt bit is requested when the PPU enters VBlank.
#[test]
fn ppu_requests_vblank_interrupt() {
    let mut ppu = enabled_ppu();
    let mut saw_vblank = false;
    for _ in 0..2000 {
        if ppu.tick(200) & 0x01 != 0 {
            saw_vblank = true;
            break;
        }
    }
    assert!(saw_vblank, "VBlank interrupt requested once per frame");
}

/// Draw one background tile and verify it lands in the framebuffer with the
/// palette applied. Tile 1 is a solid colour-3 tile; the tile map selects it
/// at the top-left, and BGP maps colour 3 -> shade 3.
#[test]
fn ppu_renders_background_tile() {
    let mut ppu = Ppu::new(); // LCD off: full VRAM access for setup

    // Tile 1 (at 0x8010): every row = 0xFF/0xFF -> colour index 3 for all 8px.
    for row in 0..8 {
        ppu.write_vram(0x8010 + row * 2, 0xFF);
        ppu.write_vram(0x8010 + row * 2 + 1, 0xFF);
    }
    // Tile map entry (0,0) at 0x9800 -> tile 1.
    ppu.write_vram(0x9800, 0x01);

    // BGP identity (colour n -> shade n), then enable LCD + BG, tile data 0x8000.
    ppu.write_register(0xFF47, 0xE4);
    ppu.write_register(0xFF40, 0x91);

    // Run one full frame so scanline 0 is rendered.
    for _ in 0..2000 {
        ppu.tick(200);
        if ppu.take_frame_ready() {
            break;
        }
    }

    let fb = ppu.framebuffer();
    // Top-left 8×8 block should be shade 3.
    assert_eq!(fb[0], 3);
    assert_eq!(fb[7], 3);
    assert_eq!(fb[160 * 7], 3);
    // Just outside the tile (tile map entry (1,0) is 0 -> blank tile) is shade 0.
    assert_eq!(fb[8], 0);
}

/// The pixel FIFO honours a palette change made *mid-scanline*: BGP is changed
/// partway through line 0, so the left of the line uses the old palette and the
/// right uses the new one. A scanline renderer (one register snapshot per line)
/// could not produce this split.
#[test]
fn ppu_bgp_change_mid_scanline_splits_the_line() {
    let mut ppu = Ppu::new();

    // Tile 0: every pixel is colour index 1 (low plane all 1s, high plane 0).
    for row in 0..8 {
        ppu.write_vram(0x8000 + row * 2, 0xFF);
        ppu.write_vram(0x8000 + row * 2 + 1, 0x00);
    }
    // Tile map is already all-zero -> tile 0 everywhere.
    ppu.write_register(0xFF47, 0x0C); // BGP: colour 1 -> shade 3
    ppu.write_register(0xFF40, 0x91); // LCD on, BG on, tile data 0x8000

    // Drive line 0 dot by dot; switch BGP partway through pixel output.
    for d in 0..300 {
        ppu.tick(1);
        if d == 180 {
            ppu.write_register(0xFF47, 0x04); // BGP: colour 1 -> shade 1
        }
    }

    let row0 = &ppu.framebuffer()[0..160];
    assert!(row0.contains(&3), "left of the line used the old palette (shade 3)");
    assert!(row0.contains(&1), "right of the line used the new palette (shade 1)");
}

/// A sprite (object) is drawn on top of the background.
#[test]
fn ppu_renders_sprite() {
    let mut ppu = Ppu::new();

    // Object tile 2 (0x8020): solid colour 3.
    for row in 0..8 {
        ppu.write_vram(0x8020 + row * 2, 0xFF);
        ppu.write_vram(0x8020 + row * 2 + 1, 0xFF);
    }
    // OAM entry 0: Y=16 (screen y 0), X=8 (screen x 0), tile 2, no attributes.
    ppu.write_oam(0xFE00, 16);
    ppu.write_oam(0xFE01, 8);
    ppu.write_oam(0xFE02, 2);
    ppu.write_oam(0xFE03, 0x00);

    ppu.write_register(0xFF48, 0xE4); // OBP0 identity
    ppu.write_register(0xFF40, 0x93); // LCD on, OBJ on, BG on

    for _ in 0..2000 {
        ppu.tick(200);
        if ppu.take_frame_ready() {
            break;
        }
    }

    let fb = ppu.framebuffer();
    assert_eq!(fb[0], 3, "sprite pixel drawn at (0,0)");
    assert_eq!(fb[7], 3, "sprite pixel drawn at (7,0)");
}
