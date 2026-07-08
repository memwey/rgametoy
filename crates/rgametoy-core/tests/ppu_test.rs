extern crate rgametoy_core;

mod common;
use common::{enabled_ppu, render_frame};
use rgametoy_core::ppu::Ppu;

// Behaviour-level PPU tests, driven only through the register / framebuffer
// interface a game would use — no reaching into internal state. The dot-precise
// timing tests that want the *internal* mode (which software can't read
// directly) live in the `debug_timing` module below: they observe it through
// the sanctioned debug tooling (`get_mode`, `--features debug`), not by peeking.

/// LY as software reads it (the FF44 register), so these tests need no
/// internal accessor.
fn ly(ppu: &Ppu) -> u8 {
    ppu.read_register(0xFF44)
}

/// The mode as software reads it — the STAT register's low two bits (which lag
/// the internal transition, exactly as hardware behaves).
fn stat_mode(ppu: &Ppu) -> u8 {
    ppu.read_register(0xFF41) & 0x03
}

/// On a normal line, STAT shows OAM scan (mode 2) then drawing (mode 3): mode 2
/// is visible for 80 dots (dots 4–83), and mode 3 appears at dot 84 — the 4-dot
/// lag past the internal dot-80 switch, which is exactly what software sees.
#[test]
fn ppu_stat_shows_oam_scan_then_drawing_on_a_normal_line() {
    let mut ppu = enabled_ppu();
    while ly(&ppu) != 2 {
        ppu.tick(1);
    }
    ppu.tick(4); // dot 4
    assert_eq!(stat_mode(&ppu), 2, "OAM scan (mode 2) visible early in the line");
    ppu.tick(79); // dot 83
    assert_eq!(stat_mode(&ppu), 2, "STAT still shows mode 2 at dot 83");
    ppu.tick(1); // dot 84
    assert_eq!(stat_mode(&ppu), 3, "drawing (mode 3) visible from dot 84");
}

/// The first line after enable skips the OAM scan — STAT shows mode 0, not the
/// mode 2 a normal line shows — and it is 4 dots short, so LY reaches 1 at
/// dot 452 (mooneye `lcdon_timing`, observed through LY + STAT).
#[test]
fn ppu_first_line_after_enable_skips_scan_and_is_short() {
    let mut ppu = enabled_ppu();
    assert_eq!(ly(&ppu), 0);

    ppu.tick(40); // mid-line on line 0
    assert_eq!(
        stat_mode(&ppu),
        0,
        "line 0 shows mode 0 (no OAM scan), unlike a normal line's mode 2"
    );

    ppu.tick(200);
    ppu.tick(200);
    ppu.tick(11); // dot 451
    assert_eq!(ly(&ppu), 0, "still on the short line 0 at dot 451");
    ppu.tick(1); // dot 452
    assert_eq!(ly(&ppu), 1, "line 0 ends 4 dots early, at dot 452");
}

/// Mode-3 *length* is an internal quantity — software only sees its shifted
/// STAT footprint. These tests assert the internal dots directly, observing the
/// mode through the debug tooling (`Ppu::get_mode`, `--features debug`) rather
/// than reaching into private state. Run with `cargo test --features debug`;
/// the observable consequences are also covered black-box by rom_suite
/// (mooneye `intr_2`) and the STAT-timeline test above.
#[cfg(feature = "debug")]
mod debug_timing {
    use rgametoy_core::ppu::{Ppu, PpuMode};

