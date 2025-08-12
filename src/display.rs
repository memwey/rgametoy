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

    pub fn receive_scanline(&mut self, ly: u8, pixels: &[u8]) {
        let start_index = (ly as usize) * LCD_WIDTH;
        for x in 0..LCD_WIDTH {
            let pixel_index_rgba = x * 4;
            let r = pixels[pixel_index_rgba] as u32;
            let g = pixels[pixel_index_rgba + 1] as u32;
            let b = pixels[pixel_index_rgba + 2] as u32;
            let a = pixels[pixel_index_rgba + 3] as u32;
            self.framebuffer[start_index + x] = (a << 24) | (r << 16) | (g << 8) | b;
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
