//! Picture Processing Unit.
//!
//! The PPU runs a dot-accurate mode state machine (OAM scan → drawing →
//! HBlank, then VBlank) driving LY, LYC, STAT and the VBlank/STAT interrupts.
//! Mode 3 is a per-dot **pixel FIFO** pipeline: a background/window fetcher
//! fills the BG FIFO in the background while one pixel is shifted out per dot,
//! merged with sprite pixels, run through the palettes and written to the
//! framebuffer. Registers are sampled per dot, so mid-scanline changes take
//! effect, and the length of mode 3 is emergent (it stretches for the SCX
//! fine-scroll discard, sprites and the window).

use std::collections::VecDeque;

const VRAM_SIZE: usize = 0x2000; // 8 KB
const OAM_SIZE: usize = 0xA0; // 160 bytes (0xFE00-0xFE9F)
pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;

const OAM_DOTS: u16 = 80;
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

/// Background/window fetcher state (each step takes two dots).
#[derive(PartialEq, Clone, Copy)]
enum FetchState {
    Tile,
    DataLow,
    DataHigh,
    Push,
}

/// A pending sprite pixel in the object FIFO. The palette is resolved at output
/// time (not merge time) so mid-line OBP changes are honoured.
#[derive(Clone, Copy, Default)]
struct ObjPixel {
    color: u8, // 0 = transparent
    use_obp1: bool,
    priority: bool, // true = behind background colours 1-3
}

#[derive(Clone)]
pub struct Ppu {
    vram: [u8; VRAM_SIZE],
    oam: [u8; OAM_SIZE],
    framebuffer: [u8; SCREEN_WIDTH * SCREEN_HEIGHT],

    mode: PpuMode,
    dots: u16,
    pub ly: u8,
    window_line: u8,
    stat_line: bool,
    /// Latched LY==LYC coincidence. Updated only while the LCD runs (the
    /// comparison clock stops when it is off), so the STAT bit and its
    /// interrupt freeze across a power-off and restart on power-on.
    lyc_match: bool,
    frame_ready: bool,

    lcdc: u8, // 0xFF40
    stat: u8, // 0xFF41 (interrupt-enable bits only)
    scy: u8,  // 0xFF42
    scx: u8,  // 0xFF43
    lyc: u8,  // 0xFF45
    dma: u8,  // 0xFF46
    bgp: u8,  // 0xFF47
    obp0: u8, // 0xFF48
    obp1: u8, // 0xFF49
    wy: u8,   // 0xFF4A
    wx: u8,   // 0xFF4B

