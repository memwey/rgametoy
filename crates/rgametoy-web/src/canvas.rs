//! Canvas plumbing: find the `<canvas id="screen">` element, get a 2D context
//! sized to the 160×144 framebuffer, and blit each frame as RGBA pixels via
//! `putImageData`. The browser handles the integer-scale CSS upscaling via
//! `image-rendering: pixelated` in `style/main.css`.

use js_sys::Uint8ClampedArray;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, Document, HtmlCanvasElement, ImageData};

pub const SCREEN_W: u32 = 160;
pub const SCREEN_H: u32 = 144;
pub const RGBA_LEN: usize = (SCREEN_W * SCREEN_H * 4) as usize;

/// Look up an element by ID and assert it's a particular subtype. The browser
/// API is loosely typed (everything is `Element?`), so this centralises the
/// "the HTML is wrong" failure mode in one place.
pub fn get_element_by_id<T: JsCast>(doc: &Document, id: &str) -> Result<T, JsValue> {
    let el = doc
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("missing element #{id}")))?;
    el.dyn_into::<T>()
        .map_err(|_| JsValue::from_str(&format!("element #{id} has wrong type")))
}

pub fn get_canvas(
    doc: &Document,
) -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), JsValue> {
    let canvas: HtmlCanvasElement = get_element_by_id(doc, "screen")?;
    canvas.set_width(SCREEN_W);
    canvas.set_height(SCREEN_H);
    let ctx = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("canvas: 2d context unavailable"))?
        .dyn_into::<CanvasRenderingContext2d>()?;
    Ok((canvas, ctx))
}

/// Reusable canvas blitter. Both the `ImageData` and its backing
/// `Uint8ClampedArray` are allocated once, up front. Per the `ImageData(data,…)`
/// constructor the `image`'s pixel store *is* `array` (same object, not a copy),
/// so each frame we copy the framebuffer into `array` and `putImageData` reads
/// it straight back — no fresh ~90 KB `ImageData` allocated (and GC'd) every
/// frame, which is what the old per-frame `present` did 60×/s.
pub struct Presenter {
    array: Uint8ClampedArray,
    image: ImageData,
}

impl Presenter {
    pub fn new() -> Result<Presenter, JsValue> {
        let array = Uint8ClampedArray::new_with_length(RGBA_LEN as u32);
        let image = ImageData::new_with_js_u8_clamped_array_and_sh(&array, SCREEN_W, SCREEN_H)?;
        Ok(Presenter { array, image })
    }

    /// Push `rgba_buf` (a `SCREEN_W * SCREEN_H * 4` RGBA buffer) to the canvas.
    pub fn blit(&self, ctx: &CanvasRenderingContext2d, rgba_buf: &[u8]) -> Result<(), JsValue> {
        self.array.copy_from(rgba_buf);
        ctx.put_image_data(&self.image, 0.0, 0.0)
    }
}
