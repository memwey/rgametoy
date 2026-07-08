//! Linear resampler for interleaved stereo at a fixed rate ratio (APU source
//! rate → device rate). Pure DSP, no WebAudio, so it sits apart from `audio`'s
//! browser glue and is unit-tested directly. Same algorithm as
//! `rgametoy-desktop`'s `Resampler`; reproduced here because that crate is
//! cpal-bound and we want this crate to compile to `wasm32` with no host code.

pub(crate) struct Resampler {
    step: f64,
    pos: f64,
    prev: (f32, f32),
    primed: bool,
}

impl Resampler {
    pub(crate) fn new(source_rate: u32, device_rate: u32) -> Resampler {
        Resampler {
            step: source_rate as f64 / device_rate as f64,
            pos: 0.0,
            prev: (0.0, 0.0),
            primed: false,
        }
    }

    pub(crate) fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
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

#[cfg(test)]
mod tests {
    use super::Resampler;

    /// Output stereo frames produced from `in_frames` input frames at a ratio.
    fn output_frames(source: u32, device: u32, in_frames: usize) -> usize {
        let mut r = Resampler::new(source, device);
        let input: Vec<f32> = (0..in_frames * 2).map(|i| i as f32).collect();
        let mut out = Vec::new();
        r.process(&input, &mut out);
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
