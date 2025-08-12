use crate::pixel::Pixel;
use minifb::{Window, WindowOptions};

const LCD_WIDTH: usize = 160;
const LCD_HEIGHT: usize = 144;

pub struct Display {
    window: Window,
    framebuffer: [u32; LCD_WIDTH * LCD_HEIGHT],
}

impl Display {
    pub fn new() -> Display {
        let mut window = Window::new(
            "rgametoy - ESC to exit",
            LCD_WIDTH,
            LCD_HEIGHT,
            WindowOptions::default(),
        )
        .unwrap_or_else(|e| {
            panic!("{}", e);
        });

        // Limit to 60 fps
        window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));

        Display {
            window,
            framebuffer: [0; LCD_WIDTH * LCD_HEIGHT],
        }
    }

    pub fn receive_scanline(&mut self, ly: u8, pixels: &[Pixel]) {
        // Ensure ly is within valid range
        if ly >= LCD_HEIGHT as u8 {
            return;
        }
        
        let start_index = (ly as usize) * LCD_WIDTH;
        // Define a simple grayscale palette
        let palette: [u32; 4] = [
            0xFF000000, // Shade 0: Black
            0xFF555555, // Shade 1: Dark Gray
            0xFFAAAAAA, // Shade 2: Light Gray
            0xFFFFFFFF, // Shade 3: White
        ];

        for x in 0..LCD_WIDTH {
            let shade = pixels[x].shade;
            self.framebuffer[start_index + x] = palette[shade as usize];
        }
    }

    pub fn present_frame(&mut self) {
        self.window
            .update_with_buffer(&self.framebuffer, LCD_WIDTH, LCD_HEIGHT)
            .unwrap();
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }
}
