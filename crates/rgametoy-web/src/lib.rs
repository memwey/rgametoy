//! rgametoy-web — browser frontend for the rgametoy Game Boy emulator.
//!
//! The actual emulated machine lives in `rgametoy-core` (dependency-free, no
//! host I/O); this crate is the host. See `../docs/web_spec.md` for the
//! full architecture and `web_tasks.md` for the staged build plan.

use wasm_bindgen::prelude::*;

mod audio;
mod canvas;
mod input;
mod pacing;
mod palette;
mod resample;
mod rom;
mod screenshot;
mod storage;
mod ui;
mod wasm_host;
mod weblog;

/// Internal surface re-exported only for the browser-integration tests in
/// `tests/web.rs`. Not a supported API; nothing in the shipped `.wasm` calls
/// it, so it's dead-code-eliminated from the cdylib.
#[doc(hidden)]
pub mod test_support {
    pub use crate::canvas::{Presenter, SCREEN_H, SCREEN_W};
    pub use crate::storage::{js_to_record, record_to_js, SaveRecord};
}

pub use wasm_host::WasmHost;

/// Wasm entry point. Trunk (via the `data-trunk` glue in `index.html`) loads
/// this crate's `.wasm` artifact and runs the symbol marked
/// `#[wasm_bindgen(start)]` once the module is instantiated. Returning
/// early is fine; the rAF loop and event listeners take over from here.
#[wasm_bindgen(start)]
pub fn main() {
    // Pretty panic messages in DevTools.
    console_error_panic_hook::set_once();

    // Construct the host. The constructor wires the rAF loop, the load
    // button, and the status line; any of those failing means the page is
    // mis-set-up, which we want to surface as a runtime error.
    if let Err(e) = WasmHost::new() {
        web_sys::console::error_1(&e);
    } else {
        web_sys::console::log_1(&"rgametoy web ok".into());
    }
}
