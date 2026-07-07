//! Audio Processing Unit (DMG).
//!
//! Emulates the four sound channels — two square waves (channel 1 with a
//! frequency sweep), a programmable wave channel and a noise channel — plus the
//! 512 Hz frame sequencer that clocks their length counters, volume envelopes
//! and sweep. Channels are mixed through their DACs, panned (NR51) and scaled
//! by the master volume (NR50), then resampled to a target rate and pushed into
//! a stereo sample buffer that an audio backend can drain.
//!
//! This module is pure logic with no I/O; actual playback lives behind the
//! optional `audio` feature (see `audio.rs`).

const CPU_HZ: f64 = 4_194_304.0;

/// Fixed, device-independent rate (Hz) at which the APU emits samples. The
/// frontend resamples this stream to the actual audio device's rate — the core
/// itself knows nothing about the output device.
pub const OUTPUT_RATE: u32 = 48000;

/// T-cycles per 512 Hz frame-sequencer step.
const FRAME_SEQ_PERIOD: u32 = 8192;
/// Cap on buffered interleaved samples, so the buffer stays bounded even if no
/// backend drains it.
const BUFFER_CAP: usize = 65536;

const DUTY: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1], // 12.5%
    [1, 0, 0, 0, 0, 0, 0, 1], // 25%
    [1, 0, 0, 0, 0, 1, 1, 1], // 50%
    [0, 1, 1, 1, 1, 1, 1, 0], // 75%
];

const NOISE_DIVISORS: [u32; 8] = [8, 16, 32, 48, 64, 80, 96, 112];

/// A per-channel digital-to-analog converter, mirroring the four independent
/// DACs in the DMG sound hardware. It converts a channel's 4-bit digital value
/// (0-15) into an analog sample in [-1, 1].
///
/// The DAC enable is separate from the channel's on/off state: a disabled DAC
/// contributes silence, while an *enabled* DAC fed a 0 value (e.g. a channel
/// switched off) sits at +1.0 — a DC level the output high-pass then removes.
#[derive(Clone)]
struct Dac {
    enabled: bool,
}

impl Dac {
    fn new() -> Dac {
        Dac { enabled: false }
    }

    fn output(&self, digital: u8) -> f32 {
        if self.enabled {
            1.0 - digital as f32 / 7.5
        } else {
            0.0
        }
    }
}

// ---------------------------------------------------------------------------
// Volume envelope (shared by the square and noise channels)
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct Envelope {
    start_volume: u8,
    add_mode: bool,
    period: u8,
    volume: u8,
    timer: u8,
}

impl Envelope {
    fn trigger(&mut self) {
        self.volume = self.start_volume;
        self.timer = self.period;
    }

    fn clock(&mut self) {
        if self.period == 0 {
            return;
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = self.period;
            if self.add_mode && self.volume < 15 {
                self.volume += 1;
            } else if !self.add_mode && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }

    /// A channel's DAC is powered by the upper 5 bits of NRx2.
    fn dac_enabled(&self) -> bool {
        self.start_volume != 0 || self.add_mode
    }

    fn read_nrx2(&self) -> u8 {
        (self.start_volume << 4) | ((self.add_mode as u8) << 3) | self.period
    }

    fn write_nrx2(&mut self, value: u8) {
        self.start_volume = value >> 4;
        self.add_mode = value & 0x08 != 0;
        self.period = value & 0x07;
    }
}

// ---------------------------------------------------------------------------
// Square channel (channels 1 and 2; channel 1 also has a sweep unit)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct SquareChannel {
    enabled: bool,
    dac: Dac,
    duty: u8,
    duty_pos: u8,
    frequency: u16,
    freq_timer: u16,
    length_counter: u16,
    length_enabled: bool,
    env: Envelope,

    has_sweep: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_timer: u8,
    sweep_enabled: bool,
    sweep_shadow: u16,
}

impl SquareChannel {
    fn new(has_sweep: bool) -> SquareChannel {
        SquareChannel {
            enabled: false,
            dac: Dac::new(),
            duty: 0,
            duty_pos: 0,
            frequency: 0,
            freq_timer: 8192,
            length_counter: 0,
            length_enabled: false,
            env: Envelope::default(),
            has_sweep,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_timer: 0,
            sweep_enabled: false,
            sweep_shadow: 0,
        }
    }

