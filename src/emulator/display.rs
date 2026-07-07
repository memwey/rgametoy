use crate::console::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use minifb::{Key, Scale, Window, WindowOptions};

/// Integer upscale factor. Each Game Boy pixel becomes a SCALE×SCALE block, so
/// the image stays crisp (nearest-neighbour) with the 10:9 aspect ratio intact.
pub const SCALE: usize = 4;

const WINDOW_WIDTH: usize = SCREEN_WIDTH * SCALE;
const WINDOW_HEIGHT: usize = SCREEN_HEIGHT * SCALE;

/// DMG grayscale-green palette: shade 0 is the lightest, shade 3 the darkest.
/// Mapping a shade to a colour is a frontend choice; the core only emits shades.
const PALETTE: [u32; 4] = [0xFFE0F8D0, 0xFF88C070, 0xFF346856, 0xFF081820];

/// The host window. Owns presentation only — the integer upscale and the
/// shade→colour mapping live here; raw key state is exposed for the input
/// module to map.
pub struct Display {
    window: Window,
    buffer: Vec<u32>,
}

impl Display {
    pub fn new() -> Display {
        // Scale::X1: we do our own integer scaling into a window-sized buffer,
        // which keeps the factor a plain integer (not tied to minifb's 2^n
        // Scale) and the pixels perfectly square.
        let options = WindowOptions {
            scale: Scale::X1,
            resize: false,
            ..WindowOptions::default()
        };

        let window = Window::new("rgametoy - ESC to exit", WINDOW_WIDTH, WINDOW_HEIGHT, options)
            .unwrap_or_else(|e| panic!("{}", e));

        Display {
            window,
            buffer: vec![PALETTE[0]; WINDOW_WIDTH * WINDOW_HEIGHT],
        }
    }

    /// Present a frame given as 160×144 shade values (0-3), upscaled SCALE×.
    pub fn present(&mut self, framebuffer: &[u8]) {
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                let color = PALETTE[(framebuffer[y * SCREEN_WIDTH + x] & 0x03) as usize];
                for dy in 0..SCALE {
                    let base = (y * SCALE + dy) * WINDOW_WIDTH + x * SCALE;
                    for dx in 0..SCALE {
                        self.buffer[base + dx] = color;
                    }
                }
            }
        }
        self.window
            .update_with_buffer(&self.buffer, WINDOW_WIDTH, WINDOW_HEIGHT)
            .unwrap();
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open() && !self.window.is_key_down(Key::Escape)
    }

    /// Raw host key state, for the input module to map to Game Boy buttons.
    pub fn is_key_down(&self, key: Key) -> bool {
        self.window.is_key_down(key)
    }
}

impl Default for Display {
    fn default() -> Self {
        Self::new()
    }
}
