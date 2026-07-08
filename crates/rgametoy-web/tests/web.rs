//! Browser-integration tests. Run them in a real, headless browser from
//! `crates/rgametoy-web`:
//!
//! ```sh
//! wasm-pack test --headless --firefox --test web    # or --chrome
//! ```
//!
//! The `--test web` scopes the run to this file. Without it wasm-pack also tries
//! to build the crate's `#[test]` unit tests for wasm, which collides with the
//! `#[wasm_bindgen(start)]` entry point — those are host tests, run separately
//! with `cargo test`.
//!
//! They cover the paths the host `#[test]`s can't reach because they need a JS
//! runtime: the IndexedDB record ⇄ JS-object conversion, and the canvas blit.
//!
//! The whole file is gated to `target_arch = "wasm32"` so a plain host
//! `cargo test` compiles it to an empty (0-test) binary instead of trying to
//! run browser code on the host.
#![cfg(target_arch = "wasm32")]

use rgametoy_web::test_support::{
    js_to_record, record_to_js, Presenter, SaveRecord, SCREEN_H, SCREEN_W,
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// A fully-populated record must survive `record_to_js` → `js_to_record`
/// unchanged — this is the persisted save's serialization contract.
#[wasm_bindgen_test]
fn record_survives_js_roundtrip() {
    let rec = SaveRecord {
        rom_hash: "abc12345".into(),
        rom_title: "TETRIS".into(),
        ram: Some(vec![0, 1, 2, 254, 255]),
        quick_state: Some(vec![9, 8, 7, 6]),
        updated_at: 1234.5,
    };
    let back = js_to_record(&record_to_js(&rec)).unwrap().unwrap();
    assert_eq!(back.rom_hash, rec.rom_hash);
    assert_eq!(back.rom_title, rec.rom_title);
    assert_eq!(back.ram, rec.ram);
    assert_eq!(back.quick_state, rec.quick_state);
    assert_eq!(back.updated_at, rec.updated_at);
}

/// A ROM with no battery RAM and no quick-state stores JS `null` for both;
/// they must come back as `None`, not as an empty `Vec`.
#[wasm_bindgen_test]
fn absent_ram_and_state_roundtrip_as_none() {
    let rec = SaveRecord {
        rom_hash: "deadbeef".into(),
        rom_title: String::new(),
        ram: None,
        quick_state: None,
        updated_at: 0.0,
    };
    let back = js_to_record(&record_to_js(&rec)).unwrap().unwrap();
    assert_eq!(back.ram, None);
    assert_eq!(back.quick_state, None);
}

/// A malformed stored object (missing `romHash`) must be an `Err`, not a
/// panic — that's what lets `get_record_async` log "corrupt, ignoring" and
/// carry on instead of taking down the tab.
#[wasm_bindgen_test]
fn a_record_without_hash_is_rejected() {
    let obj = js_sys::Object::new();
    assert!(js_to_record(&JsValue::from(obj)).is_err());
}

/// `Presenter::blit` must actually push pixels to the 2D context via its
/// persistent ImageData: blit a solid buffer and read the top-left pixel back.
/// This also exercises the load-bearing assumption behind the reuse — that the
/// `ImageData`'s store *is* the shared array we copy into each frame.
#[wasm_bindgen_test]
fn presenter_blits_rgba_to_a_canvas() {
    let doc = web_sys::window().unwrap().document().unwrap();
    let canvas: web_sys::HtmlCanvasElement =
        doc.create_element("canvas").unwrap().dyn_into().unwrap();
    canvas.set_width(SCREEN_W);
    canvas.set_height(SCREEN_H);
    let ctx: web_sys::CanvasRenderingContext2d =
        canvas.get_context("2d").unwrap().unwrap().dyn_into().unwrap();

    let presenter = Presenter::new().unwrap();
    let mut buf = vec![0u8; (SCREEN_W * SCREEN_H * 4) as usize];
    for px in buf.chunks_exact_mut(4) {
        px.copy_from_slice(&[200, 30, 40, 255]); // opaque red
    }
    presenter.blit(&ctx, &buf).unwrap();

    let data = ctx.get_image_data(0.0, 0.0, 1.0, 1.0).unwrap().data();
    assert_eq!(&data[0..4], &[200, 30, 40, 255]);

    // Blit a second, different frame through the *same* Presenter — proves the
    // reused ImageData reflects the new bytes, not the first frame's.
    for px in buf.chunks_exact_mut(4) {
        px.copy_from_slice(&[10, 20, 30, 255]);
    }
    presenter.blit(&ctx, &buf).unwrap();
    let data = ctx.get_image_data(0.0, 0.0, 1.0, 1.0).unwrap().data();
    assert_eq!(&data[0..4], &[10, 20, 30, 255]);
}
