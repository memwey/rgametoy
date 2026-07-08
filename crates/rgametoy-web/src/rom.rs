//! Cartridge loading on the web side. Wraps `rgametoy_core::Cartridge::from_bytes`
//! with a frontend-side type check (the core falls back to MBC1 silently
//! for unknown types, which we don't want here — we'd rather show the user
//! "unsupported" than a half-broken game).

use rgametoy_core::cartridge::Cartridge;
use wasm_bindgen::JsValue;

/// Cartridge type bytes the DMG core supports: no-MBC, MBC1, MBC3, MBC5
/// (with or without RAM and battery). The values are the byte at 0x0147 in
/// the ROM header.
const SUPPORTED_TYPES: &[u8] = &[
    0x00, 0x01, 0x02, 0x03, // ROM only, MBC1, MBC1+RAM, MBC1+RAM+BATTERY
    0x05, 0x06,             // MBC2, MBC2+BATTERY
    0x0F, 0x10, 0x11, 0x12, 0x13, // MBC3 variants
    0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, // MBC5 variants
];

pub fn is_supported_type(data: &[u8]) -> bool {
    match data.get(0x0147) {
        Some(&t) => SUPPORTED_TYPES.contains(&t),
        None => false,
    }
}

pub fn load_rom(data: Vec<u8>) -> Result<Cartridge, JsValue> {
    if data.len() < 0x150 {
        return Err(JsValue::from_str("ROM too small (< 336 bytes)"));
    }
    if !is_supported_type(&data) {
        let t = data[0x0147];
        return Err(JsValue::from_str(&format!(
            "unsupported cartridge type {t:#04x} (only no-MBC, MBC1, MBC3, MBC5 are emulated)"
        )));
    }
    Ok(Cartridge::from_bytes(data))
}

/// Pull the cartridge title out of the ROM header (0x0134..0x0143, terminated
/// by a 0 byte or end of slice). DMG titles are uppercase ASCII; the
/// frontend shows them verbatim.
pub fn cartridge_title(data: &[u8]) -> String {
    let mut title = String::new();
    for &b in &data[0x0134..0x0143.min(data.len())] {
        if b == 0 {
            break;
        }
        // Tolerate non-ASCII in the title slot (e.g. CGB flag in 0x0143) by
        // keeping printable ASCII only.
        if (0x20..0x7F).contains(&b) {
            title.push(b as char);
        }
    }
    title.trim().to_string()
}
