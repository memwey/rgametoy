use crate::pixel::Pixel;

const LCD_WIDTH: usize = 160;

pub struct Lcd {
    linebuffer: [Pixel; LCD_WIDTH],
    current_scanline: u8,
    current_x: u8,
    last_ppu_ly: u8, // Track the last seen PPU LY value
}

impl Lcd {
    pub fn new() -> Lcd {
        Lcd {
            linebuffer: [Pixel::new(0); LCD_WIDTH],
            current_scanline: 0,
            current_x: 0,
            last_ppu_ly: 0,
        }
    }

    pub fn receive_pixel(&mut self, pixel: Pixel, ppu_ly: u8) -> bool {
        // Check if PPU LY has changed, indicating a new scanline
        if ppu_ly != self.last_ppu_ly {
            // PPU LY has changed, we're starting a new scanline
            self.current_scanline = ppu_ly;
            self.current_x = 0;
            self.last_ppu_ly = ppu_ly;
        }
        
        // Store pixel in linebuffer if within bounds
        if (self.current_x as usize) < LCD_WIDTH {
            self.linebuffer[self.current_x as usize] = pixel;
            self.current_x += 1;
            
            // Return true if we've completed a full scanline
            if self.current_x as usize == LCD_WIDTH {
                self.current_x = 0; // Reset for next scanline
                self.last_ppu_ly = ppu_ly; // Update last seen LY
                return true;
            }
        }
        false
    }

    pub fn get_line_data(&self) -> &[Pixel] {
        &self.linebuffer
    }

    pub fn get_current_scanline(&self) -> u8 {
        self.current_scanline
    }
}
