//! Browser-side screenshot: take the last RGBA frame the canvas was
//! blitted with and offer it as a download (`<title>-<timestamp>.png`).
//!
//! The RGBA buffer is in the *expanded* form (post-palette), so this
//! function writes a tiny custom PNG straight from Rust — no canvas
//! re-read, no `toDataURL` round-trip. The encoding is small (160×144)
//! and the format is simple enough that a hand-rolled writer is shorter
//! than a dependency would be.

use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

const W: u32 = 160;
const H: u32 = 144;

/// Build a 32-bit-per-pixel BMP (top-down, no compression, BI_RGB) for
/// the given RGBA buffer and trigger a browser download. BMP is the
/// simplest standard image format with a free-from-the-host encoder
/// (PNG needs zlib for the IDAT stream; we don't have zlib here).
///
/// `rgba` must be `W * H * 4` bytes long, RGBA, top-down. The first row
/// of the buffer becomes the *top* of the image (BMP is bottom-up by
/// spec, so we flip while writing).
pub fn download(rgba: &[u8]) -> Result<(), JsValue> {
    let bmp = encode_bmp(rgba);

    let parts: js_sys::Array = js_sys::Array::new();
    let bytes = js_sys::Uint8Array::from(bmp.as_slice());
    parts.push(&bytes);
    let opts = BlobPropertyBag::new();
    opts.set_type("image/bmp");
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &opts)?;
    let url = Url::create_object_url_with_blob(&blob)?;

    let doc = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let a: HtmlAnchorElement = doc
        .create_element("a")?
        .dyn_into()
        .map_err(|_| JsValue::from_str("create <a> failed"))?;
    a.set_href(&url);
    a.set_download(&format!("rgametoy-{}.bmp", timestamp()));
    a.click();
    // We don't revoke immediately — the click is async and revoke
    // before the browser starts reading the URL would break the
    // download. The browser cleans up blob URLs when the document
    // unloads anyway.
    Ok(())
}

fn timestamp() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

// -- BMP encoder (BI_RGB, 32 bpp, top-down via negative height) -------------

fn encode_bmp(rgba: &[u8]) -> Vec<u8> {
    let row_bytes = (W * 4) as usize;
    let pixel_bytes = row_bytes * H as usize;
    let file_size = 14 + 40 + pixel_bytes;

    let mut out = Vec::with_capacity(file_size);

    // --- File header (14 bytes) ---
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]); // reserved
    out.extend_from_slice(&((14 + 40) as u32).to_le_bytes()); // pixel offset

    // --- DIB header (BITMAPINFOHEADER, 40 bytes) ---
    out.extend_from_slice(&40u32.to_le_bytes()); // header size
    out.extend_from_slice(&W.to_le_bytes()); // width
    out.extend_from_slice(&(-(H as i32)).to_le_bytes()); // height (negative = top-down)
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bpp
    out.extend_from_slice(&0u32.to_le_bytes()); // compression: BI_RGB
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes()); // image size
    out.extend_from_slice(&2835u32.to_le_bytes()); // ~72 DPI x
    out.extend_from_slice(&2835u32.to_le_bytes()); // ~72 DPI y
    out.extend_from_slice(&0u32.to_le_bytes()); // colours in palette
    out.extend_from_slice(&0u32.to_le_bytes()); // important colours

    // --- Pixel data ---
    // RGBA -> BGRA per pixel (BMP is BGR-ordered, with alpha in the high
    // byte on 32-bpp). With a top-down height the rows appear in the
    // order the buffer is laid out, so we just swap R<->B and emit.
    for chunk in rgba.chunks_exact(4) {
        out.push(chunk[2]); // B
        out.push(chunk[1]); // G
        out.push(chunk[0]); // R
        out.push(chunk[3]); // A
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_header_size_is_correct() {
        // 4 transparent pixels; the header is 14 + 40 = 54 bytes.
        let rgba = vec![0u8; 4 * 4];
        let bmp = encode_bmp(&rgba);
        assert_eq!(bmp.len(), 14 + 40 + 4 * 4);
    }

    #[test]
    fn bmp_magic_is_bm() {
        let rgba = vec![0u8; 4 * 4];
        let bmp = encode_bmp(&rgba);
        assert_eq!(&bmp[0..2], b"BM");
    }

    #[test]
    fn bmp_swaps_red_and_blue() {
        let rgba = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let bmp = encode_bmp(&rgba);
        // 14 (file) + 40 (dib) = 54, then the pixel data starts.
        assert_eq!(&bmp[54..58], &[0xCC, 0xBB, 0xAA, 0xDD]);
    }
}
