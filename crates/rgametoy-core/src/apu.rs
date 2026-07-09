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

#[cfg(feature = "serialize")]
use crate::state::{write_bool, write_u16_le, write_u32_le, write_u64_le, write_u8, Reader, SaveStateError};

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

/// The NRx4 length-enable write plus the "extra length clock" obscure behaviour
/// (dmg_sound tests 03/08/11): if a write *enables* the length counter (bit 6,
/// 0→1) while the frame sequencer is in the first half of the length period —
/// i.e. the next FS step will *not* clock length — the length counter is clocked
/// once immediately, and if it reaches 0 (and this isn't a trigger) the channel
/// is disabled. Shared by all four channels; the caller does the trigger (and,
/// for a length reloaded to max on trigger, one more immediate clock).
fn length_enable_write(
    length_counter: &mut u16,
    length_enabled: &mut bool,
    enabled: &mut bool,
    value: u8,
    first_half: bool,
) {
    let trigger = value & 0x80 != 0;
    let enable = value & 0x40 != 0;
    let was_enabled = *length_enabled;
    *length_enabled = enable;
    if enable && !was_enabled && first_half && *length_counter > 0 {
        *length_counter -= 1;
        if *length_counter == 0 && !trigger {
            *enabled = false;
        }
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
    freq_timer: u32,
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
    /// Whether a sweep calculation has run in negate mode since the last
    /// trigger. Clearing negate (NR10) after such a calc disables the channel
    /// (dmg_sound 05).
    sweep_neg_used: bool,
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
            sweep_neg_used: false,
        }
    }

    fn period(&self) -> u32 {
        u32::from(2048 - self.frequency) * 4
    }

    fn tick(&mut self, mut cycles: u32) {
        while cycles > 0 {
            if self.freq_timer > cycles {
                self.freq_timer -= cycles;
                break;
            }
            cycles -= self.freq_timer;
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
            self.sweep_neg_used = true;
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
            self.sweep_neg_used = false;
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
    freq_timer: u32,
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

    fn period(&self) -> u32 {
        u32::from(2048 - self.frequency) * 2
    }

    fn tick(&mut self, mut cycles: u32) {
        while cycles > 0 {
            if self.freq_timer > cycles {
                self.freq_timer -= cycles;
                break;
            }
            cycles -= self.freq_timer;
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
    /// A fresh, powered-off APU: all channels reset, sample buffer empty.
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

    /// Advance every channel and the 512 Hz frame sequencer by `t_cycles`
    /// T-cycles, appending any newly produced samples (at [`OUTPUT_RATE`]) to the
    /// output buffer. Nothing runs while the APU is powered off.
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

    /// Whether the frame sequencer is in the *first half* of a length period —
    /// its next step will not clock the length counters. Length is clocked on
    /// steps 0/2/4/6, and `frame_seq_step` is the step about to run, so the next
    /// step clocks length exactly when it is even; the first half is the odd
    /// steps. This gates the NRx4 extra-length-clock quirk ([`length_enable_write`]).
    fn length_first_half(&self) -> bool {
        self.frame_seq_step % 2 == 1
    }

    fn step_frame_sequencer(&mut self) {
        debug_assert!(self.frame_seq_step < 8, "frame sequencer has 8 steps");
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

    /// Read an APU register (0xFF10-0xFF3F). Unused/unreadable bits read as 1
    /// (per hardware); wave RAM reads straight through.
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

    /// Write an APU register (0xFF10-0xFF3F). While powered off most writes are
    /// ignored — only NR52, wave RAM, and the NRx1 length-load fields still land
    /// (DMG). Handles the obscure length/sweep/trigger side effects.
    pub fn write_register(&mut self, addr: u16, value: u8) {
        // Wave RAM and NR52 are writable even while powered off. On DMG the
        // length-load registers (NRx1) are too — but only their length field
        // takes effect while off (the duty/other bits don't); see the handlers.
        let length_load = matches!(addr, 0xFF11 | 0xFF16 | 0xFF1B | 0xFF20);
        if !(self.power || addr == 0xFF26 || length_load || (0xFF30..=0xFF3F).contains(&addr)) {
            return;
        }

        match addr {
            0xFF10 => {
                let was_negate = self.ch1.sweep_negate;
                self.ch1.sweep_period = (value >> 4) & 0x07;
                self.ch1.sweep_negate = value & 0x08 != 0;
                self.ch1.sweep_shift = value & 0x07;
                // Obscure: leaving negate mode after at least one negate-mode
                // sweep calculation disables the channel (dmg_sound 05).
                if was_negate && !self.ch1.sweep_negate && self.ch1.sweep_neg_used {
                    self.ch1.enabled = false;
                }
            }
            0xFF11 => {
                if self.power {
                    self.ch1.duty = value >> 6;
                }
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
                let first_half = self.length_first_half();
                length_enable_write(
                    &mut self.ch1.length_counter,
                    &mut self.ch1.length_enabled,
                    &mut self.ch1.enabled,
                    value,
                    first_half,
                );
                if value & 0x80 != 0 {
                    let reload = self.ch1.length_counter == 0;
                    self.ch1.trigger();
                    if reload && value & 0x40 != 0 && first_half {
                        self.ch1.length_counter -= 1;
                    }
                }
            }

            0xFF16 => {
                if self.power {
                    self.ch2.duty = value >> 6;
                }
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
                let first_half = self.length_first_half();
                length_enable_write(
                    &mut self.ch2.length_counter,
                    &mut self.ch2.length_enabled,
                    &mut self.ch2.enabled,
                    value,
                    first_half,
                );
                if value & 0x80 != 0 {
                    let reload = self.ch2.length_counter == 0;
                    self.ch2.trigger();
                    if reload && value & 0x40 != 0 && first_half {
                        self.ch2.length_counter -= 1;
                    }
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
                let first_half = self.length_first_half();
                length_enable_write(
                    &mut self.ch3.length_counter,
                    &mut self.ch3.length_enabled,
                    &mut self.ch3.enabled,
                    value,
                    first_half,
                );
                if value & 0x80 != 0 {
                    let reload = self.ch3.length_counter == 0;
                    self.ch3.trigger();
                    if reload && value & 0x40 != 0 && first_half {
                        self.ch3.length_counter -= 1;
                    }
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
                let first_half = self.length_first_half();
                length_enable_write(
                    &mut self.ch4.length_counter,
                    &mut self.ch4.length_enabled,
                    &mut self.ch4.enabled,
                    value,
                    first_half,
                );
                if value & 0x80 != 0 {
                    let reload = self.ch4.length_counter == 0;
                    self.ch4.trigger();
                    if reload && value & 0x40 != 0 && first_half {
                        self.ch4.length_counter -= 1;
                    }
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

    /// Reset all channels and control registers, preserving Wave RAM. On DMG
    /// the length *counters* also survive a power-off (dmg_sound 08/11) — only
    /// the rest of each channel is cleared — so we save and restore them.
    fn power_off(&mut self) {
        let wave_ram = self.ch3.wave_ram;
        let lengths = [
            self.ch1.length_counter,
            self.ch2.length_counter,
            self.ch3.length_counter,
            self.ch4.length_counter,
        ];
        self.ch1 = SquareChannel::new(true);
        self.ch2 = SquareChannel::new(false);
        self.ch3 = WaveChannel::new();
        self.ch3.wave_ram = wave_ram;
        self.ch4 = NoiseChannel::new();
        self.ch1.length_counter = lengths[0];
        self.ch2.length_counter = lengths[1];
        self.ch3.length_counter = lengths[2];
        self.ch4.length_counter = lengths[3];
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

// -- Save state -------------------------------------------------------------

impl Envelope {
    #[cfg(feature = "serialize")]
    fn write_state(&self, out: &mut Vec<u8>) {
        write_u8(out, self.start_volume);
        write_bool(out, self.add_mode);
        write_u8(out, self.period);
        write_u8(out, self.volume);
        write_u8(out, self.timer);
    }

    #[cfg(feature = "serialize")]
    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.start_volume = r.read_u8()?;
        self.add_mode = r.read_bool()?;
        self.period = r.read_u8()?;
        self.volume = r.read_u8()?;
        self.timer = r.read_u8()?;
        Ok(())
    }
}

impl SquareChannel {
    #[cfg(feature = "serialize")]
    fn write_state(&self, out: &mut Vec<u8>) {
        write_bool(out, self.enabled);
        write_bool(out, self.dac.enabled);
        write_u8(out, self.duty);
        write_u8(out, self.duty_pos);
        write_u16_le(out, self.frequency);
        write_u32_le(out, self.freq_timer);
        write_u16_le(out, self.length_counter);
        write_bool(out, self.length_enabled);
        self.env.write_state(out);
        write_bool(out, self.has_sweep);
        write_u8(out, self.sweep_period);
        write_bool(out, self.sweep_negate);
        write_u8(out, self.sweep_shift);
        write_u8(out, self.sweep_timer);
        write_bool(out, self.sweep_enabled);
        write_u16_le(out, self.sweep_shadow);
        write_bool(out, self.sweep_neg_used);
    }

    #[cfg(feature = "serialize")]
    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.enabled = r.read_bool()?;
        self.dac.enabled = r.read_bool()?;
        self.duty = r.read_u8()?;
        self.duty_pos = r.read_u8()?;
        self.frequency = r.read_u16_le()?;
        self.freq_timer = r.read_u32_le()?;
        self.length_counter = r.read_u16_le()?;
        self.length_enabled = r.read_bool()?;
        self.env.read_state(r)?;
        self.has_sweep = r.read_bool()?;
        self.sweep_period = r.read_u8()?;
        self.sweep_negate = r.read_bool()?;
        self.sweep_shift = r.read_u8()?;
        self.sweep_timer = r.read_u8()?;
        self.sweep_enabled = r.read_bool()?;
        self.sweep_shadow = r.read_u16_le()?;
        self.sweep_neg_used = r.read_bool()?;
        Ok(())
    }
}

impl WaveChannel {
    #[cfg(feature = "serialize")]
    fn write_state(&self, out: &mut Vec<u8>) {
        write_bool(out, self.enabled);
        write_bool(out, self.dac.enabled);
        write_u16_le(out, self.frequency);
        write_u32_le(out, self.freq_timer);
        write_u8(out, self.position);
        write_u8(out, self.sample_buffer);
        write_u8(out, self.volume_code);
        write_u16_le(out, self.length_counter);
        write_bool(out, self.length_enabled);
        out.extend_from_slice(&self.wave_ram);
    }

    #[cfg(feature = "serialize")]
    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.enabled = r.read_bool()?;
        self.dac.enabled = r.read_bool()?;
        self.frequency = r.read_u16_le()?;
        self.freq_timer = r.read_u32_le()?;
        self.position = r.read_u8()?;
        self.sample_buffer = r.read_u8()?;
        self.volume_code = r.read_u8()?;
        self.length_counter = r.read_u16_le()?;
        self.length_enabled = r.read_bool()?;
        self.wave_ram.copy_from_slice(r.read_exact(16)?);
        Ok(())
    }
}

impl NoiseChannel {
    #[cfg(feature = "serialize")]
    fn write_state(&self, out: &mut Vec<u8>) {
        write_bool(out, self.enabled);
        write_bool(out, self.dac.enabled);
        write_u16_le(out, self.lfsr);
        write_u8(out, self.clock_shift);
        write_bool(out, self.width_7bit);
        write_u8(out, self.divisor_code);
        write_u32_le(out, self.freq_timer);
        write_u16_le(out, self.length_counter);
        write_bool(out, self.length_enabled);
        self.env.write_state(out);
    }

    #[cfg(feature = "serialize")]
    fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.enabled = r.read_bool()?;
        self.dac.enabled = r.read_bool()?;
        self.lfsr = r.read_u16_le()?;
        self.clock_shift = r.read_u8()?;
        self.width_7bit = r.read_bool()?;
        self.divisor_code = r.read_u8()?;
        self.freq_timer = r.read_u32_le()?;
        self.length_counter = r.read_u16_le()?;
        self.length_enabled = r.read_bool()?;
        self.env.read_state(r)?;
        Ok(())
    }
}

impl Apu {
    /// Append the full APU state to `out`. The `cycles_per_sample` and
    /// `hp_factor` fields are currently computed at construction time from
    /// the constant `OUTPUT_RATE`, but we still serialize them so the blob
    /// stays bit-exact even if that constant ever becomes runtime-config.
    #[cfg(feature = "serialize")]
    pub fn write_state(&self, out: &mut Vec<u8>) {
        self.ch1.write_state(out);
        self.ch2.write_state(out);
        self.ch3.write_state(out);
        self.ch4.write_state(out);
        write_bool(out, self.power);
        write_u8(out, self.nr50);
        write_u8(out, self.nr51);
        write_u32_le(out, self.frame_seq_counter);
        write_u8(out, self.frame_seq_step);
        write_u64_le(out, self.sample_clock.to_bits());
        write_u64_le(out, self.cycles_per_sample.to_bits());
        write_u32_le(out, self.hp_factor.to_bits());
        write_u32_le(out, self.hp_cap_l.to_bits());
        write_u32_le(out, self.hp_cap_r.to_bits());
        // Interleaved (L, R) f32 sample buffer, length-prefixed because the
        // size depends on how long the frontend went between drains.
        write_u32_le(out, self.buffer.len() as u32);
        for &s in &self.buffer {
            write_u32_le(out, s.to_bits());
        }
    }

    #[cfg(feature = "serialize")]
    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.ch1.read_state(r)?;
        self.ch2.read_state(r)?;
        self.ch3.read_state(r)?;
        self.ch4.read_state(r)?;
        self.power = r.read_bool()?;
        self.nr50 = r.read_u8()?;
        self.nr51 = r.read_u8()?;
        self.frame_seq_counter = r.read_u32_le()?;
        self.frame_seq_step = r.read_u8()?;
        if self.frame_seq_step > 7 {
            return Err(SaveStateError::Corrupt); // the sequencer only has 8 steps
        }
        self.sample_clock = f64::from_bits(r.read_u64_le()?);
        self.cycles_per_sample = f64::from_bits(r.read_u64_le()?);
        self.hp_factor = f32::from_bits(r.read_u32_le()?);
        self.hp_cap_l = f32::from_bits(r.read_u32_le()?);
        self.hp_cap_r = f32::from_bits(r.read_u32_le()?);
        // The buffer never legitimately exceeds BUFFER_CAP. Reject a larger
        // count before it can overflow `n * 4` (usize is 32-bit on wasm32) or
        // force a giant `reserve`.
        let n = r.read_u32_le()? as usize;
        if n > BUFFER_CAP {
            return Err(SaveStateError::Corrupt);
        }
        let bytes = r.read_exact(n * 4)?;
        self.buffer.clear();
        self.buffer.reserve(n);
        for chunk in bytes.chunks_exact(4) {
            self.buffer
                .push(f32::from_bits(u32::from_le_bytes(chunk.try_into().unwrap())));
        }
        Ok(())
    }
}
