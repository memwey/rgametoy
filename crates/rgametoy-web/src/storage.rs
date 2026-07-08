//! IndexedDB-backed persistence for save data. Stores one record per ROM,
//! keyed by the ROM's content hash. Holds three pieces of state per ROM:
//!
//! - `ram`: the cartridge's external RAM (battery save).
//! - `quickState`: the instant save-state slot (5 / 7).
//! - `romTitle`: for the status line when reloading a save.
//!
//! IndexedDB is fully async. We deliberately do **not** busy-wait for any
//! IDB event — early versions of this module did, and they would hang the
//! tab when the IDB callback fired late or not at all (e.g. locked
//! databases, private windows). Every operation here is fire-and-forget:
//! the caller hands in a callback (or fills a shared `Rc<RefCell<…>>`),
//! and the work happens on the next event-loop turn.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    Event, IdbDatabase, IdbObjectStoreParameters,
    IdbOpenDbRequest, IdbRequest,
};

/// Database / object-store names. Bumping the version is what triggers
/// `onupgradeneeded`; we only have one store at v1, so no migrations yet.
const DB_NAME: &str = "rgametoy";
const DB_VERSION: u32 = 1;
const STORE_NAME: &str = "saves";

/// A single persisted record. The IDB store's keyPath is `romHash`, so the
/// struct's field name is part of the schema.
#[derive(Clone, Debug)]
pub struct SaveRecord {
    pub rom_hash: String,
    pub rom_title: String,
    pub ram: Option<Vec<u8>>,
    pub quick_state: Option<Vec<u8>>,
    pub updated_at: f64,
}

/// FNV-1a 32-bit hash, identical to the algorithm `rgametoy-desktop` uses
/// in its `paths` module. Copied here (not shared) because the desktop
/// crate is cpal-bound and we want this crate to compile to `wasm32` with
/// no host code.
pub fn rom_hash(rom: &[u8]) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for &b in rom {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

/// Async, non-blocking IDB open. Returns immediately. The connection is
/// dropped into `slot` (a shared `Rc<RefCell<Option<IdbDatabase>>>`)
/// when `onsuccess` fires; on error the slot is left `None` and
/// `on_unavailable` runs so the host can surface the problem.
///
/// The open-request's three listeners (`onsuccess`, `onupgradeneeded`,
/// `onerror`) are wrapped in `Closure`s and `forget()`'d so they stay
/// alive until the request fires. Bounded leak: one set per page load.
pub fn init_async(
    slot: Rc<RefCell<Option<IdbDatabase>>>,
    on_unavailable: impl FnOnce() + 'static,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let factory = match window.indexed_db()? {
        Some(f) => f,
        None => {
            on_unavailable();
            return Ok(());
        }
    };
    let request: IdbOpenDbRequest = factory.open_with_u32(DB_NAME, DB_VERSION)?;

    let slot_for_open: Rc<RefCell<Option<IdbDatabase>>> = slot.clone();
    let open_closure = Closure::wrap(Box::new(move |ev: Event| {
        let target: IdbOpenDbRequest = match ev.target().and_then(|t| t.dyn_into().ok()) {
            Some(t) => t,
            None => return,
        };
        let db: IdbDatabase = match target.result() {
            Ok(v) => match v.dyn_into() {
                Ok(d) => d,
                Err(_) => return,
            },
            Err(_) => return,
        };
        *slot_for_open.borrow_mut() = Some(db);
    }) as Box<dyn FnMut(Event)>);
    request.set_onsuccess(Some(open_closure.as_ref().unchecked_ref()));
    open_closure.forget();

    let upgrade_closure = Closure::wrap(Box::new(move |ev: Event| {
        let target: IdbOpenDbRequest = match ev.target().and_then(|t| t.dyn_into().ok()) {
            Some(t) => t,
            None => return,
        };
        let db: IdbDatabase = match target.result() {
            Ok(v) => match v.dyn_into() {
                Ok(d) => d,
                Err(_) => return,
            },
            Err(_) => return,
        };
        // v1 schema: a single object store keyed by romHash.
        let params = IdbObjectStoreParameters::new();
        params.set_key_path(&JsValue::from_str("romHash"));
        let _ = db.create_object_store_with_optional_parameters(STORE_NAME, &params);
    }) as Box<dyn FnMut(Event)>);
    request.set_onupgradeneeded(Some(upgrade_closure.as_ref().unchecked_ref()));
    upgrade_closure.forget();

    let slot_for_error: Rc<RefCell<Option<IdbDatabase>>> = slot;
    let on_unavailable_cell: Rc<RefCell<Option<Box<dyn FnOnce()>>>> =
        Rc::new(RefCell::new(Some(Box::new(on_unavailable))));
    let on_unavailable_for_cb = on_unavailable_cell.clone();
    let error_closure = Closure::wrap(Box::new(move |_ev: Event| {
        *slot_for_error.borrow_mut() = None;
        if let Some(f) = on_unavailable_for_cb.borrow_mut().take() {
            f();
        }
    }) as Box<dyn FnMut(Event)>);
    request.set_onerror(Some(error_closure.as_ref().unchecked_ref()));
    error_closure.forget();
    Ok(())
}