    // --- Mode 3 pixel pipeline ---
    draw_x: u8,
    fetch_state: FetchState,
    fetch_step: bool, // two-dot cadence
    fetch_x: u8,
    fetch_tile_id: u8,
    fetch_lo: u8,
    fetch_hi: u8,
    bg_fifo: VecDeque<u8>,
    obj_fifo: [ObjPixel; 8],
    discard: u8, // SCX fine-scroll pixels still to drop
    window_active: bool,
    wy_triggered: bool,
    line_sprites: [u8; 10], // OAM indices of sprites on this line
    line_sprite_count: u8,
    sprite_fetched: [bool; 10],
    sprite_delay: u8,       // dots left in an in-progress sprite fetch
    sprite_index: usize,    // line_sprites slot being fetched
    warmup: u8,             // fetcher startup stall at the start of mode 3
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
            lyc_match: false,
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
            draw_x: 0,
            fetch_state: FetchState::Tile,
            fetch_step: false,
            fetch_x: 0,
            fetch_tile_id: 0,
            fetch_lo: 0,
            fetch_hi: 0,
            bg_fifo: VecDeque::with_capacity(16),
            obj_fifo: [ObjPixel::default(); 8],
            discard: 0,
            window_active: false,
            wy_triggered: false,
            line_sprites: [0; 10],
            line_sprite_count: 0,
            sprite_fetched: [false; 10],
            sprite_delay: 0,
            sprite_index: 0,
            warmup: 0,
        }
    }

    fn lcd_enabled(&self) -> bool {
        self.lcdc & 0x80 != 0
    }

    pub fn get_mode(&self) -> PpuMode {
        self.mode
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    pub fn take_frame_ready(&mut self) -> bool {
        let ready = self.frame_ready;
        self.frame_ready = false;
        ready
    }

    /// Advance the PPU by `t_cycles` dots. Returns the interrupts (VBlank /
    /// STAT) requested during this step, as IF-register bits.
    pub fn tick(&mut self, t_cycles: u8) -> u8 {
        if !self.lcd_enabled() {
            return 0;
        }

        let mut requested = 0u8;
        for _ in 0..t_cycles {
            self.dots += 1;
            match self.mode {
                PpuMode::OamScan => {
                    if self.dots == OAM_DOTS {
                        self.start_drawing();
                        self.mode = PpuMode::Drawing;
                    }
                }
                PpuMode::Drawing => {
                    self.draw_dot();
                    if self.draw_x as usize >= SCREEN_WIDTH {
                        if self.window_active {
                            self.window_line = self.window_line.wrapping_add(1);
                        }
                        self.mode = PpuMode::HBlank;
                    }
                }
                PpuMode::HBlank => {
                    if self.dots >= LINE_DOTS {
                        self.dots = 0;
                        self.ly += 1;
                        if self.ly == SCREEN_HEIGHT as u8 {
                            self.mode = PpuMode::VBlank;
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
                            self.window_line = 0;
                            self.wy_triggered = false;
                            self.mode = PpuMode::OamScan;
                        }
                    }
                }
            }
            // The LY==LYC comparison clock runs only while the LCD is on.
            self.lyc_match = self.ly == self.lyc;
            self.update_stat_line(&mut requested);
        }
        requested
    }

    // --- STAT interrupt (level-triggered, rising-edge request) ---

    fn stat_condition(&self) -> bool {
        (self.mode == PpuMode::HBlank && self.stat & 0x08 != 0)
            || (self.mode == PpuMode::VBlank && self.stat & 0x10 != 0)
            || (self.mode == PpuMode::OamScan && self.stat & 0x20 != 0)
            // The mode-2 (OAM) source is also asserted at the start of VBlank
            // (line 144), so a STAT interrupt fires together with VBlank.
            || (self.ly == SCREEN_HEIGHT as u8 && self.stat & 0x20 != 0)
            || (self.lyc_match && self.stat & 0x40 != 0)
    }

    fn update_stat_line(&mut self, requested: &mut u8) {
        let cond = self.stat_condition();
        if cond && !self.stat_line {
            *requested |= INT_LCDSTAT;
        }
        self.stat_line = cond;
    }

    // --- Mode 3 pixel pipeline ---------------------------------------------

    fn start_drawing(&mut self) {
        if self.ly == self.wy {
            self.wy_triggered = true;
        }
        self.select_sprites();
        self.draw_x = 0;
        self.fetch_state = FetchState::Tile;
        self.fetch_step = false;
        self.fetch_x = 0;
        self.fetch_tile_id = 0;
        self.fetch_lo = 0;
        self.fetch_hi = 0;
        self.bg_fifo.clear();
        self.obj_fifo = [ObjPixel::default(); 8];
        self.discard = self.scx & 7;
        self.window_active = false;
        self.sprite_delay = 0;
        // The BG fetcher needs a fixed startup before the first pixel can be
        // pushed, so mode 3 is 172 dots at SCX 0 (not the 167 the bare FIFO
        // warmup yields). Model the missing dots as an explicit stall.
        self.warmup = 5;
    }

    /// Select up to 10 sprites overlapping this line, in OAM order.
    fn select_sprites(&mut self) {
        self.line_sprite_count = 0;
        self.sprite_fetched = [false; 10];
        let height = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        for i in 0..40u8 {
            let y = self.oam[i as usize * 4] as i16 - 16;
            if (self.ly as i16) >= y && (self.ly as i16) < y + height {
                self.line_sprites[self.line_sprite_count as usize] = i;
                self.line_sprite_count += 1;
                if self.line_sprite_count == 10 {
                    break;
                }
            }
        }
    }

    /// One dot of mode 3: advance the fetcher, then discard / start the window
    /// / fetch a sprite / output a pixel as appropriate.
    fn draw_dot(&mut self) {
        // Fixed fetcher startup: mode 3 stalls before any pixel work.
        if self.warmup > 0 {
            self.warmup -= 1;
            return;
        }

        // A sprite fetch pauses the background pipeline.
        if self.sprite_delay > 0 {
            self.sprite_delay -= 1;
            if self.sprite_delay == 0 {
                self.merge_sprite();
            }
            return;
        }

        self.advance_fetcher();

        if self.bg_fifo.is_empty() {
            return;
        }

        // SCX fine scroll: drop the first (SCX % 8) pixels of the line.
        if self.discard > 0 {
            self.bg_fifo.pop_front();
            self.discard -= 1;
            return;
        }

        // Switch the fetcher to the window when it is reached.
        if !self.window_active && self.window_should_start() {
            self.window_active = true;
            self.bg_fifo.clear();
            self.fetch_state = FetchState::Tile;
            self.fetch_step = false;
            self.fetch_x = 0;
            return;
        }

        // A sprite at this X pauses output while it is fetched.
        if self.lcdc & 0x02 != 0 {
            if let Some(slot) = self.sprite_at(self.draw_x) {
                self.sprite_index = slot;
                self.sprite_fetched[slot] = true;
                self.sprite_delay = 6;
                return;
            }
        }

        // Output one pixel.
        let bg = self.bg_fifo.pop_front().unwrap();
        let obj = self.obj_fifo[0];
        for i in 0..7 {
            self.obj_fifo[i] = self.obj_fifo[i + 1];
        }
        self.obj_fifo[7] = ObjPixel::default();

        let px = self.mix(bg, obj);
        self.framebuffer[self.ly as usize * SCREEN_WIDTH + self.draw_x as usize] = px;
        self.draw_x += 1;
    }

    fn window_should_start(&self) -> bool {
        self.lcdc & 0x20 != 0
            && self.wy_triggered
            && self.wx <= 166
            && self.draw_x as u16 + 7 >= self.wx as u16
    }

    /// Advance the background/window fetcher one two-dot step.
    fn advance_fetcher(&mut self) {
        self.fetch_step = !self.fetch_step;
        if self.fetch_step {
            return; // act on every second dot
        }

        match self.fetch_state {
            FetchState::Tile => {
                let (map_base, tile_x, tile_y) = if self.window_active {
                    let map = if self.lcdc & 0x40 != 0 { 0x1C00 } else { 0x1800 };
                    (map, self.fetch_x, self.window_line / 8)
                } else {
                    let map = if self.lcdc & 0x08 != 0 { 0x1C00 } else { 0x1800 };
                    let tx = (self.scx / 8).wrapping_add(self.fetch_x) & 0x1F;
                    let ty = self.ly.wrapping_add(self.scy) / 8;
                    (map, tx, ty)
                };
                self.fetch_tile_id = self.vram[map_base + tile_y as usize * 32 + tile_x as usize];
                self.fetch_state = FetchState::DataLow;
            }
            FetchState::DataLow => {
                self.fetch_lo = self.vram[self.tile_data_addr()];
                self.fetch_state = FetchState::DataHigh;
            }
            FetchState::DataHigh => {
                self.fetch_hi = self.vram[self.tile_data_addr() + 1];
                self.fetch_state = FetchState::Push;
            }
            FetchState::Push => {
                if self.bg_fifo.is_empty() {
                    for i in 0..8 {
                        let bit = 7 - i;
                        let color = ((self.fetch_hi >> bit) & 1) << 1 | ((self.fetch_lo >> bit) & 1);
                        self.bg_fifo.push_back(color);
                    }
                    self.fetch_x = self.fetch_x.wrapping_add(1);
                    self.fetch_state = FetchState::Tile;
                }
            }
        }
    }

    fn tile_data_addr(&self) -> usize {
        let row = if self.window_active {
            self.window_line & 7
        } else {
            self.ly.wrapping_add(self.scy) & 7
        };
        let base = if self.lcdc & 0x10 != 0 {
            self.fetch_tile_id as usize * 16
        } else {
            (0x1000_i32 + (self.fetch_tile_id as i8 as i32) * 16) as usize
        };
        base + row as usize * 2
    }

    /// First not-yet-fetched selected sprite whose left edge is at `draw_x`.
    fn sprite_at(&self, draw_x: u8) -> Option<usize> {
        for i in 0..self.line_sprite_count as usize {
            if self.sprite_fetched[i] {
                continue;
            }
            let x = self.oam[self.line_sprites[i] as usize * 4 + 1];
            let trigger = x.saturating_sub(8);
            if trigger == draw_x {
                return Some(i);
            }
        }
        None
    }

    /// Merge the fetched sprite's 8 pixels into the object FIFO, filling only
    /// transparent slots so earlier (higher-priority) sprites win.
    fn merge_sprite(&mut self) {
        let base = self.line_sprites[self.sprite_index] as usize * 4;
        let y = self.oam[base] as i16 - 16;
        let x = self.oam[base + 1] as i16 - 8;
        let mut tile = self.oam[base + 2];
        let attr = self.oam[base + 3];
        let flip_x = attr & 0x20 != 0;
        let flip_y = attr & 0x40 != 0;
        let use_obp1 = attr & 0x10 != 0;
        let priority = attr & 0x80 != 0;
        let height: i16 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };

        let mut row = self.ly as i16 - y;
        if flip_y {
            row = height - 1 - row;
        }
        if height == 16 {
            tile &= 0xFE;
        }
        let addr = tile as usize * 16 + row as usize * 2;
        let lo = self.vram[addr];
        let hi = self.vram[addr + 1];

        for p in 0..8i16 {
            let slot = x + p - self.draw_x as i16;
            if !(0..8).contains(&slot) {
                continue;
            }
            let bit = if flip_x { p as u8 } else { 7 - p as u8 };
            let color = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
            if color != 0 && self.obj_fifo[slot as usize].color == 0 {
                self.obj_fifo[slot as usize] = ObjPixel {
                    color,
                    use_obp1,
                    priority,
                };
            }
        }
    }

    /// Mix a background colour and an object pixel to a shade, sampling the
    /// palettes and LCDC live (so mid-line changes apply).
    fn mix(&self, bg_color: u8, obj: ObjPixel) -> u8 {
        // LCDC bit 0 clear blanks the background on DMG (colour 0).
        let bg = if self.lcdc & 0x01 != 0 { bg_color } else { 0 };
        if obj.color != 0 && (!obj.priority || bg == 0) {
            let palette = if obj.use_obp1 { self.obp1 } else { self.obp0 };
            shade(palette, obj.color)
        } else {
            shade(self.bgp, bg)
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
                let lyc = if self.lyc_match { 0x04 } else { 0x00 };
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
                    self.ly = 0;
                    self.dots = 0;
                    self.mode = PpuMode::HBlank;
                    self.window_line = 0;
                    self.stat_line = false;
                    // The coincidence flag is retained while the LCD is off.
                } else if !was_on && now_on {
                    self.ly = 0;
                    self.dots = 0;
                    self.mode = PpuMode::OamScan;
                    self.window_line = 0;
                    // The comparison clock restarts on power-on.
                    self.lyc_match = self.ly == self.lyc;
                }
            }
            0xFF41 => self.stat = value & 0x78,
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {}
            0xFF45 => self.lyc = value,
            0xFF46 => self.dma = value,
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
