const LCD_WIDTH: usize = 160;
const LCD_HEIGHT: usize = 144;
const FRAMEBUFFER_SIZE: usize = LCD_WIDTH * LCD_HEIGHT * 4; // RGBA

pub struct Lcd {
    framebuffer: [u8; FRAMEBUFFER_SIZE],
}

impl Lcd {
    pub fn new() -> Lcd {
        Lcd {
            framebuffer: [0; FRAMEBUFFER_SIZE],
        }
    }

    pub fn receive_pixel(&mut self, x: u8, y: u8, r: u8, g: u8, b: u8, a: u8) {
        let index = ((y as usize) * LCD_WIDTH + (x as usize)) * 4;
        self.framebuffer[index] = r;
        self.framebuffer[index + 1] = g;
        self.framebuffer[index + 2] = b;
        self.framebuffer[index + 3] = a;
    }

    pub fn get_frame_data(&self) -> &[u8] {
        &self.framebuffer
    }
}
