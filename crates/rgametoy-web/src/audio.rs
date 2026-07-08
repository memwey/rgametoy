//! WebAudio output: a `ScriptProcessorNode` that drains interleaved stereo
//! from the core's APU and feeds the default audio device. The audio
//! callback drives the emulator forward in time so audio and visual stay
//! in lock-step; the rAF loop only paints the current framebuffer.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    AudioBuffer, AudioContext, AudioProcessingEvent, Event, EventTarget,
    ScriptProcessorNode,
};

use crate::resample::Resampler;

/// Bundles the AudioContext plus the ScriptProcessorNode's lifetime. Both
/// are kept alive for the page's lifetime; the `onaudioprocess` closure is
/// `forget()`-leaked in [`enable`] (audio is brought up at most once).
pub struct AudioPlayer {
    pub ctx: AudioContext,
    #[allow(dead_code)]
    pub node: ScriptProcessorNode,
    #[allow(dead_code)]
    pub device_rate: u32,
}

/// Maximum number of interleaved source samples the resampler is allowed to
/// hold. One Game Boy frame at 48 kHz is ~800 samples; the cap is set
/// generously so a stutter never has to drop the APU's output wholesale.
const RESAMPLE_BUF_CAP: usize = 16384;

/// Bring up audio: a new `AudioContext`, a ScriptProcessor, and the
/// `onaudioprocess` closure that drains the APU and writes the output
/// buffer. The caller passes in the shared `Rc<RefCell<Inner>>` so the
/// callback can run frames on the same `Console` the rAF loop is reading.
///
/// `source_rate` (the APU's fixed output rate) is passed in rather than read
/// from `inner` here: the callers hold a `borrow_mut()` on the same `RefCell`
/// while calling this, so a `borrow()` inside would panic ("already mutably
/// borrowed"). `inner` is only ever touched *asynchronously* — when the audio
/// callback fires — never synchronously in this function.
///
/// Returns the live `AudioPlayer` (kept in `Inner::audio` so the GC doesn't
/// reap the AudioContext); the `onaudioprocess` closure is leaked internally.
/// The context is *not* resumed — modern browsers gate `AudioContext` on a
/// user gesture, so the UI exposes an "Enable audio" button that calls
/// `AudioPlayer::resume` after the click.
pub fn enable(
    inner: Rc<RefCell<crate::wasm_host::Inner>>,
    source_rate: u32,
) -> Result<AudioPlayer, JsValue> {
    let ctx = AudioContext::new()?;
    let device_rate = ctx.sample_rate() as u32;
    // Buffer size: 2048 frames at the device rate is ~43 ms at 48 kHz —
    // a balance between latency and the overhead per callback. The
    // browser clamps to a power of two in [256, 16384].
    let node = ctx.create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(2048, 0, 2)?;
    node.connect_with_audio_node(&ctx.destination())?;

    let host: Rc<RefCell<crate::wasm_host::Inner>> = inner;
    // The APU's output rate is fixed, so we build a *persistent* resampler:
    // recreating it per callback would reset its phase (`pos`/`prev`) and click
    // at every buffer boundary. The output buffer is reused too.
    let source_rate = source_rate.max(1);
    let mut resampler = Resampler::new(source_rate, device_rate.max(1));
    let mut resampled: Vec<f32> = Vec::new();
    let closure = Closure::wrap(Box::new(move |ev: Event| {
        // ScriptProcessorNode's onaudioprocess is the only place where the
        // runtime asks us to produce samples. Advance the emulator and produce
        // audio for *this* batch.
        let ev: AudioProcessingEvent = ev.unchecked_into();
        let output = match ev.output_buffer() {
            Ok(b) => b,
            Err(_) => return,
        };
        if source_rate == 0 || device_rate == 0 {
            return;
        }

        let mut host = host.borrow_mut();
        // Output length in frames at the device rate; produce that many frames
        // worth of (resampled) APU output.
        let out_frames = output.length() as usize;
        // While fast-forward is held, the rAF loop drives the console at the
        // turbo multiplier and fast-forward is muted (matches desktop). Emit
        // silence and don't step here — stepping too would double-drive it.
        if host.keys_down.contains(crate::input::TURBO_KEY) {
            let silence = vec![0.0f32; out_frames];
            let _ = output.copy_to_channel_with_start_in_channel(&silence, 0, 0);
            let _ = output.copy_to_channel_with_start_in_channel(&silence, 1, 0);
            return;
        }
        // Source frames required, rounded up so we never under-fill.
        let need_source_frames =
            (out_frames * source_rate as usize).div_ceil(device_rate as usize);
        // The core has no public "run N cycles" entry point, so step whole
        // instructions until we've spent roughly this batch's cycle budget.
        let need_cycles = (need_source_frames as u64) * (4_194_304u64 / source_rate as u64);
        let mut spent = 0u64;
        while spent < need_cycles {
            spent += host.console.step() as u64;
        }
        let apu_samples = host.console.take_audio_samples();
        // Interleaved stereo f32 at `source_rate`; cap to keep it bounded.
        let truncated = if apu_samples.len() > RESAMPLE_BUF_CAP {
            &apu_samples[apu_samples.len() - RESAMPLE_BUF_CAP..]
        } else {
            &apu_samples[..]
        };

        // Resample through the persistent resampler (continuous phase across
        // callbacks), reusing the output buffer, then pad/truncate to exactly
        // out_frames * 2 interleaved.
        resampled.clear();
        resampler.process(truncated, &mut resampled);
        if resampled.len() < out_frames * 2 {
            resampled.resize(out_frames * 2, 0.0);
        } else if resampled.len() > out_frames * 2 {
            resampled.truncate(out_frames * 2);
        }

        // `copyToChannel` wants a non-interleaved slice, so deinterleave.
        let mut left: Vec<f32> = resampled.iter().step_by(2).copied().collect();
        let mut right: Vec<f32> = resampled.iter().skip(1).step_by(2).copied().collect();
        if left.len() < out_frames {
            left.resize(out_frames, 0.0);
        }
        if right.len() < out_frames {
            right.resize(out_frames, 0.0);
        }
        let _ = output.copy_to_channel_with_start_in_channel(&left, 0, 0);
        let _ = output.copy_to_channel_with_start_in_channel(&right, 1, 0);
    }) as Box<dyn FnMut(Event)>);

    // `set_onaudioprocess` is a property setter; the closure-as-property
    // pattern requires `as_ref().unchecked_ref()`.
    node.set_onaudioprocess(Some(closure.as_ref().unchecked_ref()));

    // Leak the closure so it outlives this call. Audio is enabled at most once
    // per page (the button early-returns once `audio_enabled`), so this is a
    // bounded, one-time leak — no need to park it in a field on `Inner`.
    closure.forget();

    Ok(AudioPlayer {
        ctx,
        node,
        device_rate,
    })
}

impl AudioPlayer {
    /// Resume the AudioContext. Modern browsers suspend the context on
    /// creation; the user has to do something (click) before audio
    /// actually flows. Call this from a click handler.
    pub fn resume(&self) -> Result<(), JsValue> {
        let _ = self.ctx.resume()?;
        Ok(())
    }

    /// Suspend audio output (e.g. when the tab loses focus). The emulator
    /// keeps running visually.
    pub fn suspend(&self) -> Result<(), JsValue> {
        let _ = self.ctx.suspend()?;
        Ok(())
    }
}

// Avoid orphan import warnings on a couple of types we want to keep for
// the closure's downcast even if we don't name them at module scope.
#[allow(dead_code)]
fn _ensure_targets(_: &EventTarget, _: &AudioBuffer) {}
