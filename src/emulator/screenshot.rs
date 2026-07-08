//! Framebuffer → image, a frontend (presentation) concern: the core only emits
//! 160×144 shade values, and turning those into coloured pixels + a file is a
//! host choice, kept out of `console`. Both the interactive emulator (F2) and
//! the headless `screenshot` example encode through here, so the on-screen and
//! saved images share one palette and one encoder.
//!
//! The format is uncompressed 24-bit BMP: it needs no dependency (PNG would
//! need DEFLATE, which std lacks) and every viewer opens it. Images are saved
//! at native 1:1 resolution — pixel-exact, which is what a debug screenshot
//! wants.

use crate::console::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::emulator::paths::sanitize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// DMG grayscale-green palette, shade 0 (lightest) → 3 (darkest), as RGB. The
/// single source of truth for the shade→colour mapping; `display` derives its
/// window ARGB from this so the window and screenshots always match.
pub const DMG_PALETTE: [(u8, u8, u8); 4] = [
    (0xE0, 0xF8, 0xD0),
    (0x88, 0xC0, 0x70),
    (0x34, 0x68, 0x56),
    (0x08, 0x18, 0x20),
];

/// Encode a 160×144 shade framebuffer (values 0-3) as an uncompressed 24-bit
/// BMP. Pure — no I/O — so it is trivially testable and reusable headless.
pub fn encode_bmp(framebuffer: &[u8]) -> Vec<u8> {
    let (w, h) = (SCREEN_WIDTH, SCREEN_HEIGHT);
    let row_bytes = w * 3; // 480, already 4-byte aligned
    let pixel_data = row_bytes * h;

    let mut out = Vec::with_capacity(54 + pixel_data);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + pixel_data) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&54u32.to_le_bytes()); // pixel-data offset
    out.extend_from_slice(&40u32.to_le_bytes()); // DIB header size
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // no compression
    out.extend_from_slice(&(pixel_data as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes()); // 72 DPI x
    out.extend_from_slice(&2835u32.to_le_bytes()); // 72 DPI y
    out.extend_from_slice(&0u32.to_le_bytes()); // palette colours
    out.extend_from_slice(&0u32.to_le_bytes()); // important colours

    // BMP rows are bottom-up; pixels are stored B, G, R.
    for y in (0..h).rev() {
        for x in 0..w {
            let (r, g, b) = DMG_PALETTE[(framebuffer[y * w + x] & 0x03) as usize];
            out.extend_from_slice(&[b, g, r]);
        }
    }
    out
}

/// Save a screenshot into `dir` (created if missing) and return its path.
/// The file is `<name_hint>-<epoch_ms>.bmp`; `name_hint` is sanitised and, if
/// empty, falls back to `rgametoy`, so the file is always identifiable and
/// two captures never collide.
pub fn save(framebuffer: &[u8], dir: &Path, name_hint: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("{}-{}.bmp", sanitize(name_hint), stamp));
    std::fs::write(&path, encode_bmp(framebuffer))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_has_a_valid_header_and_pixel_size() {
        let fb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        let bmp = encode_bmp(&fb);
        assert_eq!(&bmp[0..2], b"BM", "BMP magic");
        assert_eq!(bmp.len(), 54 + SCREEN_WIDTH * SCREEN_HEIGHT * 3);
        // Total-size and pixel-offset header fields.
        assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()), bmp.len() as u32);
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 54);
    }

    #[test]
    fn shade_maps_through_the_palette_bottom_up() {
        // Set the top-left pixel to shade 3 (darkest); it lands in the last BMP
        // row (bottom-up) as B,G,R of DMG_PALETTE[3].
        let mut fb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        fb[0] = 3;
        let bmp = encode_bmp(&fb);
        let (r, g, b) = DMG_PALETTE[3];
        let last_row = 54 + (SCREEN_HEIGHT - 1) * SCREEN_WIDTH * 3;
        assert_eq!(&bmp[last_row..last_row + 3], &[b, g, r]);
    }

    #[test]
    fn save_creates_the_dir_and_names_the_file() {
        let dir = std::env::temp_dir().join(format!("rgametoy-shottest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fb = vec![1u8; SCREEN_WIDTH * SCREEN_HEIGHT];

        let path = save(&fb, &dir, "Test ROM!").expect("save");

        assert!(path.exists(), "screenshot written");
        assert_eq!(path.extension().and_then(|s| s.to_str()), Some("bmp"));
        let stem = path.file_stem().unwrap().to_string_lossy();
        assert!(stem.starts_with("Test_ROM-"), "sanitised title stem, got {stem:?}");
        assert_eq!(&std::fs::read(&path).unwrap()[0..2], b"BM");

        let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
    }
}
