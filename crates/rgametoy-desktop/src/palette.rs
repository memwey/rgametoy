//! DMG shade → colour palettes. Mapping a shade (0 lightest … 3 darkest) to a
//! colour is a pure frontend choice — the `console` core only emits shades — so
//! the window and screenshots pick one of these. The first is the classic DMG
//! green; the rest tint the four shades into "some colour" (the same trick the
//! Game Boy Color used to colourise monochrome games). Cycled at runtime with
//! the "3" key; index 0 is the default.

/// Four RGB colours, one per shade (index 0 = lightest, 3 = darkest).
pub type Palette = [(u8, u8, u8); 4];

pub const PALETTES: &[(&str, Palette)] = &[
    (
        "DMG green",
        [
            (0xE0, 0xF8, 0xD0),
            (0x88, 0xC0, 0x70),
            (0x34, 0x68, 0x56),
            (0x08, 0x18, 0x20),
        ],
    ),
    (
        "Pocket gray",
        [
            (0xC4, 0xCF, 0xA1),
            (0x8B, 0x95, 0x6D),
            (0x4D, 0x53, 0x3C),
            (0x1F, 0x1F, 0x1F),
        ],
    ),
    (
        "Grayscale",
        [
            (0xFF, 0xFF, 0xFF),
            (0xAA, 0xAA, 0xAA),
            (0x55, 0x55, 0x55),
            (0x00, 0x00, 0x00),
        ],
    ),
    (
        "Amber",
        [
            (0xFF, 0xE4, 0xA8),
            (0xD8, 0x98, 0x38),
            (0x86, 0x4C, 0x12),
            (0x28, 0x14, 0x00),
        ],
    ),
    (
        "Ocean",
        [
            (0xE0, 0xF4, 0xFF),
            (0x60, 0xA8, 0xD8),
            (0x28, 0x58, 0x98),
            (0x08, 0x20, 0x48),
        ],
    ),
    (
        "Berry",
        [
            (0xFF, 0xE8, 0xF0),
            (0xE0, 0x78, 0xA0),
            (0x90, 0x30, 0x58),
            (0x28, 0x08, 0x20),
        ],
    ),
];

/// The default palette (classic DMG green), used by headless screenshots.
pub const DEFAULT: &Palette = &PALETTES[0].1;

/// Opaque-ARGB form of a palette, for the minifb window buffer.
pub fn to_argb(p: &Palette) -> [u32; 4] {
    let c =
        |(r, g, b): (u8, u8, u8)| 0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    [c(p[0]), c(p[1]), c(p[2]), c(p[3])]
}
