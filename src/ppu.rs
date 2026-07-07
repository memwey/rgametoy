//! Picture Processing Unit.
//!
//! The PPU runs a dot-accurate mode state machine (OAM scan → drawing →
//! HBlank, then VBlank) that drives LY, the LYC compare, STAT mode bits and
//! the VBlank / STAT interrupts. Visible lines are drawn with a scanline
//! renderer (background, window and sprites) into an internal framebuffer of
//! shade values, which the display blits to the window.

const VRAM_SIZE: usize = 0x2000; // 8 KB
const OAM_SIZE: usize = 0xA0; // 160 bytes (0xFE00-0xFE9F)
pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;

// Dot counts for each phase of a scanline.
const OAM_DOTS: u16 = 80;
const DRAW_DOTS: u16 = 172;
const LINE_DOTS: u16 = 456;

// Interrupt request bits (matches the IF register layout).
const INT_VBLANK: u8 = 0x01;
const INT_LCDSTAT: u8 = 0x02;

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum PpuMode {
    HBlank = 0,
    VBlank = 1,
    OamScan = 2,
    Drawing = 3,
}

pub struct Ppu {
    vram: [u8; VRAM_SIZE],
    oam: [u8; OAM_SIZE],
    framebuffer: [u8; SCREEN_WIDTH * SCREEN_HEIGHT],

    mode: PpuMode,
    dots: u16,
    pub ly: u8,
    /// Internal window line counter (only advances on lines the window draws).
    window_line: u8,
    /// Previous state of the (level-triggered) STAT interrupt line, for edge
    /// detection.
    stat_line: bool,
    frame_ready: bool,

    lcdc: u8, // 0xFF40 LCD Control
    stat: u8, // 0xFF41 LCD Status (only the interrupt-enable bits are stored)
    scy: u8,  // 0xFF42
    scx: u8,  // 0xFF43
    lyc: u8,  // 0xFF45
    dma: u8,  // 0xFF46
    bgp: u8,  // 0xFF47
    obp0: u8, // 0xFF48
    obp1: u8, // 0xFF49
    wy: u8,   // 0xFF4A
    wx: u8,   // 0xFF4B
}

impl Ppu {
    pub fn new() -> Ppu {
        Ppu {
            vram: [0; VRAM_SIZE],
            oam: [0; OAM_SIZE],
            framebuffer: [0; SCREEN_WIDTH * SCREEN_HEIGHT],
            mode: PpuMode::OamScan,
            dots: 0,
            ly: 0,
            window_line: 0,
            stat_line: false,
            frame_ready: false,
            lcdc: 0,
            stat: 0,
            scy: 0,
            scx: 0,
            lyc: 0,
            dma: 0,
            bgp: 0,
            obp0: 0,
            obp1: 0,
            wy: 0,
            wx: 0,
        }
    }

    fn lcd_enabled(&self) -> bool {
        self.lcdc & 0x80 != 0
    }

    pub fn get_mode(&self) -> PpuMode {
        self.mode
    }

    /// The rendered frame as 160×144 shade values (0 = lightest, 3 = darkest).
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    /// Returns `true` exactly once per completed frame (at the moment the PPU
    /// enters VBlank), clearing the flag so the next frame must set it again.
    pub fn take_frame_ready(&mut self) -> bool {
        let ready = self.frame_ready;
        self.frame_ready = false;
        ready
    }

    /// Advance the PPU by `t_cycles` dots. Returns the set of interrupts
    /// (VBlank / STAT) requested during this step, as IF-register bits.
    pub fn tick(&mut self, t_cycles: u8) -> u8 {
        if !self.lcd_enabled() {
            return 0;
        }

        let mut requested = 0u8;
        for _ in 0..t_cycles {
            self.dots += 1;
            match self.mode {
                PpuMode::OamScan => {
                    if self.dots >= OAM_DOTS {
                        self.mode = PpuMode::Drawing;
                    }
                }
                PpuMode::Drawing => {
                    if self.dots >= OAM_DOTS + DRAW_DOTS {
                        self.render_scanline();
                        self.mode = PpuMode::HBlank;
                    }
                }
                PpuMode::HBlank => {
                    if self.dots >= LINE_DOTS {
                        self.dots = 0;
                        self.ly += 1;
                        if self.ly == SCREEN_HEIGHT as u8 {
                            self.mode = PpuMode::VBlank;
                            self.window_line = 0;
                            self.frame_ready = true;
                            requested |= INT_VBLANK;
                        } else {
                            self.mode = PpuMode::OamScan;
                        }
                    }
                }
                PpuMode::VBlank => {
                    if self.dots >= LINE_DOTS {
                        self.dots = 0;
                        self.ly += 1;
                        if self.ly > 153 {
                            self.ly = 0;
                            self.mode = PpuMode::OamScan;
                        }
                    }
                }
            }
            self.update_stat_line(&mut requested);
        }
        requested
    }

