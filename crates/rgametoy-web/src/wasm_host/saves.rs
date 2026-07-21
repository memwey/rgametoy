//! ROM installation and save persistence: installing a cartridge, the power
//! cycle (Reset), and the IndexedDB flows — battery-RAM autosave, the
//! quick-state slot, and the hash-guarded restore on load. All IDB traffic
//! is fire-and-forget async; the callbacks re-enter `Inner` through the
//! `host_rc` back-reference and re-check the live ROM hash before applying
//! anything.

use std::cell::RefCell;
use std::rc::Rc;

use rgametoy_core::cartridge::Cartridge;
use web_sys::IdbDatabase;

use crate::rom::{cartridge_title, load_rom};
use crate::storage::{rom_hash, SaveRecord};
use crate::weblog;

use super::Inner;

impl Inner {
    /// Install a ROM from raw bytes. The frontend already sanity-checks the
    /// header (`rom::is_supported_type`); any further failure is reported
    /// into `#status` so the user sees it without having to open DevTools.
    /// On success, also looks up the matching IDB save record (if any) and
    /// restores its RAM / quick-state into the console. The IDB read is
    /// fire-and-forget — the user sees "loaded: <title>" immediately and
    /// the restored-save status message lands a tick later.
    pub(super) fn load_rom(&mut self, bytes: &[u8]) {
        let cart: Cartridge = match load_rom(bytes.to_vec()) {
            Ok(c) => c,
            Err(e) => {
                let msg = e.as_string().unwrap_or_else(|| "load failed".to_string());
                self.set_status(&format!("ROM error: {msg}"));
                return;
            }
        };
        // Switching cartridges is a save boundary. Snapshot any dirty RAM into
        // an IDB transaction before replacing the cartridge; the transaction
        // owns its bytes and may finish after the new ROM is installed.
        if self.has_rom && self.console.cartridge().ram_dirty() {
            self.persist_record();
        }
        let title = cartridge_title(bytes);
        self.title = title.clone();
        self.console.power_on(cart);
        self.has_rom = true;
        self.paused = false;
        self.quick_state = None;
        self.save_pending = false;
        self.load_pending = false;
        self.pending_restore = None;
        self.frame_accum_ms = 0.0;
        self.last_tick_ms = 0.0;

        // Compute the content hash and look up an existing save record.
        // The lookup is async: the user sees the ROM running first, and
        // the saved RAM / state land a tick later.
        self.rom_hash = rom_hash(bytes);
        self.set_status(&format!("loaded: {title} ({} KB)", bytes.len() / 1024));

        // Look up and restore this ROM's save. If IDB is already open, do it
        // now; otherwise defer — the IDB-ready hook retries (P2). Either way the
        // restore is hash-guarded so a late reply can't land on a newer ROM.
        let Some(host_rc) = self.host_rc.clone() else {
            return;
        };
        if self.storage.borrow().is_some() {
            restore_save(&host_rc, &self.storage, self.rom_hash.clone());
        } else {
            self.pending_restore = Some(self.rom_hash.clone());
        }
    }

    /// Reboot the current ROM in place. Used by the Reset button: the
    /// cartridge is kept, but the bus is reloaded and CPU / PPU / APU
    /// state is reset to the post-boot values. Saved RAM and the
    /// in-memory title are not touched.
    pub(super) fn power_cycle(&mut self) {
        if !self.has_rom {
            self.set_status("nothing to reset");
            return;
        }
        // The toolbar calls this "Reset", but the DMG has no reset button: the
        // core models an off/on power cycle and preserves battery-backed RAM.
        self.console.power_cycle();
        self.paused = false;
        self.set_status("reset");
    }

    /// Persist the current battery RAM to IDB. Called from the rAF loop
    /// on a debounced cadence, and from the save/load hotkeys (5/7) so
    /// the user-visible slot updates immediately.
    pub(super) fn persist_record(&mut self) {
        if self.rom_hash.is_empty() {
            return;
        }
        let ram_snapshot = self.console.cartridge().ram().to_vec();
        // Only write if the cartridge actually has RAM — saves with no
        // external RAM are pointless, and the dirty-flag check (a separate code
        // path) would not be triggered anyway. Log to the console (not the
        // status bar — this is a ~2 s background flush, not a user action).
        let ram = if ram_snapshot.is_empty() {
            None
        } else {
            Some(ram_snapshot.clone())
        };
        let hash = self.rom_hash.clone();
        let record = SaveRecord {
            rom_hash: hash.clone(),
            rom_title: self.title.clone(),
            ram,
            // Preserve the saved quick-state slot: autosave rewrites the whole
            // record, so writing `None` here would wipe the user's quick-save.
            quick_state: self.quick_state.clone(),
            updated_at: js_sys::Date::now(),
        };
        let Some(host_rc) = self.host_rc.clone() else {
            return;
        };
        crate::storage::put_record_async(&self.storage.borrow(), record, move |committed| {
            if !committed {
                return;
            }
            let Ok(mut inner) = host_rc.try_borrow_mut() else {
                return;
            };
            if inner.rom_hash == hash && inner.console.cartridge().ram() == ram_snapshot {
                inner.console.cartridge_mut().clear_ram_dirty();
                web_sys::console::log_1(
                    &format!("battery saved ({} KiB)", ram_snapshot.len() / 1024).into(),
                );
            }
        });
    }

