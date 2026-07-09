//! Cartridge loading on the web side. Wraps `rgametoy_core::Cartridge::from_bytes`
//! with a frontend-side type check (the core falls back to MBC1 silently
//! for unknown types, which we don't want here — we'd rather show the user
//! "unsupported" than a half-broken game).

use rgametoy_core::cartridge::Cartridge;
use wasm_bindgen::JsValue;

/// Whether the ROM's cartridge-type byte (header 0x0147) is one the core
/// actually emulates. Delegates to `Cartridge::is_type_supported` — the single
/// source of truth — rather than a second table here, which drifted before
/// (it listed MBC2, which the core does *not* implement, so a MBC2 ROM was
/// accepted and then silently mis-run as MBC1).
pub fn is_supported_type(data: &[u8]) -> bool {
    match data.get(0x0147) {
        Some(&t) => Cartridge::is_type_supported(t),
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

#[cfg(test)]
mod tests {
    use super::is_supported_type;

    fn rom_with_type(t: u8) -> Vec<u8> {
        let mut v = vec![0u8; 0x150];
        v[0x0147] = t;
        v
    }

    #[test]
    fn accepts_the_emulated_mbcs() {
        for t in [0x00, 0x01, 0x03, 0x0F, 0x13, 0x19, 0x1E] {
            assert!(is_supported_type(&rom_with_type(t)), "type {t:#04x}");
        }
    }

    #[test]
    fn rejects_mbc2_and_other_unimplemented() {
        // MBC2 (0x05/0x06) is the regression this guards: the core doesn't
        // implement it, so accepting it silently mis-runs the ROM as MBC1.
        for t in [0x05, 0x06, 0x0B, 0x20, 0x22, 0xFF] {
            assert!(!is_supported_type(&rom_with_type(t)), "type {t:#04x} must be rejected");
        }
    }

    #[test]
    fn rejects_a_header_too_short_to_have_a_type_byte() {
        assert!(!is_supported_type(&[0u8; 0x100]));
    }
}
