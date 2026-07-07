//! Headless APU demo: plays a short plucky arpeggio on the square channel and
//! writes the result to a 16-bit stereo WAV file.
//!
//! Run with: `cargo run --example apu_wav -- out.wav`

use rgametoy::console::apu::{Apu, OUTPUT_RATE};
use std::fs::File;
use std::io::{BufWriter, Write};

const SAMPLE_RATE: u32 = OUTPUT_RATE;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "out.wav".to_string());

    let mut apu = Apu::new();
    apu.write_register(0xFF26, 0x80); // power on
    apu.write_register(0xFF24, 0x77); // NR50: max volume
    apu.write_register(0xFF25, 0xFF); // NR51: both sides
    apu.write_register(0xFF11, 0x80); // NR11: 50% duty

    // Frequency register = 2048 - 131072 / freq_hz.
    let notes = [
        523.25, 659.25, 783.99, 1046.50, // C E G C
        783.99, 659.25, 523.25, 0.0, // G E C rest
    ];

    let mut samples: Vec<f32> = Vec::new();
    for &hz in &notes {
        if hz > 0.0 {
            let reg = (2048.0 - 131072.0 / hz) as u16 & 0x7FF;
            apu.write_register(0xFF12, 0xF2); // volume 15, decay, period 2
            apu.write_register(0xFF13, (reg & 0xFF) as u8);
            apu.write_register(0xFF14, 0x80 | (reg >> 8) as u8); // trigger
        } else {
            apu.write_register(0xFF12, 0x00); // silence (DAC off)
            apu.write_register(0xFF14, 0x80);
        }
        // ~0.22 s per note.
        for _ in 0..46_000 {
            apu.tick(20);
        }
        samples.extend(apu.take_samples());
    }

    let peak = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    write_wav(&path, &samples).expect("write WAV");
    println!(
        "wrote {path}: {} stereo frames, peak amplitude {:.3}",
        samples.len() / 2,
        peak
    );
}

/// Minimal 16-bit PCM stereo WAV writer.
fn write_wav(path: &str, samples: &[f32]) -> std::io::Result<()> {
    let channels: u16 = 2;
    let bits: u16 = 16;
    let byte_rate = SAMPLE_RATE * channels as u32 * (bits / 8) as u32;
    let block_align = channels * (bits / 8);
    let data_bytes = (samples.len() * 2) as u32;

    let mut out = BufWriter::new(File::create(path)?);
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_bytes).to_le_bytes())?;
    out.write_all(b"WAVE")?;
    out.write_all(b"fmt ")?;
    out.write_all(&16u32.to_le_bytes())?; // fmt chunk size
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&channels.to_le_bytes())?;
    out.write_all(&SAMPLE_RATE.to_le_bytes())?;
    out.write_all(&byte_rate.to_le_bytes())?;
    out.write_all(&block_align.to_le_bytes())?;
    out.write_all(&bits.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_bytes.to_le_bytes())?;

    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.write_all(&v.to_le_bytes())?;
    }
    out.flush()
}