    /// Level-triggered STAT interrupt source, per the current mode / LYC.
    fn stat_condition(&self) -> bool {
        (self.mode == PpuMode::HBlank && self.stat & 0x08 != 0)
            || (self.mode == PpuMode::VBlank && self.stat & 0x10 != 0)
            || (self.mode == PpuMode::OamScan && self.stat & 0x20 != 0)
            || (self.ly == self.lyc && self.stat & 0x40 != 0)
    }

    /// Request an LCDSTAT interrupt on the rising edge of the STAT line.
    fn update_stat_line(&mut self, requested: &mut u8) {
        let cond = self.stat_condition();
        if cond && !self.stat_line {
            *requested |= INT_LCDSTAT;
        }
        self.stat_line = cond;
    }

    // --- Scanline rendering -------------------------------------------------

    // The pixel index drives scroll arithmetic and writes two arrays while
    // calling `&self` tile helpers, so an iterator form doesn't fit here.
    #[allow(clippy::needless_range_loop)]
    fn render_scanline(&mut self) {
        let ly = self.ly;
        if ly as usize >= SCREEN_HEIGHT {
            return;
        }
        let row_base = ly as usize * SCREEN_WIDTH;
        // Background/window colour index (before palette) — needed for the
        // object-to-background priority check.
        let mut bg_color = [0u8; SCREEN_WIDTH];

        // --- Background ---
        if self.lcdc & 0x01 != 0 {
            let map_base: usize = if self.lcdc & 0x08 != 0 { 0x1C00 } else { 0x1800 };
            let signed = self.lcdc & 0x10 == 0;
            let y = ly.wrapping_add(self.scy);
            for x in 0..SCREEN_WIDTH {
                let bx = (x as u8).wrapping_add(self.scx);
                let tile_id = self.vram[map_base + (y / 8) as usize * 32 + (bx / 8) as usize];
                let color = self.tile_color(tile_id, signed, y % 8, bx % 8);
                bg_color[x] = color;
                self.framebuffer[row_base + x] = shade(self.bgp, color);
            }
        } else {
            for x in 0..SCREEN_WIDTH {
                self.framebuffer[row_base + x] = 0;
            }
        }

        // --- Window ---
        if self.lcdc & 0x20 != 0 && ly >= self.wy && self.wx <= 166 {
            let map_base: usize = if self.lcdc & 0x40 != 0 { 0x1C00 } else { 0x1800 };
            let signed = self.lcdc & 0x10 == 0;
            let wline = self.window_line;
            let mut drew = false;
            for x in 0..SCREEN_WIDTH {
                if (x as u16 + 7) < self.wx as u16 {
                    continue;
                }
                let win_x = (x as u16 + 7 - self.wx as u16) as u8;
                let tile_id = self.vram[map_base + (wline / 8) as usize * 32 + (win_x / 8) as usize];
                let color = self.tile_color(tile_id, signed, wline % 8, win_x % 8);
                bg_color[x] = color;
                self.framebuffer[row_base + x] = shade(self.bgp, color);
                drew = true;
            }
            if drew {
                self.window_line = self.window_line.wrapping_add(1);
            }
        }

        // --- Sprites ---
        if self.lcdc & 0x02 != 0 {
            self.render_sprites(ly, row_base, &bg_color);
        }
    }

