//! The audio on/off toggle. Enabling builds the AudioContext + AudioWorklet
//! graph lazily and asynchronously (Web Audio requires a user gesture, and
//! `addModule` returns a promise); disabling suspends the same context so it
//! can be resumed cheaply. The rAF loop drives emulation either way — audio
//! is a pure sink.

use std::cell::RefCell;
use std::rc::Rc;

use crate::weblog;

use super::Inner;

/// Start (or resume) audio. Sets the enabled flag + relabels the button
/// optimistically, then either resumes the existing context or builds one
/// asynchronously (`AudioWorklet.addModule` is a promise). Building is
/// fire-and-forget: until it resolves, the rAF loop's audio feed simply finds
/// no player yet and skips — emulation is unaffected.
pub(super) fn start_audio(host: &Rc<RefCell<Inner>>, label: Option<web_sys::HtmlElement>) {
    {
        let mut h = host.borrow_mut();
        if h.audio_enabled {
            return;
        }
        h.audio_enabled = true;
        h.set_status("audio: on");
        if let Some(btn) = &label {
            btn.set_text_content(Some("Disable audio"));
        }
        if let Some(player) = h.audio.as_ref() {
            if let Err(e) = player.resume() {
                weblog::error_val("audio: failed to resume the AudioContext", &e);
            }
            return;
        }
    }
    // No graph yet — build it off the borrow (async).
    let source_rate = host.borrow().console.audio_output_rate();
    let host = host.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match crate::audio::enable(source_rate).await {
            Ok(player) => {
                let mut h = host.borrow_mut();
                // The user may have toggled audio off while the graph was
                // loading; honour the current flag, but keep the player for reuse.
                if h.audio_enabled {
                    if let Err(e) = player.resume() {
                        weblog::error_val("audio: failed to resume the AudioContext", &e);
                    }
                } else {
                    let _ = player.suspend();
                }
                h.audio = Some(player);
            }
            Err(e) => {
                weblog::error_val("audio init failed", &e);
                let mut h = host.borrow_mut();
                h.audio_enabled = false;
                h.set_status("audio: init failed");
                if let Some(btn) = &label {
                    btn.set_text_content(Some("Enable audio"));
                }
            }
        }
    });
}

/// Suspend audio (disable). The rAF loop keeps driving emulation regardless.
pub(super) fn stop_audio(host: &Rc<RefCell<Inner>>, label: Option<web_sys::HtmlElement>) {
    let mut h = host.borrow_mut();
    if !h.audio_enabled {
        return;
    }
    if let Some(player) = h.audio.as_ref() {
        if let Err(e) = player.suspend() {
            weblog::error_val("audio: failed to suspend the AudioContext", &e);
        }
    }
    h.audio_enabled = false;
    h.set_status("audio: off");
    if let Some(btn) = &label {
        btn.set_text_content(Some("Enable audio"));
    }
}