    fn period(&self) -> u16 {
        (2048 - self.frequency) * 4
    }

    fn tick(&mut self, mut cycles: u32) {
        while cycles > 0 {
            if self.freq_timer as u32 > cycles {
                self.freq_timer -= cycles as u16;
                break;
            }
            cycles -= self.freq_timer as u32;
            self.freq_timer = self.period();
            self.duty_pos = (self.duty_pos + 1) & 7;
        }
    }

    fn clock_length(&mut self) {
        if self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    fn clock_sweep(&mut self) {
        if !self.has_sweep {
            return;
        }
        if self.sweep_timer > 0 {
            self.sweep_timer -= 1;
        }
        if self.sweep_timer == 0 {
            self.sweep_timer = if self.sweep_period != 0 { self.sweep_period } else { 8 };
            if self.sweep_enabled && self.sweep_period != 0 {
                let new_freq = self.sweep_calc();
                if new_freq <= 2047 && self.sweep_shift != 0 {
                    self.frequency = new_freq;
                    self.sweep_shadow = new_freq;
                    self.sweep_calc(); // second overflow check
                }
            }
        }
    }

    /// Compute the next sweep frequency; overflow disables the channel.
    fn sweep_calc(&mut self) -> u16 {
        let delta = self.sweep_shadow >> self.sweep_shift;
        let new_freq = if self.sweep_negate {
            self.sweep_shadow.wrapping_sub(delta)
        } else {
            self.sweep_shadow + delta
        };
        if new_freq > 2047 {
            self.enabled = false;
        }
        new_freq
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }
        self.freq_timer = self.period();
        self.env.trigger();
        if self.has_sweep {
            self.sweep_shadow = self.frequency;
            self.sweep_timer = if self.sweep_period != 0 { self.sweep_period } else { 8 };
            self.sweep_enabled = self.sweep_period != 0 || self.sweep_shift != 0;
            if self.sweep_shift != 0 {
                self.sweep_calc();
            }
        }
        if !self.dac.enabled {
            self.enabled = false;
        }
    }

    fn dac_output(&self) -> f32 {
        let digital = if self.enabled && DUTY[self.duty as usize][self.duty_pos as usize] == 1 {
            self.env.volume
        } else {
            0
        };
        self.dac.output(digital)
    }
}

// ---------------------------------------------------------------------------
// Wave channel (channel 3)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct WaveChannel {
    enabled: bool,
    dac: Dac,
    frequency: u16,
    freq_timer: u16,
    position: u8,
    sample_buffer: u8,
    volume_code: u8,
    length_counter: u16,
    length_enabled: bool,
    wave_ram: [u8; 16],
}

impl WaveChannel {
    fn new() -> WaveChannel {
        WaveChannel {
            enabled: false,
            dac: Dac::new(),
            frequency: 0,
            freq_timer: 4096,
            position: 0,
            sample_buffer: 0,
            volume_code: 0,
            length_counter: 0,
            length_enabled: false,
            wave_ram: [0; 16],
        }
    }

    fn period(&self) -> u16 {
        (2048 - self.frequency) * 2
    }

    fn tick(&mut self, mut cycles: u32) {
        while cycles > 0 {
            if self.freq_timer as u32 > cycles {
                self.freq_timer -= cycles as u16;
                break;
            }
            cycles -= self.freq_timer as u32;
            self.freq_timer = self.period();
            self.position = (self.position + 1) & 31;
            let byte = self.wave_ram[(self.position / 2) as usize];
            self.sample_buffer = if self.position & 1 == 0 { byte >> 4 } else { byte & 0x0F };
        }
    }

    fn clock_length(&mut self) {
        if self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 256;
        }
        self.freq_timer = self.period();
        self.position = 0;
        if !self.dac.enabled {
            self.enabled = false;
        }
    }

    fn dac_output(&self) -> f32 {
        let digital = if self.enabled {
            let shift = match self.volume_code {
                1 => 0,
                2 => 1,
                3 => 2,
                _ => 4, // 0 => mute
            };
            self.sample_buffer >> shift
        } else {
            0
        };
        self.dac.output(digital)
    }
}