/// Fire-and-forget record read. `on_result` is called on the next
/// event-loop turn with the record (or `None` if missing / corrupt).
/// If `db` is `None` (IDB not ready / unavailable) the callback runs
/// immediately with `None`.
pub fn get_record_async(
    db: &Option<IdbDatabase>,
    hash: String,
    on_result: impl FnOnce(Option<SaveRecord>) + 'static,
) {
    let Some(db) = db else {
        on_result(None);
        return;
    };
    let tx = match db.transaction_with_str_and_mode(STORE_NAME, web_sys::IdbTransactionMode::Readonly) {
        Ok(t) => t,
        Err(_) => {
            on_result(None);
            return;
        }
    };
    let store = match tx.object_store(STORE_NAME) {
        Ok(s) => s,
        Err(_) => {
            on_result(None);
            return;
        }
    };
    let request = match store.get(&JsValue::from_str(&hash)) {
        Ok(r) => r,
        Err(_) => {
            on_result(None);
            return;
        }
    };
    let on_result_cell: Rc<RefCell<Option<Box<dyn FnOnce(Option<SaveRecord>)>>>> =
        Rc::new(RefCell::new(Some(Box::new(on_result))));
    let cb_cell = on_result_cell.clone();
    let closure = Closure::wrap(Box::new(move |ev: Event| {
        let target: IdbRequest = match ev.target().and_then(|t| t.dyn_into().ok()) {
            Some(t) => t,
            None => return,
        };
        let v = target.result().unwrap_or(JsValue::UNDEFINED);
        let record = if v.is_undefined() || v.is_null() {
            None
        } else {
            js_to_record(&v).ok().flatten()
        };
        if let Some(f) = cb_cell.borrow_mut().take() {
            f(record);
        }
    }) as Box<dyn FnMut(Event)>);
    let _ = request.set_onsuccess(Some(closure.as_ref().unchecked_ref()));
    closure.forget();
}

/// Fire-and-forget record write. Errors (if any) are silently dropped
/// — the autosave loop is best-effort and would be noisy if it
/// surfaced every IDB hiccup.
pub fn put_record_async(db: &Option<IdbDatabase>, record: SaveRecord) {
    let Some(db) = db else {
        return;
    };
    let tx = match db.transaction_with_str_and_mode(STORE_NAME, web_sys::IdbTransactionMode::Readwrite) {
        Ok(t) => t,
        Err(_) => return,
    };
    let store = match tx.object_store(STORE_NAME) {
        Ok(s) => s,
        Err(_) => return,
    };
    let value = record_to_js(&record);
    let _ = store.put(&value);
}

// -- JS <-> Rust conversion ------------------------------------------------

fn record_to_js(r: &SaveRecord) -> JsValue {
    let obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("romHash"),
        &JsValue::from_str(&r.rom_hash),
    );
    let _ = js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("romTitle"),
        &JsValue::from_str(&r.rom_title),
    );
    let _ = js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("updatedAt"),
        &JsValue::from_f64(r.updated_at),
    );
    let ram_js = match &r.ram {
        Some(bytes) => {
            let arr = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
            arr.copy_from(bytes);
            arr.into()
        }
        None => JsValue::NULL,
    };
    let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("ram"), &ram_js);
    let qs_js = match &r.quick_state {
        Some(bytes) => {
            let arr = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
            arr.copy_from(bytes);
            arr.into()
        }
        None => JsValue::NULL,
    };
    let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("quickState"), &qs_js);
    obj.into()
}

fn js_to_record(v: &JsValue) -> Result<Option<SaveRecord>, JsValue> {
    let hash = reflect_string(v, "romHash")?;
    let title = reflect_string(v, "romTitle").unwrap_or_default();
    let updated = js_sys::Reflect::get(v, &JsValue::from_str("updatedAt"))
        .ok()
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    let ram = reflect_bytes(v, "ram")?;
    let qs = reflect_bytes(v, "quickState")?;
    Ok(Some(SaveRecord {
        rom_hash: hash,
        rom_title: title,
        ram,
        quick_state: qs,
        updated_at: updated,
    }))
}

fn reflect_string(v: &JsValue, key: &str) -> Result<String, JsValue> {
    let x = js_sys::Reflect::get(v, &JsValue::from_str(key))?;
    x.as_string()
        .ok_or_else(|| JsValue::from_str(&format!("expected string at {key}")))
}

fn reflect_bytes(v: &JsValue, key: &str) -> Result<Option<Vec<u8>>, JsValue> {
    let x = js_sys::Reflect::get(v, &JsValue::from_str(key))?;
    if x.is_null() || x.is_undefined() {
        return Ok(None);
    }
    let arr: js_sys::Uint8Array = match x.dyn_into() {
        Ok(a) => a,
        Err(_) => return Ok(None),
    };
    let mut out = vec![0u8; arr.length() as usize];
    arr.copy_to(&mut out);
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_hash_is_8_hex() {
        assert_eq!(rom_hash(b"hello world").len(), 8);
    }

    #[test]
    fn rom_hash_is_stable_and_content_addressed() {
        assert_eq!(rom_hash(b"same"), rom_hash(b"same"));
        assert_ne!(rom_hash(b"rom A"), rom_hash(b"rom B"));
    }

    #[test]
    fn rom_hash_matches_desktop_known_vector() {
        // Cross-check: this is what the desktop `rom_hash` produces for
        // an empty input — FNV-1a 32-bit of "" is 0x811c9dc5.
        assert_eq!(rom_hash(b""), "811c9dc5");
    }
}
