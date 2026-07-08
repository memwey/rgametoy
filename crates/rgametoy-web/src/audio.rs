//! WebAudio output via an **AudioWorklet**. The emulator runs on the main
//! thread (the rAF loop); each frame it hands its APU output to this sink, which
//! resamples to the device rate and `postMessage`s it to a tiny ring-buffer
//! processor running on the browser's audio render thread. The worklet plays it
//! out on its own thread, so audio survives main-thread jank far better than the
//! old (deprecated, main-thread) `ScriptProcessorNode`.
//!
//! Unlike that version, this module is a *pure output sink*: it does not drive
//! the emulator and never touches `Inner`, so there is no reentrancy hazard. The
//! drive model is unified — the rAF loop paces emulation against the wall clock
//! whether or not audio is on (see `wasm_host::Inner::step_and_present`).

use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioWorkletNode, AudioWorkletNodeOptions, Blob, BlobPropertyBag, MessagePort,
    Url,
};

use crate::resample::Resampler;

/// The audio-thread processor: a bounded ring buffer of interleaved stereo,
/// fed device-rate chunks over the port and drained 128 frames per `process()`.
/// On overflow it drops the oldest samples (latency stays bounded); on underrun
/// it outputs silence. Inlined and loaded via a blob URL so no separate asset
/// has to be served.
const WORKLET_JS: &str = r#"
class RgametoyAudio extends AudioWorkletProcessor {
  constructor() {
    super();
    this.cap = 16384;            // interleaved floats (~170 ms stereo @ 48 kHz)
    this.buf = new Float32Array(this.cap);
    this.read = 0;
    this.count = 0;              // interleaved floats currently buffered
    this.port.onmessage = (e) => {
      const chunk = e.data;
      let n = chunk.length;
      let start = 0;
      if (n > this.cap) { start = n - this.cap; n = this.cap; } // keep the tail
      const overflow = this.count + n - this.cap;
      if (overflow > 0) { this.read = (this.read + overflow) % this.cap; this.count -= overflow; }
      for (let i = start; i < chunk.length; i++) {
        this.buf[(this.read + this.count) % this.cap] = chunk[i];
        this.count++;
      }
    };
  }
  process(inputs, outputs) {
    const out = outputs[0];
    const left = out[0], right = out[1];
    const frames = left.length;
    for (let i = 0; i < frames; i++) {
      if (this.count >= 2) {
        left[i] = this.buf[this.read]; this.read = (this.read + 1) % this.cap;
        right[i] = this.buf[this.read]; this.read = (this.read + 1) % this.cap;
        this.count -= 2;
      } else {
        left[i] = 0; right[i] = 0;
      }
    }
    return true;
  }
}
registerProcessor('rgametoy-audio', RgametoyAudio);
"#;

/// The live audio graph: an `AudioContext`, the worklet node (kept alive), its
/// message port, and the resampler that maps the APU's fixed rate to the
/// device rate. Held in `Inner::audio` for the page's lifetime.
pub struct AudioPlayer {
    ctx: AudioContext,
    port: MessagePort,
    resampler: Resampler,
    resampled: Vec<f32>,
    source_rate: u32,
    device_rate: u32,
    // Kept alive so the audio node isn't GC'd; not otherwise read.
    #[allow(dead_code)]
    node: AudioWorkletNode,
}

/// Bring up the audio graph. Async because `AudioWorklet.addModule` returns a
/// promise. Does not drive the emulator and never touches `Inner` — no
/// reentrancy hazard. The context starts suspended (browsers gate audio on a
/// user gesture); the caller [`resume`](AudioPlayer::resume)s it from the click.
pub async fn enable(source_rate: u32) -> Result<AudioPlayer, JsValue> {
    let ctx = AudioContext::new()?;
    let device_rate = (ctx.sample_rate() as u32).max(1);
    let source_rate = source_rate.max(1);

    // Load the worklet module from an inline blob URL (no separately served file).
    let parts = js_sys::Array::of1(&JsValue::from_str(WORKLET_JS));
    let bag = BlobPropertyBag::new();
    bag.set_type("text/javascript");
    let blob = Blob::new_with_str_sequence_and_options(&parts, &bag)?;
    let url = Url::create_object_url_with_blob(&blob)?;
    let add = ctx.audio_worklet()?.add_module(&url)?;
    let load = JsFuture::from(add).await;
    let _ = Url::revoke_object_url(&url);
    load?;

    // Stereo source node (no inputs, one 2-channel output).
    let opts = AudioWorkletNodeOptions::new();
    let chans = js_sys::Array::of1(&JsValue::from_f64(2.0));
    opts.set_output_channel_count(chans.as_ref());
    let node = AudioWorkletNode::new_with_options(&ctx, "rgametoy-audio", &opts)?;
    node.connect_with_audio_node(&ctx.destination())?;
    let port = node.port()?;

    Ok(AudioPlayer {
        resampler: Resampler::new(source_rate, device_rate),
        resampled: Vec::new(),
        source_rate,
        device_rate,
        port,
        node,
        ctx,
    })
}

impl AudioPlayer {
    /// Resample this frame's APU output (interleaved stereo at the source rate)
    /// to the device rate and post it to the worklet. Called once per emulated
    /// frame by the rAF loop while audio is on and not fast-forwarding.
    pub fn feed(&mut self, apu_samples: &[f32]) {
        if self.source_rate == 0 || self.device_rate == 0 || apu_samples.is_empty() {
            return;
        }
        self.resampled.clear();
        self.resampler.process(apu_samples, &mut self.resampled);
        if self.resampled.is_empty() {
            return;
        }
        let arr = js_sys::Float32Array::from(self.resampled.as_slice());
        let _ = self.port.post_message(arr.as_ref());
    }

    /// Resume the AudioContext (from a user gesture — required to start audio).
    pub fn resume(&self) -> Result<(), JsValue> {
        let _ = self.ctx.resume()?;
        Ok(())
    }

    /// Suspend audio output (on disable / tab blur). Emulation keeps running.
    pub fn suspend(&self) -> Result<(), JsValue> {
        let _ = self.ctx.suspend()?;
        Ok(())
    }
}
