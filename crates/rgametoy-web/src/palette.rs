//! DMG shade (0 lightest … 3 darkest) → RGBA colour palettes. The core only
//! emits shades; the frontend picks the palette. Cycled at runtime with the
//! `3` key; index 0 is the default.
//!
//! KEEP IN SYNC with `crates/rgametoy-desktop/src/palette.rs`. Any new
//! palette or order change must land in both crates — there's no
//! cross-crate mechanism that would catch a drift.

/// Four RGBA colours, one per shade (index 0 = lightest, 3 = darkest).
pub type Palette = [(u8, u8, u8, u8); 4];

/// Palette index → display name. Used by the status bar.
pub const PALETTES: &[(&str, Palette)] = &[
    (
        "DMG green",
        [
            (0xE0, 0xF8, 0xD0, 0xFF),
            (0x88, 0xC0, 0x70, 0xFF),
            (0x34, 0x68, 0x56, 0xFF),
            (0x08, 0x18, 0x20, 0xFF),
        ],
    ),
    (
        "Pocket gray",
        [
            (0xC4, 0xCF, 0xA1, 0xFF),
            (0x8B, 0x95, 0x6D, 0xFF),
            (0x4D, 0x53, 0x3C, 0xFF),
            (0x1F, 0x1F, 0x1F, 0xFF),
        ],
    ),
    (
        "Grayscale",
        [
            (0xFF, 0xFF, 0xFF, 0xFF),
            (0xAA, 0xAA, 0xAA, 0xFF),
            (0x55, 0x55, 0x55, 0xFF),
            (0x00, 0x00, 0x00, 0xFF),
        ],
    ),
    (
        "Amber",
        [
            (0xFF, 0xE4, 0xA8, 0xFF),
            (0xD8, 0x98, 0x38, 0xFF),
            (0x86, 0x4C, 0x12, 0xFF),
            (0x28, 0x14, 0x00, 0xFF),
        ],
    ),
    (
        "Ocean",
        [
            (0xE0, 0xF4, 0xFF, 0xFF),
            (0x60, 0xA8, 0xD8, 0xFF),
            (0x28, 0x58, 0x98, 0xFF),
            (0x08, 0x20, 0x48, 0xFF),
        ],
    ),
    (
        "Berry",
        [
            (0xFF, 0xE8, 0xF0, 0xFF),
            (0xE0, 0x78, 0xA0, 0xFF),
            (0x90, 0x30, 0x58, 0xFF),
            (0x28, 0x08, 0x20, 0xFF),
        ],
    ),
];

/// Opaque-RGBA form of the palette at `idx`, suitable for use as a 4-entry
/// lookup table when expanding a 160×144 framebuffer of 0..=3 shades into
/// a 160×144×4 RGBA buffer for `ImageData`.
pub fn rgba_table(idx: usize) -> [(u8, u8, u8, u8); 4] {
    PALETTES[idx.min(PALETTES.len() - 1)].1
}

/// Expand a 160×144 shade buffer (each byte 0..=3) into an RGBA buffer using
/// `table`. Writes exactly `160 * 144 * 4` bytes into `out`; the caller is
/// responsible for sizing it.
pub fn shade_to_rgba(shades: &[u8], table: [(u8, u8, u8, u8); 4], out: &mut [u8]) {
    debug_assert_eq!(shades.len() * 4, out.len());
    for (i, &s) in shades.iter().enumerate() {
        let (r, g, b, a) = table[(s & 0x03) as usize];
        out[i * 4 + 0] = r;
        out[i * 4 + 1] = g;
        out[i * 4 + 2] = b;
        out[i * 4 + 3] = a;
    }
}
