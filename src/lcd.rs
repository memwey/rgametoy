use crate::pixel::Pixel;

const LCD_WIDTH: usize = 160;
const LCD_HEIGHT: usize = 144;
const FRAMEBUFFER_SIZE: usize = LCD_WIDTH * LCD_HEIGHT; // Shades

pub struct Lcd {
    framebuffer: [Pixel; FRAMEBUFFER_SIZE],
}

impl Lcd {
    pub fn new() -> Lcd {
        Lcd {
            framebuffer: [Pixel::new(0); FRAMEBUFFER_SIZE],
        }
    }

    pub fn receive_pixel(&mut self, x: u8, y: u8, pixel: Pixel) {
        let index = (y as usize) * LCD_WIDTH + (x as usize);
        self.framebuffer[index] = pixel;
    }

    pub fn get_frame_data(&self) -> &[Pixel] {
        &self.framebuffer
    }
}