    /// Internal mode-3 length on line 2, via the debug-only mode accessor.
    fn mode3_len_on_line2(ppu: &mut Ppu) -> u32 {
        while ppu.read_register(0xFF44) != 2 {
            ppu.tick(1);
        }
        let (mut start, mut dot, mut len) = (0u32, 0u32, 0u32);
        while ppu.read_register(0xFF44) == 2 {
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

    fn mode3_len_with_sprites(sprite_xs: &[u8]) -> u32 {
        let mut ppu = Ppu::new();
        for (i, &x) in sprite_xs.iter().enumerate() {
            let base = 0xFE00 + i as u16 * 4;
            ppu.write_oam(base, 18); // Y=18 -> screen line 2
            ppu.write_oam(base + 1, x);
            ppu.write_oam(base + 2, 0);
            ppu.write_oam(base + 3, 0);
        }
        ppu.write_register(0xFF40, if sprite_xs.is_empty() { 0x91 } else { 0x93 });
        mode3_len_on_line2(&mut ppu)
    }

    /// Mode 3 grows by the SCX fine-scroll discard: 172 + (SCX & 7) dots.
    #[test]
    fn mode3_grows_with_scx_fine_scroll() {
        for scx in [0u8, 1, 3, 5, 7] {
            let mut ppu = Ppu::new();
            ppu.write_register(0xFF43, scx);
            ppu.write_register(0xFF40, 0x91);
            assert_eq!(
                mode3_len_on_line2(&mut ppu),
                172 + (scx & 7) as u32,
                "SCX={scx} fine scroll stretches mode 3"
            );
        }
    }

    /// Stacked sprites at the same X pay the fetch-abort once: the first X=0
    /// sprite adds 11 dots, each further one only 6.
    #[test]
    fn stacked_sprite_penalty_aggregates() {
        assert_eq!(mode3_len_with_sprites(&[]), 172, "baseline mode 3 is 172 dots");
        assert_eq!(mode3_len_with_sprites(&[0]), 183, "one X=0 sprite adds the full 11");
        assert_eq!(mode3_len_with_sprites(&[0, 0]), 189, "a second stacked adds 6");
        assert_eq!(mode3_len_with_sprites(&[0, 0, 0]), 195, "a third stacked adds 6");
    }

    /// At most 10 sprites are selected per line, so an 11th stacked one adds no
    /// further stretch.
    #[test]
    fn at_most_10_sprites_stretch_mode3() {
        let ten = mode3_len_with_sprites(&[0; 10]);
        let eleven = mode3_len_with_sprites(&[0; 11]);
        assert_eq!(ten, 172 + 11 + 6 * 9, "10 stacked sprites: 11 + 6*9");
        assert_eq!(eleven, ten, "the 11th sprite is dropped (10-per-line cap)");
    }
}

/// The LY==LYC coincidence sets STAT bit 2 and, with the source enabled, raises
/// a STAT interrupt on the rising edge (mooneye `intr_1_2_timing`, `stat_lyc`).
#[test]
fn ppu_lyc_coincidence_sets_stat_bit_and_interrupts() {
    let mut ppu = enabled_ppu();
    ppu.write_register(0xFF45, 5); // LYC = 5
    ppu.write_register(0xFF41, 0x40); // enable the LYC=LY STAT source
    while ly(&ppu) != 5 {
        ppu.tick(1);
    }
    let mut saw_int = false;
    for _ in 0..8 {
        // The coincidence latches a few dots into the line.
        if ppu.tick(1) & 0x02 != 0 {
            saw_int = true;
        }
    }
    assert_eq!(
        ppu.read_register(0xFF41) & 0x04,
        0x04,
        "coincidence bit set while LY == LYC"
    );
    assert!(saw_int, "a STAT interrupt fired on the LY==LYC rising edge");
}

/// OAM is locked during modes 2 and 3; VRAM is locked only during mode 3.
#[test]
fn ppu_locks_oam_in_scan_and_draw_and_vram_in_draw() {
    let mut ppu = Ppu::new(); // LCD off: free access to seed sentinels
    ppu.write_vram(0x8000, 0x42);
    ppu.write_oam(0xFE00, 0x42);
    ppu.write_register(0xFF40, 0x80); // LCD on

    while ly(&ppu) != 2 {
        ppu.tick(1);
    }
    ppu.tick(40); // mode 2 (OAM scan)
    assert_eq!(ppu.read_oam(0xFE00), 0xFF, "OAM locked during scan");
    assert_eq!(ppu.read_vram(0x8000), 0x42, "VRAM free during scan");

    ppu.tick(110); // dot 150: mode 3 (drawing)
    assert_eq!(ppu.read_oam(0xFE00), 0xFF, "OAM locked during drawing");
    assert_eq!(ppu.read_vram(0x8000), 0xFF, "VRAM locked during drawing");

    ppu.tick(250); // dot 400: mode 0 (HBlank)
    assert_eq!(ppu.read_oam(0xFE00), 0x42, "OAM free in HBlank");
    assert_eq!(ppu.read_vram(0x8000), 0x42, "VRAM free in HBlank");
}

/// When two opaque sprites overlap, the one with the lower OAM index wins on
/// DMG (drawn first, and later sprites fill only still-transparent pixels).
#[test]
fn ppu_lower_oam_index_wins_on_sprite_overlap() {
    let mut ppu = Ppu::new();
    // Tile 2 = solid colour 1 (low plane 1s), tile 3 = solid colour 2 (high plane 1s).
    for row in 0..8 {
        ppu.write_vram(0x8020 + row * 2, 0xFF);
        ppu.write_vram(0x8020 + row * 2 + 1, 0x00);
        ppu.write_vram(0x8030 + row * 2, 0x00);
        ppu.write_vram(0x8030 + row * 2 + 1, 0xFF);
    }
    // Both sprites at the same spot (screen 0,0). OAM 0 uses tile 2, OAM 1 tile 3.
    ppu.write_oam(0xFE00, 16);
    ppu.write_oam(0xFE01, 8);
    ppu.write_oam(0xFE02, 2);
    ppu.write_oam(0xFE03, 0x00);
    ppu.write_oam(0xFE04, 16);
    ppu.write_oam(0xFE05, 8);
    ppu.write_oam(0xFE06, 3);
    ppu.write_oam(0xFE07, 0x00);
    ppu.write_register(0xFF48, 0xE4); // OBP0 identity
    ppu.write_register(0xFF40, 0x93);

    render_frame(&mut ppu);
    assert_eq!(
        ppu.framebuffer()[0],
        1,
        "sprite 0 (lower OAM index, colour 1) wins over sprite 1"
    );
}

/// A sprite with attribute bit 4 set uses OBP1 instead of OBP0.
#[test]
fn ppu_sprite_uses_obp1_when_attr_bit4_set() {
    let mut ppu = Ppu::new();
    for row in 0..8 {
        ppu.write_vram(0x8020 + row * 2, 0xFF); // tile 2: solid colour 3
        ppu.write_vram(0x8020 + row * 2 + 1, 0xFF);
    }
    ppu.write_oam(0xFE00, 16);
    ppu.write_oam(0xFE01, 8);
    ppu.write_oam(0xFE02, 2);
    ppu.write_oam(0xFE03, 0x10); // attr bit 4 -> OBP1
    ppu.write_register(0xFF48, 0xFF); // OBP0: colour 3 -> shade 3
    ppu.write_register(0xFF49, 0x40); // OBP1: colour 3 -> shade 1
    ppu.write_register(0xFF40, 0x93);

    render_frame(&mut ppu);
    assert_eq!(ppu.framebuffer()[0], 1, "sprite honoured OBP1, not OBP0");
}

/// LCDC bit 0 clear disables the background on DMG: it reads as colour 0
/// regardless of the tile data underneath.
#[test]
fn ppu_bg_disabled_forces_colour_0() {
    let mut ppu = Ppu::new();
    for row in 0..8 {
        ppu.write_vram(0x8010 + row * 2, 0xFF); // tile 1: solid colour 3
        ppu.write_vram(0x8010 + row * 2 + 1, 0xFF);
    }
    ppu.write_vram(0x9800, 0x01); // map (0,0) -> tile 1
    ppu.write_register(0xFF47, 0xE4); // BGP identity
    ppu.write_register(0xFF40, 0x90); // LCD on, tile data 0x8000, BG OFF (bit 0 = 0)

    render_frame(&mut ppu);
    assert_eq!(
        ppu.framebuffer()[0],
        0,
        "BG disabled blanks to colour 0 -> shade 0"
    );
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

    render_frame(&mut ppu);

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
    assert_eq!(ly(&ppu), 0);
}

/// The frame-ready flag is set once, at the transition into VBlank (ly == 144),
/// and is cleared when consumed.
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
            ly_at_fire = ly(&ppu);
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

    render_frame(&mut ppu);

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

    render_frame(&mut ppu);

    let fb = ppu.framebuffer();
    assert_eq!(fb[0], 3, "sprite pixel drawn at (0,0)");
    assert_eq!(fb[7], 3, "sprite pixel drawn at (7,0)");
}