    /// Update the quick-state slot in IDB. `Some(bytes)` saves the slot
    /// with that data; `None` loads the slot from IDB into the console.
    pub(super) fn write_quick_state_to_storage(&mut self, bytes: Vec<u8>) {
        if self.rom_hash.is_empty() {
            return;
        }
        let record = SaveRecord {
            rom_hash: self.rom_hash.clone(),
            rom_title: self.title.clone(),
            ram: self.console.cartridge().ram().to_vec().into(),
            quick_state: Some(bytes),
            updated_at: js_sys::Date::now(),
        };
        crate::storage::put_record_async(&self.storage.borrow(), record, |_| {});
    }

    /// Load the quick-state slot. Prefers the in-memory copy: it's synchronous
    /// and repeatable (same as the desktop frontend), so a second load of the
    /// same save works — routing every load through an async IDB read made it
    /// fragile, seeming to only work once. IDB is a fallback for the one case
    /// the in-memory slot can't cover: a fresh page load before the slot has
    /// been populated. The IDB result is cached in memory so the next load is
    /// instant.
    pub(super) fn load_quick_state(&mut self) {
        if let Some(qs) = self.quick_state.clone() {
            match self.console.load_state_bytes(&qs) {
                Ok(()) => self.set_status("loaded state"),
                Err(e) => self.set_status(&format!("state error: {e}")),
            }
            return;
        }
        if self.rom_hash.is_empty() {
            self.set_status("no save slot for this ROM");
            return;
        }
        self.set_status("loading state…");
        let Some(host_rc) = self.host_rc.clone() else {
            return;
        };
        let storage = self.storage.clone();
        let hash = self.rom_hash.clone();
        crate::storage::get_record_async(&storage.borrow(), hash.clone(), move |rec| {
            let Ok(mut inner) = host_rc.try_borrow_mut() else {
                return;
            };
            if inner.rom_hash != hash {
                return;
            }
            match rec.and_then(|r| r.quick_state) {
                Some(qs) => match inner.console.load_state_bytes(&qs) {
                    Ok(()) => {
                        inner.quick_state = Some(qs);
                        inner.set_status("loaded state");
                    }
                    Err(e) => inner.set_status(&format!("state error: {e}")),
                },
                None => inner.set_status("no save slot for this ROM"),
            }
        });
    }
}

/// Restore a ROM's saved RAM + quick-state from IDB into the console. The async
/// reply re-checks that the live ROM (`inner.rom_hash`) *and* the record's hash
/// still equal `hash` before applying anything, so a slow reply for ROM A can't
/// clobber a since-loaded ROM B.
pub(super) fn restore_save(
    host_rc: &Rc<RefCell<Inner>>,
    storage: &Rc<RefCell<Option<IdbDatabase>>>,
    hash: String,
) {
    let host_rc = host_rc.clone();
    let storage_owned = storage.clone();
    crate::storage::get_record_async(&storage.borrow(), hash.clone(), move |rec| {
        let mut inner = match host_rc.try_borrow_mut() {
            Ok(inner) => inner,
            Err(_) => {
                // Inner is momentarily borrowed. This shouldn't happen — the
                // reply fires between event-loop tasks, when no borrow is held —
                // but if it does, retry on the next microtask rather than
                // silently dropping the restore (and losing the save).
                let (h, s, hash) = (host_rc.clone(), storage_owned.clone(), hash.clone());
                wasm_bindgen_futures::spawn_local(async move { restore_save(&h, &s, hash) });
                return;
            }
        };
        if inner.rom_hash != hash {
            return; // a different ROM is loaded now — discard this late reply
        }
        let Some(rec) = rec else { return };
        if rec.rom_hash != hash {
            return; // record is for another ROM — discard
        }
        if let Some(ram) = &rec.ram {
            inner.console.cartridge_mut().load_ram(ram);
        }
        if let Some(qs) = &rec.quick_state {
            if let Err(e) = inner.console.load_state_bytes(qs) {
                weblog::error(&format!("saved quick-state could not be restored: {e}"));
            }
        }
        inner.quick_state = rec.quick_state.clone();
        inner.set_status("restored save");
    });
}
