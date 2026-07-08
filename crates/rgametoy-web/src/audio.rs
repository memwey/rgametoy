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

/// Linear resampler for interleaved stereo at a fixed rate ratio (APU
/// source rate → device rate). Same algorithm as `rgametoy-desktop`'s
/// `Resampler`; reproduced here because that crate is cpal-bound.
struct Resampler {
    step: f64,
    pos: f64,
    prev: (f32, f32),
    primed: bool,
}

impl Resampler {
    fn new(source_rate: u32, device_rate: u32) -> Resampler {
        Resampler {
            step: source_rate as f64 / device_rate as f64,
            pos: 0.0,
            prev: (0.0, 0.0),
            primed: false,
        }
    }

    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for frame in input.chunks_exact(2) {
            let cur = (frame[0], frame[1]);
            if !self.primed {
                self.prev = cur;
                self.primed = true;
            }
            while self.pos < 1.0 {
                let t = self.pos as f32;
                out.push(self.prev.0 + (cur.0 - self.prev.0) * t);
                out.push(self.prev.1 + (cur.1 - self.prev.1) * t);
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = cur;
        }
    }
}

/// Bundles the AudioContext plus the ScriptProcessorNode's lifetime. Both
/// are kept alive for the page's lifetime; the closure lives in
/// `Inner::audio_closure` so the GC can find it.
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
/// Returns the live `AudioPlayer` (kept in `Inner::audio` so the GC
/// doesn't reap the AudioContext) plus the closure stashed in
/// `Inner::audio_closure`. The context is *not* resumed — modern browsers
/// gate `AudioContext` on a user gesture, so the UI exposes an "Enable
/// audio" button that calls `AudioPlayer::resume` after the click.
pub fn enable(
    inner: Rc<RefCell<crate::wasm_host::Inner>>,
) -> Result<AudioPlayer, JsValue> {
    let ctx = AudioContext::new()?;
    let device_rate = ctx.sample_rate() as u32;
    // Buffer size: 2048 frames at the device rate is ~43 ms at 48 kHz —
    // a balance between latency and the overhead per callback. The
    // browser clamps to a power of two in [256, 16384].
    let node = ctx.create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(2048, 0, 2)?;
    node.connect_with_audio_node(&ctx.destination())?;

    let host: Rc<RefCell<crate::wasm_host::Inner>> = inner.clone();
    let closure = Closure::wrap(Box::new(move |ev: Event| {
        // ScriptProcessorNode's onaudioprocess is the only place where
        // the runtime asks us to produce samples. Take this opportunity
        // to advance the emulator and produce audio for *this* batch.
        let ev: AudioProcessingEvent = match ev.unchecked_into() {
            e => e,
        };
        let output = match ev.output_buffer() {
            Ok(b) => b,
            Err(_) => return,
        };

        let mut host = host.borrow_mut();
        let source_rate = host.console.audio_output_rate();
        if source_rate == 0 || device_rate == 0 {
            return;
        }

        // Output length in frames. `audio` callback asks for `output.length()`
        // frames at the device rate; we need to produce that many samples
        // worth of APU output (resampled).
        let out_frames = output.length() as usize;
        // Source frames required: (out_frames * source_rate) / device_rate,
        // rounded up so we never under-fill.
        let need_source_frames =
            (out_frames * source_rate as usize).div_ceil(device_rate as usize);
        // Each source frame is one step call's worth of CPU work, but a
        // step is one *instruction* and consumes a variable number of
        // T-cycles. Easier: run a fixed budget of cycles proportional
        // to the time slice.
        let need_cycles = (need_source_frames as u64) * (4_194_304u64 / source_rate as u64);

        // Drive the console forward. We use `step` (one instruction) in
        // a loop because the core has no public "run N cycles" entry
        // point — `run_frame` is fixed to 70224 T-cycles. To match the
        // requested time slice we step until we've spent enough cycles.
        let mut spent = 0u64;
        while spent < need_cycles {
            spent += host.console.step() as u64;
        }
        let apu_samples = host.console.take_audio_samples();
        // `apu_samples` is interleaved stereo f32 at `source_rate`.
        // Cap to keep the resampler bounded.
        let truncated = if apu_samples.len() > RESAMPLE_BUF_CAP {
            &apu_samples[apu_samples.len() - RESAMPLE_BUF_CAP..]
        } else {
            &apu_samples[..]
        };

        // Resample into the output buffer.
        let mut resampled: Vec<f32> = Vec::with_capacity(out_frames * 2);
        let mut r = Resampler::new(source_rate, device_rate);
        r.process(truncated, &mut resampled);
        // Pad / truncate to exactly out_frames * 2 (interleaved stereo).
        if resampled.len() < out_frames * 2 {
            resampled.resize(out_frames * 2, 0.0);
        } else if resampled.len() > out_frames * 2 {
            resampled.truncate(out_frames * 2);
        }

        // Write to the AudioBuffer's two channels. `copyToChannel` expects
        // a non-interleaved slice, so we deinterleave on the fly.
        let mut left: Vec<f32> = resampled.iter().step_by(2).copied().collect();
        let mut right: Vec<f32> = resampled.iter().skip(1).step_by(2).copied().collect();
        if left.len() < out_frames {
            left.resize(out_frames, 0.0);
        }
        if right.len() < out_frames {
            right.resize(out_frames, 0.0);
        }
        let _ = output.copy_to_channel_with_start_in_channel(&mut left, 0, 0);
        let _ = output.copy_to_channel_with_start_in_channel(&mut right, 1, 0);
    }) as Box<dyn FnMut(Event)>);

    // `set_onaudioprocess` is a property setter; the closure-as-property
    // pattern requires `as_ref().unchecked_ref()`.
    node.set_onaudioprocess(Some(closure.as_ref().unchecked_ref()));

    // Stash so the closure outlives this call.
    inner.borrow_mut().audio_closure = Some(closure);

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