    /// Decode one pixel of a background/window tile. `row`/`col` are 0..8.
    fn tile_color(&self, tile_id: u8, signed: bool, row: u8, col: u8) -> u8 {
        let tile_addr = if signed {
            (0x1000_i32 + (tile_id as i8 as i32) * 16) as usize
        } else {
            tile_id as usize * 16
        };
        let lo = self.vram[tile_addr + row as usize * 2];
        let hi = self.vram[tile_addr + row as usize * 2 + 1];
        let bit = 7 - col;
        ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1)
    }

    fn render_sprites(&mut self, ly: u8, row_base: usize, bg_color: &[u8; SCREEN_WIDTH]) {
        let height: i16 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };

        // Select up to 10 sprites overlapping this line, in OAM order.
        let mut visible: Vec<usize> = Vec::with_capacity(10);
        for i in 0..40 {
            let sy = self.oam[i * 4] as i16 - 16;
            if (ly as i16) >= sy && (ly as i16) < sy + height {
                visible.push(i);
                if visible.len() == 10 {
                    break;
                }
            }
        }

        // DMG priority: smaller X wins; ties broken by lower OAM index. Draw
        // from lowest to highest priority so the winner ends up on top.
        visible.sort_by(|&a, &b| {
            let xa = self.oam[a * 4 + 1];
            let xb = self.oam[b * 4 + 1];
            xb.cmp(&xa).then(b.cmp(&a))
        });

        for &i in &visible {
            let sy = self.oam[i * 4] as i16 - 16;
            let sx = self.oam[i * 4 + 1] as i16 - 8;
            let mut tile = self.oam[i * 4 + 2];
            let attr = self.oam[i * 4 + 3];
            let flip_x = attr & 0x20 != 0;
            let flip_y = attr & 0x40 != 0;
            let behind_bg = attr & 0x80 != 0;
            let palette = if attr & 0x10 != 0 { self.obp1 } else { self.obp0 };

            let mut row = (ly as i16 - sy) as u8;
            if flip_y {
                row = (height as u8) - 1 - row;
            }
            if height == 16 {
                tile &= 0xFE; // 8×16 objects ignore the low tile bit
            }
            let tile_addr = tile as usize * 16 + row as usize * 2;
            let lo = self.vram[tile_addr];
            let hi = self.vram[tile_addr + 1];

            for px in 0..8i16 {
                let x = sx + px;
                if x < 0 || x >= SCREEN_WIDTH as i16 {
                    continue;
                }
                let bit = if flip_x { px as u8 } else { 7 - px as u8 };
                let color = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                if color == 0 {
                    continue; // transparent
                }
                if behind_bg && bg_color[x as usize] != 0 {
                    continue; // background/window pixels 1-3 are in front
                }
                self.framebuffer[row_base + x as usize] = shade(palette, color);
            }
        }
    }

    // --- CPU-visible memory & registers ------------------------------------

    fn can_access_vram(&self) -> bool {
        !self.lcd_enabled() || self.mode != PpuMode::Drawing
    }

    fn can_access_oam(&self) -> bool {
        !self.lcd_enabled() || (self.mode != PpuMode::OamScan && self.mode != PpuMode::Drawing)
    }

    pub fn read_vram(&self, addr: u16) -> u8 {
        if self.can_access_vram() {
            self.vram[(addr & 0x1FFF) as usize]
        } else {
            0xFF
        }
    }

    pub fn write_vram(&mut self, addr: u16, value: u8) {
        if self.can_access_vram() {
            self.vram[(addr & 0x1FFF) as usize] = value;
        }
    }

    pub fn read_oam(&self, addr: u16) -> u8 {
        if self.can_access_oam() {
            self.oam[(addr - 0xFE00) as usize]
        } else {
            0xFF
        }
    }

    pub fn write_oam(&mut self, addr: u16, value: u8) {
        if self.can_access_oam() {
            self.oam[(addr - 0xFE00) as usize] = value;
        }
    }

    /// Direct OAM write used by the OAM DMA transfer (bypasses mode blocking).
    pub fn dma_write_oam(&mut self, index: usize, value: u8) {
        if index < OAM_SIZE {
            self.oam[index] = value;
        }
    }

    pub fn read_register(&self, addr: u16) -> u8 {
        match addr {
            0xFF40 => self.lcdc,
            0xFF41 => {
                // Bit 7 reads 1; bits 0-2 report live mode / LYC state.
                let lyc = if self.ly == self.lyc { 0x04 } else { 0x00 };
                0x80 | (self.stat & 0x78) | lyc | self.mode as u8
            }
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF46 => self.dma,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            _ => 0xFF,
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF40 => {
                let was_on = self.lcd_enabled();
                self.lcdc = value;
                let now_on = self.lcd_enabled();
                if was_on && !now_on {
                    // Turning the LCD off resets the state machine and blanks LY.
                    self.ly = 0;
                    self.dots = 0;
                    self.mode = PpuMode::HBlank;
                    self.window_line = 0;
                    self.stat_line = false;
                } else if !was_on && now_on {
                    self.ly = 0;
                    self.dots = 0;
                    self.mode = PpuMode::OamScan;
                    self.window_line = 0;
                }
            }
            0xFF41 => self.stat = value & 0x78, // only the interrupt-enable bits
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only
            0xFF45 => self.lyc = value,
            0xFF46 => self.dma = value, // the transfer itself is done by the bus
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => self.wy = value,
            0xFF4B => self.wx = value,
            _ => {}
        }
    }
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a 2-bit colour index through a palette register to a shade (0-3).
fn shade(palette: u8, color: u8) -> u8 {
    (palette >> (color * 2)) & 0x03
}
