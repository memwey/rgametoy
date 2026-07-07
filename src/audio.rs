//! Real-time audio output backend (behind the `audio` feature).
//!
//! Bridges the APU's interleaved stereo sample stream to the host audio device
//! via cpal. The emulator drains samples on the main thread and queues them
//! here; a cpal callback on the audio thread pulls them out.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type SharedQueue = Arc<Mutex<VecDeque<f32>>>;

pub struct AudioPlayer {
    _stream: cpal::Stream,
    queue: SharedQueue,
    sample_rate: u32,
    channels: usize,
}

impl AudioPlayer {
    /// Open the default output device. Returns `None` if no device is
    /// available or the stream can't be built (the emulator then runs muted).
    pub fn new() -> Option<AudioPlayer> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let config = device.default_output_config().ok()?;

        let sample_format = config.sample_format();
        let sample_rate = config.sample_rate().0;
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
            sample_rate,
            channels,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Queue interleaved stereo samples produced by the APU, dropping them if
    /// playback has fallen far enough behind to build up ~1s of latency.
    pub fn queue(&self, samples: &[f32]) {
        let mut queue = self.queue.lock().unwrap();
        let max = self.sample_rate as usize * self.channels;
        if queue.len() < max {
            queue.extend(samples.iter().copied());
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    queue: SharedQueue,
    channels: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    device.build_output_stream(
        config,
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
        |err| eprintln!("audio stream error: {err}"),
        None,
    )
}
