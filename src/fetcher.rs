use crate::pixel::Pixel;

#[derive(PartialEq, Clone)]
pub enum FetcherState {
    GetTileId,
    GetTileDataLow,
    GetTileDataHigh,
    PushToFifo,
}

pub struct Fetcher {
    fetcher_state: FetcherState,
    tile_id: u8,
    tile_data_low: u8,
    tile_data_high: u8,
    bg_fifo: Vec<Pixel>,
    oam_fifo: Vec<Pixel>,
    dots: u8,
    current_pixel_x: u8,
}

impl Fetcher {
    pub fn new() -> Fetcher {
        Fetcher {
            fetcher_state: FetcherState::GetTileId,
            tile_id: 0,
            tile_data_low: 0,
            tile_data_high: 0,
            bg_fifo: Vec::new(),
            oam_fifo: Vec::new(),
            dots: 0,
            current_pixel_x: 0,
        }
    }

    pub fn tick(
        &mut self,
        vram: &[u8],
        lcdc: u8,
        scx: u8,
        scy: u8,
        ly: u8,
        bgp: u8,
    ) {
        self.dots += 1;
        if self.dots < 2 {
            return;
        }
        self.dots = 0;

        match self.fetcher_state {
            FetcherState::GetTileId => {
                let tile_map_base = if (lcdc & 0x08) != 0 { 0x9C00 } else { 0x9800 };
                let tile_x = (self.current_pixel_x + scx) / 8;
                let tile_y = (ly + scy) / 8;
                let tile_map_addr = tile_map_base + (tile_y as u16 * 32) + tile_x as u16;
                self.tile_id = vram[(tile_map_addr & 0x1FFF) as usize]; // Access vram directly

                self.fetcher_state = FetcherState::GetTileDataLow;
            }
            FetcherState::GetTileDataLow => {
                let tile_data_base = if (lcdc & 0x10) != 0 { 0x8000 } else { 0x8800 };
                let tile_row = (ly + scy) % 8;

                let tile_addr = if (lcdc & 0x10) != 0 {
                    tile_data_base + (self.tile_id as u16 * 16) + (tile_row as u16 * 2)
                } else {
                    (0x9000i32 + (self.tile_id as i8 as i32 * 16) + (tile_row as i32 * 2)) as u16
                };

                self.tile_data_low = vram[(tile_addr & 0x1FFF) as usize]; // Access vram directly
                self.fetcher_state = FetcherState::GetTileDataHigh;
            }
            FetcherState::GetTileDataHigh => {
                let tile_data_base = if (lcdc & 0x10) != 0 { 0x8000 } else { 0x8800 };
                let tile_row = (ly + scy) % 8;

                let tile_addr = if (lcdc & 0x10) != 0 {
                    tile_data_base + (self.tile_id as u16 * 16) + (tile_row as u16 * 2) + 1
                } else {
                    (0x9000i32 + (self.tile_id as i8 as i32 * 16) + (tile_row as i32 * 2) + 1) as u16
                };

                self.tile_data_high = vram[(tile_addr & 0x1FFF) as usize]; // Access vram directly
                self.fetcher_state = FetcherState::PushToFifo;
            }
            FetcherState::PushToFifo => {
                if self.bg_fifo.len() > 8 {
                    return;
                }
                for i in 0..8 {
                    let bit_low = (self.tile_data_low >> (7 - i)) & 0x01;
                    let bit_high = (self.tile_data_high >> (7 - i)) & 0x01;
                    let color_id = (bit_high << 1) | bit_low;
                    
                    let shade = (bgp >> (color_id * 2)) & 0x03;
                    self.bg_fifo.push(Pixel::new(shade));
                }
                self.fetcher_state = FetcherState::GetTileId;
            }
        }
    }

    pub fn get_bg_fifo(&mut self) -> &mut Vec<Pixel> {
        &mut self.bg_fifo
    }

    pub fn clear_fifos(&mut self) {
        self.bg_fifo.clear();
        self.oam_fifo.clear();
    }
}
