use crate::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use minifb::{Key, Scale, Window, WindowOptions};

/// DMG grayscale palette: shade 0 is the lightest, shade 3 the darkest.
const PALETTE: [u32; 4] = [0xFFE0F8D0, 0xFF88C070, 0xFF346856, 0xFF081820];

pub struct Display {
    window: Window,
    buffer: [u32; SCREEN_WIDTH * SCREEN_HEIGHT],
}

impl Display {
    pub fn new() -> Display {
        let options = WindowOptions {
            scale: Scale::X4,
            ..WindowOptions::default()
        };

        let mut window = Window::new("rgametoy - ESC to exit", SCREEN_WIDTH, SCREEN_HEIGHT, options)
            .unwrap_or_else(|e| panic!("{}", e));

        // Limit to ~60 fps.
        window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));

        Display {
            window,
            buffer: [PALETTE[0]; SCREEN_WIDTH * SCREEN_HEIGHT],
        }
    }

    /// Present a full frame given as 160×144 shade values (0-3).
    pub fn present(&mut self, framebuffer: &[u8]) {
        for (out, &shade) in self.buffer.iter_mut().zip(framebuffer.iter()) {
            *out = PALETTE[(shade & 0x03) as usize];
        }
        self.window
            .update_with_buffer(&self.buffer, SCREEN_WIDTH, SCREEN_HEIGHT)
            .unwrap();
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open() && !self.window.is_key_down(Key::Escape)
    }

    /// Read the host keyboard and return the raw Game Boy button state, using
    /// the Input/P1 convention where a cleared bit means "pressed".
    ///
    /// bit0 Right, bit1 Left, bit2 Up, bit3 Down, bit4 A, bit5 B, bit6 Select,
    /// bit7 Start.
    pub fn poll_buttons(&self) -> u8 {
        let mut state = 0xFFu8;
        let mut press = |key: Key, bit: u8| {
            if self.window.is_key_down(key) {
                state &= !bit;
            }
        };
        press(Key::Right, 0x01);
        press(Key::Left, 0x02);
        press(Key::Up, 0x04);
        press(Key::Down, 0x08);
        press(Key::Z, 0x10); // A
        press(Key::X, 0x20); // B
        press(Key::Backspace, 0x40); // Select
        press(Key::Enter, 0x80); // Start
        state
    }
}

impl Default for Display {
    fn default() -> Self {
        Self::new()
    }
}