// ---------------------------------------------------------------------------
// Noise channel (channel 4)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct NoiseChannel {
    enabled: bool,
    dac: Dac,
    lfsr: u16,
    clock_shift: u8,
    width_7bit: bool,
    divisor_code: u8,
    freq_timer: u32,
    length_counter: u16,
    length_enabled: bool,
    env: Envelope,
}

impl NoiseChannel {
    fn new() -> NoiseChannel {
        NoiseChannel {
            enabled: false,
            dac: Dac::new(),
            lfsr: 0x7FFF,
            clock_shift: 0,
            width_7bit: false,
            divisor_code: 0,
            freq_timer: 8,
            length_counter: 0,
            length_enabled: false,
            env: Envelope::default(),
        }
    }

    fn period(&self) -> u32 {
        NOISE_DIVISORS[self.divisor_code as usize] << self.clock_shift
    }

    fn tick(&mut self, mut cycles: u32) {
        while cycles > 0 {
            if self.freq_timer > cycles {
                self.freq_timer -= cycles;
                break;
            }
            cycles -= self.freq_timer;
            self.freq_timer = self.period();
            self.clock_lfsr();
        }
    }

    fn clock_lfsr(&mut self) {
        let xor = (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1);
        self.lfsr >>= 1;
        self.lfsr |= xor << 14;
        if self.width_7bit {
            self.lfsr &= !(1 << 6);
            self.lfsr |= xor << 6;
        }
    }

    fn clock_length(&mut self) {
        if self.length_enabled && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }
        self.freq_timer = self.period();
        self.lfsr = 0x7FFF;
        self.env.trigger();
        if !self.dac.enabled {
            self.enabled = false;
        }
    }

    fn dac_output(&self) -> f32 {
        let digital = if self.enabled && (self.lfsr & 1) == 0 {
            self.env.volume
        } else {
            0
        };
        self.dac.output(digital)
    }
}

// ---------------------------------------------------------------------------
// APU
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Apu {
    ch1: SquareChannel,
    ch2: SquareChannel,
    ch3: WaveChannel,
    ch4: NoiseChannel,

    power: bool,
    nr50: u8, // master volume + VIN
    nr51: u8, // panning

    frame_seq_counter: u32,
    frame_seq_step: u8,

    sample_clock: f64,
    cycles_per_sample: f64,
    hp_factor: f32,
    hp_cap_l: f32,
    hp_cap_r: f32,
    buffer: Vec<f32>,
}

impl Apu {
    pub fn new() -> Apu {
        Apu {
            ch1: SquareChannel::new(true),
            ch2: SquareChannel::new(false),
            ch3: WaveChannel::new(),
            ch4: NoiseChannel::new(),
            power: false,
            nr50: 0,
            nr51: 0,
            frame_seq_counter: 0,
            frame_seq_step: 0,
            sample_clock: 0.0,
            // T-cycles between emitted samples, and the DMG high-pass capacitor
            // factor, both fixed to the core's device-independent OUTPUT_RATE.
            cycles_per_sample: CPU_HZ / OUTPUT_RATE as f64,
            hp_factor: 0.999958_f32.powf(CPU_HZ as f32 / OUTPUT_RATE as f32),
            hp_cap_l: 0.0,
            hp_cap_r: 0.0,
            buffer: Vec::new(),
        }
    }

    /// The (fixed) rate at which [`Apu::take_samples`] produces samples.
    pub fn output_rate(&self) -> u32 {
        OUTPUT_RATE
    }

