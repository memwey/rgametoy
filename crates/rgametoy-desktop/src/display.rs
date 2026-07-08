use rgametoy_core::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::palette;
use minifb::{Key, Scale, Window, WindowOptions};

/// Window upscale factor. minifb's backend (Metal on macOS) uploads a texture
/// the size of the buffer we hand it, then upscales it to the window on the GPU
/// with nearest-neighbour sampling. So we present the *native* 160×144 buffer
/// and let the GPU do the 4× scale — 16× less data to upload each frame than a
/// pre-scaled 640×576 buffer, and no CPU scaling loop. X4 → a crisp 640×576
/// window at the DMG's 10:9 aspect.
const SCALE: Scale = Scale::X4;

/// The host window. Owns presentation only — the shade→colour mapping lives
/// here (the GPU handles the upscale); raw key state is exposed for the input
/// module to map. The active palette (see [`palette`]) can be cycled at runtime.
pub struct Display {
    window: Window,
    /// Native 160×144 ARGB frame; minifb/the GPU upscales it to the window.
    buffer: Vec<u32>,
    /// Index into [`palette::PALETTES`], and its precomputed opaque-ARGB form.
    palette_idx: usize,
    argb: [u32; 4],
}

impl Display {
    pub fn new() -> Display {
        // Scale::X4 sizes the window to 160×144 × 4 = 640×576; the default
        // ScaleMode::Stretch fills it from our native buffer (exact 4×, so
        // nearest-neighbour stays pixel-perfect).
        let options = WindowOptions {
            scale: SCALE,
            resize: false,
            ..WindowOptions::default()
        };

        let window = Window::new("rgametoy", SCREEN_WIDTH, SCREEN_HEIGHT, options)
            .unwrap_or_else(|e| panic!("{}", e));

        let argb = palette::to_argb(&palette::PALETTES[0].1);
        Display {
            window,
            buffer: vec![argb[0]; SCREEN_WIDTH * SCREEN_HEIGHT],
            palette_idx: 0,
            argb,
        }
    }

    /// Present a frame given as 160×144 shade values (0-3). Maps each shade to
    /// its palette colour; the GPU upscales the native-resolution buffer.
    pub fn present(&mut self, framebuffer: &[u8]) {
        for (dst, &shade) in self.buffer.iter_mut().zip(framebuffer) {
            *dst = self.argb[(shade & 0x03) as usize];
        }
        self.window
            .update_with_buffer(&self.buffer, SCREEN_WIDTH, SCREEN_HEIGHT)
            .unwrap();
    }

    /// Advance to the next palette; returns its name.
    pub fn cycle_palette(&mut self) -> &'static str {
        self.palette_idx = (self.palette_idx + 1) % palette::PALETTES.len();
        self.argb = palette::to_argb(&palette::PALETTES[self.palette_idx].1);
        palette::PALETTES[self.palette_idx].0
    }

    /// The active palette's name and RGB colours (so a screenshot can match the
    /// window).
    pub fn palette_name(&self) -> &'static str {
        palette::PALETTES[self.palette_idx].0
    }

    pub fn palette(&self) -> &'static palette::Palette {
        &palette::PALETTES[self.palette_idx].1
    }

    /// Set the window title (used to show the fps / speed / palette).
    pub fn set_title(&mut self, title: &str) {
        self.window.set_title(title);
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
