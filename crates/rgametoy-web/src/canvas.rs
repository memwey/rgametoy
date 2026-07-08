//! Canvas plumbing: find the `<canvas id="screen">` element, get a 2D context
//! sized to the 160×144 framebuffer, and blit each frame as RGBA pixels via
//! `putImageData`. The browser handles the integer-scale CSS upscaling via
//! `image-rendering: pixelated` in `style/main.css`.

use wasm_bindgen::{Clamped, JsCast, JsValue};
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

pub fn get_canvas(doc: &Document) -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), JsValue> {
    let canvas: HtmlCanvasElement = get_element_by_id(doc, "screen")?;
    canvas.set_width(SCREEN_W);
    canvas.set_height(SCREEN_H);
    let ctx = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("canvas: 2d context unavailable"))?
        .dyn_into::<CanvasRenderingContext2d>()?;
    Ok((canvas, ctx))
}

/// Push the contents of `rgba_buf` (a `SCREEN_W * SCREEN_H * 4` RGBA buffer)
/// to the canvas. The ImageData is created fresh each frame; the 23 KB
/// allocation is small enough that pool-reuse would be premature optimisation.
pub fn present(ctx: &CanvasRenderingContext2d, rgba_buf: &[u8]) -> Result<(), JsValue> {
    let clamped = Clamped(rgba_buf);
    let image = ImageData::new_with_u8_clamped_array(clamped, SCREEN_W)?;
    ctx.put_image_data(&image, 0.0, 0.0)?;
    Ok(())
}