    /// Remove and return all buffered interleaved (L, R, L, R, …) samples,
    /// at [`OUTPUT_RATE`]. The frontend resamples these to its device rate.
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.buffer)
    }

    pub fn tick(&mut self, t_cycles: u8) {
        let cycles = t_cycles as u32;

        if self.power {
            self.ch1.tick(cycles);
            self.ch2.tick(cycles);
            self.ch3.tick(cycles);
            self.ch4.tick(cycles);

            self.frame_seq_counter += cycles;
            while self.frame_seq_counter >= FRAME_SEQ_PERIOD {
                self.frame_seq_counter -= FRAME_SEQ_PERIOD;
                self.step_frame_sequencer();
            }
        }

        self.sample_clock += cycles as f64;
        while self.sample_clock >= self.cycles_per_sample {
            self.sample_clock -= self.cycles_per_sample;
            self.emit_sample();
        }
    }

    fn step_frame_sequencer(&mut self) {
        match self.frame_seq_step {
            0 | 4 => self.clock_length(),
            2 | 6 => {
                self.clock_length();
                self.ch1.clock_sweep();
            }
            7 => {
                self.ch1.env.clock();
                self.ch2.env.clock();
                self.ch4.env.clock();
            }
            _ => {}
        }
        self.frame_seq_step = (self.frame_seq_step + 1) & 7;
    }

    fn clock_length(&mut self) {
        self.ch1.clock_length();
        self.ch2.clock_length();
        self.ch3.clock_length();
        self.ch4.clock_length();
    }

    fn emit_sample(&mut self) {
        let (mut left, mut right) = (0.0f32, 0.0f32);
        let outputs = [
            self.ch1.dac_output(),
            self.ch2.dac_output(),
            self.ch3.dac_output(),
            self.ch4.dac_output(),
        ];
        for (i, &out) in outputs.iter().enumerate() {
            if self.nr51 & (0x10 << i) != 0 {
                left += out;
            }
            if self.nr51 & (0x01 << i) != 0 {
                right += out;
            }
        }

        let left_vol = ((self.nr50 >> 4) & 0x07) as f32 + 1.0;
        let right_vol = (self.nr50 & 0x07) as f32 + 1.0;
        left = left / 4.0 * (left_vol / 8.0);
        right = right / 4.0 * (right_vol / 8.0);

        // High-pass filter to remove the DAC DC offset.
        let out_l = left - self.hp_cap_l;
        self.hp_cap_l = left - out_l * self.hp_factor;
        let out_r = right - self.hp_cap_r;
        self.hp_cap_r = right - out_r * self.hp_factor;

        if self.buffer.len() < BUFFER_CAP {
            self.buffer.push(out_l);
            self.buffer.push(out_r);
        }
    }

    // --- Register access ---------------------------------------------------

    pub fn read_register(&self, addr: u16) -> u8 {
        match addr {
            0xFF10 => {
                0x80 | (self.ch1.sweep_period << 4)
                    | ((self.ch1.sweep_negate as u8) << 3)
                    | self.ch1.sweep_shift
            }
            0xFF11 => (self.ch1.duty << 6) | 0x3F,
            0xFF12 => self.ch1.env.read_nrx2(),
            0xFF13 => 0xFF,
            0xFF14 => ((self.ch1.length_enabled as u8) << 6) | 0xBF,

            0xFF16 => (self.ch2.duty << 6) | 0x3F,
            0xFF17 => self.ch2.env.read_nrx2(),
            0xFF18 => 0xFF,
            0xFF19 => ((self.ch2.length_enabled as u8) << 6) | 0xBF,

            0xFF1A => ((self.ch3.dac.enabled as u8) << 7) | 0x7F,
            0xFF1B => 0xFF,
            0xFF1C => (self.ch3.volume_code << 5) | 0x9F,
            0xFF1D => 0xFF,
            0xFF1E => ((self.ch3.length_enabled as u8) << 6) | 0xBF,

            0xFF20 => 0xFF,
            0xFF21 => self.ch4.env.read_nrx2(),
            0xFF22 => {
                (self.ch4.clock_shift << 4)
                    | ((self.ch4.width_7bit as u8) << 3)
                    | self.ch4.divisor_code
            }
            0xFF23 => ((self.ch4.length_enabled as u8) << 6) | 0xBF,

            0xFF24 => self.nr50,
            0xFF25 => self.nr51,
            0xFF26 => {
                0x70 | ((self.power as u8) << 7)
                    | (self.ch1.enabled as u8)
                    | ((self.ch2.enabled as u8) << 1)
                    | ((self.ch3.enabled as u8) << 2)
                    | ((self.ch4.enabled as u8) << 3)
            }
            0xFF30..=0xFF3F => self.ch3.wave_ram[(addr - 0xFF30) as usize],
            _ => 0xFF,
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        // Wave RAM and NR52 are writable even while powered off; the other
        // registers ignore writes when the APU is off.
        if !(self.power || addr == 0xFF26 || (0xFF30..=0xFF3F).contains(&addr)) {
            return;
        }

        match addr {
            0xFF10 => {
                self.ch1.sweep_period = (value >> 4) & 0x07;
                self.ch1.sweep_negate = value & 0x08 != 0;
                self.ch1.sweep_shift = value & 0x07;
            }
            0xFF11 => {
                self.ch1.duty = value >> 6;
                self.ch1.length_counter = 64 - (value & 0x3F) as u16;
            }
            0xFF12 => {
                self.ch1.env.write_nrx2(value);
                self.ch1.dac.enabled = self.ch1.env.dac_enabled();
                if !self.ch1.dac.enabled {
                    self.ch1.enabled = false;
                }
            }
            0xFF13 => self.ch1.frequency = (self.ch1.frequency & 0x700) | value as u16,
            0xFF14 => {
                self.ch1.frequency = (self.ch1.frequency & 0xFF) | (((value & 0x07) as u16) << 8);
                self.ch1.length_enabled = value & 0x40 != 0;
                if value & 0x80 != 0 {
                    self.ch1.trigger();
                }
            }

            0xFF16 => {
                self.ch2.duty = value >> 6;
                self.ch2.length_counter = 64 - (value & 0x3F) as u16;
            }
            0xFF17 => {
                self.ch2.env.write_nrx2(value);
                self.ch2.dac.enabled = self.ch2.env.dac_enabled();
                if !self.ch2.dac.enabled {
                    self.ch2.enabled = false;
                }
            }
            0xFF18 => self.ch2.frequency = (self.ch2.frequency & 0x700) | value as u16,
            0xFF19 => {
                self.ch2.frequency = (self.ch2.frequency & 0xFF) | (((value & 0x07) as u16) << 8);
                self.ch2.length_enabled = value & 0x40 != 0;
                if value & 0x80 != 0 {
                    self.ch2.trigger();
                }
            }

            0xFF1A => {
                self.ch3.dac.enabled = value & 0x80 != 0;
                if !self.ch3.dac.enabled {
                    self.ch3.enabled = false;
                }
            }
            0xFF1B => self.ch3.length_counter = 256 - value as u16,
            0xFF1C => self.ch3.volume_code = (value >> 5) & 0x03,
            0xFF1D => self.ch3.frequency = (self.ch3.frequency & 0x700) | value as u16,
            0xFF1E => {
                self.ch3.frequency = (self.ch3.frequency & 0xFF) | (((value & 0x07) as u16) << 8);
                self.ch3.length_enabled = value & 0x40 != 0;
                if value & 0x80 != 0 {
                    self.ch3.trigger();
                }
            }

            0xFF20 => self.ch4.length_counter = 64 - (value & 0x3F) as u16,
            0xFF21 => {
                self.ch4.env.write_nrx2(value);
                self.ch4.dac.enabled = self.ch4.env.dac_enabled();
                if !self.ch4.dac.enabled {
                    self.ch4.enabled = false;
                }
            }
            0xFF22 => {
                self.ch4.clock_shift = value >> 4;
                self.ch4.width_7bit = value & 0x08 != 0;
                self.ch4.divisor_code = value & 0x07;
            }
            0xFF23 => {
                self.ch4.length_enabled = value & 0x40 != 0;
                if value & 0x80 != 0 {
                    self.ch4.trigger();
                }
            }

            0xFF24 => self.nr50 = value,
            0xFF25 => self.nr51 = value,
            0xFF26 => {
                let power = value & 0x80 != 0;
                if power && !self.power {
                    self.frame_seq_step = 0;
                    self.frame_seq_counter = 0;
                    self.power = true;
                } else if !power && self.power {
                    self.power_off();
                }
            }
            0xFF30..=0xFF3F => self.ch3.wave_ram[(addr - 0xFF30) as usize] = value,
            _ => {}
        }
    }

    /// Reset all channels and control registers, preserving Wave RAM.
    fn power_off(&mut self) {
        let wave_ram = self.ch3.wave_ram;
        self.ch1 = SquareChannel::new(true);
        self.ch2 = SquareChannel::new(false);
        self.ch3 = WaveChannel::new();
        self.ch3.wave_ram = wave_ram;
        self.ch4 = NoiseChannel::new();
        self.nr50 = 0;
        self.nr51 = 0;
        self.power = false;
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}
