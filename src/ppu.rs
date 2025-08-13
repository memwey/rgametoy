use crate::pixel::Pixel;

const VRAM_SIZE: usize = 0x2000; // 8KB
const OAM_SIZE: usize = 0xA0; // 160 bytes (0xFE00-0xFE9F)

use crate::fetcher::{Fetcher, FetcherState}; // New import

#[derive(PartialEq, Clone)]
pub enum PpuMode {
    HBlank,
    VBlank,
    OamScan,
    DrawingPixels,
}

pub struct Ppu {
    vram: [u8; VRAM_SIZE],
    oam: [u8; OAM_SIZE],
    mode: PpuMode,
    cycles_in_mode: u16,
    pub ly: u8, // LCD Y-coordinate (current scanline)
    pub current_pixel_x: u8, // Current pixel X-coordinate on the scanline
    lcdc: u8, // LCD Control Register (0xFF40)
    stat: u8, // LCDC Status Register (0xFF41)
    scy: u8,  // Scroll Y (0xFF42)
    scx: u8,  // Scroll X (0xFF43)
    lyc: u8,  // LY Compare (0xFF45)
    dma: u8,  // DMA Transfer and Start Address (0xFF46)
    bgp: u8,  // BG Palette Data (0xFF47)
    obp0: u8, // Object Palette 0 Data (0xFF48)
    obp1: u8, // Object Palette 1 Data (0xFF49)
    wy: u8,   // Window Y Position (0xFF4A)
    wx: u8,   // Window X Position (0xFF4B)

    // Fetcher instance
    fetcher: Fetcher,
}

impl Ppu {
    pub fn new() -> Ppu {
        Ppu {
            vram: [0; VRAM_SIZE],
            oam: [0; OAM_SIZE],
            mode: PpuMode::OamScan, // Initial mode
            cycles_in_mode: 0,
            ly: 0,
            current_pixel_x: 0,
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
            fetcher: Fetcher::new(), // Initialize Fetcher
        }
    }

    // PPU tick method (called by Console)
    pub fn tick(&mut self, cycles: u8) -> Vec<(u8, Pixel)> {
        let mut output_pixels: Vec<(u8, Pixel)> = Vec::new(); // (x, Pixel)
        self.cycles_in_mode += cycles as u16;

        match self.mode {
            PpuMode::OamScan => {
                if self.cycles_in_mode >= 80 { // Mode 2 lasts 80 cycles
                    self.mode = PpuMode::DrawingPixels;
                    self.cycles_in_mode = 0;
                    self.current_pixel_x = 0;
                }
            }
            PpuMode::DrawingPixels => {
                // In DrawingPixels mode, we process pixels using the fetcher
                // Each cycle, we advance the fetcher state
                for _ in 0..cycles {
                    if self.current_pixel_x < 160 {
                        // Only process fetcher if FIFO is empty or needs more pixels
                        if self.fetcher.get_bg_fifo().is_empty() {
                            self.fetcher.fetch_pixel_data(
                                &self.vram,
                                self.lcdc,
                                self.scx,
                                self.scy,
                                self.ly,
                                self.current_pixel_x,
                                self.bgp,
                            );
                        }

                        // If FIFO has pixels, pop one and output it
                        if !self.fetcher.get_bg_fifo().is_empty() {
                            let pixel = self.fetcher.get_bg_fifo().remove(0); // Pop from front
                            output_pixels.push((self.current_pixel_x, pixel));
                            self.current_pixel_x += 1;
                        }
                    }
                }

                if self.current_pixel_x >= 160 { // Finished drawing scanline
                    self.mode = PpuMode::HBlank;
                    self.cycles_in_mode = 0;
                    // Clear FIFOs for next scanline
                    self.fetcher.clear_fifos();
                }
            }
            PpuMode::HBlank => {
                if self.cycles_in_mode >= 204 { // Mode 0 lasts 204 cycles
                    self.cycles_in_mode = 0;
                    self.ly += 1;

                    if self.ly == 144 { // End of visible screen, enter VBlank
                        self.mode = PpuMode::VBlank;
                        // TODO: Request VBlank interrupt
                    } else { // Next scanline, go back to OAM Scan
                        self.mode = PpuMode::OamScan;
                    }
                }
            }
            PpuMode::VBlank => {
                if self.cycles_in_mode >= 456 { // VBlank scanline lasts 456 cycles
                    self.cycles_in_mode = 0;
                    self.ly += 1;

                    if self.ly > 153 { // End of VBlank period, reset to scanline 0
                        self.ly = 0;
                        self.mode = PpuMode::OamScan;
                    }
                }
            }
        }
        output_pixels
    }

    

    // Check if CPU can access VRAM (blocked during Mode 3)
    pub fn can_access_vram(&self) -> bool {
        self.mode != PpuMode::DrawingPixels
    }

    // Check if CPU can access OAM (blocked during Mode 2 and Mode 3)
    pub fn can_access_oam(&self) -> bool {
        self.mode != PpuMode::OamScan && self.mode != PpuMode::DrawingPixels
    }

    pub fn read_vram(&self, addr: u16) -> u8 {
        if self.can_access_vram() {
            self.vram[(addr & 0x1FFF) as usize]
        } else {
            0xFF // Return garbage if access is blocked
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
            0xFF // Return garbage if access is blocked
        }
    }

    pub fn write_oam(&mut self, addr: u16, value: u8) {
        if self.can_access_oam() {
            self.oam[(addr - 0xFE00) as usize] = value;
        }
    }

    pub fn read_register(&self, addr: u16) -> u8 {
        match addr {
            0xFF40 => self.lcdc,
            0xFF41 => self.stat,
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
            _ => 0xFF, // Should not happen
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF40 => self.lcdc = value,
            0xFF41 => self.stat = value,
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => self.ly = value, // LY is read-only, but some games write to it
            0xFF45 => self.lyc = value,
            0xFF46 => self.dma = value, // DMA will need special handling
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => self.wy = value,
            0xFF4B => self.wx = value,
            _ => { /* Should not happen */ }
        }
    }
    
    pub fn get_mode(&self) -> PpuMode {
        self.mode.clone()
    }
}