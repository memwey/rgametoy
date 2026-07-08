//! Real-time audio output backend (behind the `audio` feature).
//!
//! Bridges the APU's device-independent sample stream to the host audio device
//! via cpal. The APU emits interleaved stereo at a fixed rate; this module
//! resamples it (linear interpolation, a fixed source→device ratio) to the
//! device's rate and feeds a cpal callback. The emulator only pushes samples at
//! normal speed, so the ratio never varies.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type SharedQueue = Arc<Mutex<VecDeque<f32>>>;

/// Stateful linear resampler for interleaved stereo, at a fixed rate ratio.
struct Resampler {
    /// Source frames to advance per output frame (source_rate / device_rate).
    step: f64,
    /// Sub-frame position in [0, 1) between `prev` and the current frame.
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

    /// Resample interleaved stereo `input` (source rate) into interleaved
    /// stereo `out` (device rate).
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

pub struct AudioPlayer {
    _stream: cpal::Stream,
    queue: SharedQueue,
    resampler: Resampler,
    /// Reused across frames so `queue` allocates nothing on the hot path.
    resampled: Vec<f32>,
    device_rate: u32,
}

impl AudioPlayer {
    /// Open the default output device, resampling from `source_rate` (the APU's
    /// output rate) to the device rate. Returns `None` if no device is
    /// available (the emulator then runs muted).
    pub fn new(source_rate: u32) -> Option<AudioPlayer> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let config = device.default_output_config().ok()?;

        let sample_format = config.sample_format();
        // cpal 0.18: SampleRate is a plain u32 alias (no more newtype `.0`).
        let device_rate = config.sample_rate();
        let channels = config.channels() as usize;
        let stream_config: cpal::StreamConfig = config.into();

        let queue: SharedQueue = Arc::new(Mutex::new(VecDeque::new()));

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                build_stream::<f32>(&device, &stream_config, Arc::clone(&queue), channels)
            }
            cpal::SampleFormat::I16 => {
                build_stream::<i16>(&device, &stream_config, Arc::clone(&queue), channels)
            }
            cpal::SampleFormat::U16 => {
                build_stream::<u16>(&device, &stream_config, Arc::clone(&queue), channels)
            }
            _ => return None,
        }
        .ok()?;

        stream.play().ok()?;

        Some(AudioPlayer {
            _stream: stream,
            queue,
            resampler: Resampler::new(source_rate, device_rate),
            resampled: Vec::new(),
            device_rate,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.device_rate
    }

    /// Resample APU samples to the device rate and queue them, dropping them if
    /// playback has fallen ~1 s behind.
    pub fn queue(&mut self, samples: &[f32]) {
        self.resampled.clear();
        self.resampler.process(samples, &mut self.resampled);

        let mut queue = self.queue.lock().unwrap();
        let max = self.device_rate as usize * 2; // ~1 s of interleaved stereo
        if queue.len() < max {
            queue.extend(self.resampled.iter().copied());
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    queue: SharedQueue,
    channels: usize,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    // cpal 0.18: StreamConfig is passed by value (it is `Copy`) and the build
    // error is the unified `cpal::Error`.
    device.build_output_stream(
        *config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let mut queue = queue.lock().unwrap();
            for frame in data.chunks_mut(channels.max(1)) {
                let left = queue.pop_front().unwrap_or(0.0);
                let right = queue.pop_front().unwrap_or(left);
                if channels == 1 {
                    frame[0] = T::from_sample(0.5 * (left + right));
                } else {
                    frame[0] = T::from_sample(left);
                    frame[1] = T::from_sample(right);
                    for sample in frame.iter_mut().skip(2) {
                        *sample = T::from_sample(0.0);
                    }
                }
            }
        },
        |err| crate::emulator::log::error(&format!("audio stream: {err}")),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::Resampler;

    /// Number of output frames from `in_frames` input frames at a rate ratio.
    fn output_frames(source: u32, device: u32, in_frames: usize) -> usize {
        let mut resampler = Resampler::new(source, device);
        let input: Vec<f32> = (0..in_frames * 2).map(|i| i as f32).collect();
        let mut out = Vec::new();
        resampler.process(&input, &mut out);
        out.len() / 2
    }

    #[test]
    fn equal_rate_is_one_to_one() {
        assert_eq!(output_frames(48000, 48000, 1000), 1000);
    }

    #[test]
    fn downsample_halves_the_frames() {
        let n = output_frames(48000, 24000, 1000) as i32;
        assert!((n - 500).abs() <= 1, "got {n}");
    }

    #[test]
    fn upsample_doubles_the_frames() {
        let n = output_frames(24000, 48000, 1000) as i32;
        assert!((n - 2000).abs() <= 2, "got {n}");
    }
}
