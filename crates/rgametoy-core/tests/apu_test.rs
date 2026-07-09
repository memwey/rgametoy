extern crate rgametoy_core;

use rgametoy_core::apu::Apu;

/// Powered-on APU at full master volume with every channel panned to both
/// sides.
fn powered_apu() -> Apu {
    let mut apu = Apu::new();
    apu.write_register(0xFF26, 0x80); // NR52: power on
    apu.write_register(0xFF24, 0x77); // NR50: max L/R volume
    apu.write_register(0xFF25, 0xFF); // NR51: all channels L+R
    apu
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn run_cycles(apu: &mut Apu, count: usize) {
    for _ in 0..count {
        apu.tick(20);
    }
}

#[test]
fn square_channel_produces_tone() {
    let mut apu = powered_apu();
    apu.write_register(0xFF11, 0x80); // NR11: 50% duty
    apu.write_register(0xFF12, 0xF0); // NR12: volume 15, DAC on, no envelope
    apu.write_register(0xFF13, 0x00); // NR13: frequency low
    apu.write_register(0xFF14, 0x87); // NR14: trigger, frequency high = 7

    run_cycles(&mut apu, 1000); // ~20k T-cycles
    let samples = apu.take_samples();

    assert!(!samples.is_empty(), "samples were produced");
    assert!(
        samples.iter().any(|&s| s.abs() > 0.01),
        "a non-silent tone is generated"
    );
    // NR52 reports channel 1 active.
    assert_eq!(apu.read_register(0xFF26) & 0x01, 0x01);
}

#[test]
fn length_counter_disables_channel() {
    let mut apu = powered_apu();
    apu.write_register(0xFF11, 0xBF); // duty + length load 63 -> counter = 1
    apu.write_register(0xFF12, 0xF0); // DAC on
    apu.write_register(0xFF13, 0x00);
    apu.write_register(0xFF14, 0xC7); // trigger + length enabled

    assert_eq!(apu.read_register(0xFF26) & 0x01, 0x01, "enabled after trigger");

    // Length clocks at 256 Hz; a single tick of the counter (=1) disables it.
    run_cycles(&mut apu, 4000); // ~80k T-cycles, several length clocks
    assert_eq!(
        apu.read_register(0xFF26) & 0x01,
        0x00,
        "channel disabled once the length counter expires"
    );
}

#[test]
fn envelope_decreases_volume() {
    let mut apu = powered_apu();
    apu.write_register(0xFF11, 0x80);
    apu.write_register(0xFF12, 0xF1); // volume 15, decrease, period 1
    apu.write_register(0xFF13, 0x00);
    apu.write_register(0xFF14, 0x87); // trigger, length disabled

    run_cycles(&mut apu, 2000);
    let early = apu.take_samples();

    run_cycles(&mut apu, 50_000); // let the envelope ramp to zero
    let _ = apu.take_samples();
    run_cycles(&mut apu, 2000);
    let late = apu.take_samples();

    assert!(
        rms(&late) < rms(&early) * 0.5,
        "output quietens as the envelope decays (early={}, late={})",
        rms(&early),
        rms(&late)
    );
}

#[test]
fn wave_channel_produces_output() {
    let mut apu = powered_apu();
    apu.write_register(0xFF1A, 0x80); // NR30: DAC on
    // Wave RAM: first half loud (nibbles = 15), second half silent.
    for addr in 0xFF30..0xFF38 {
        apu.write_register(addr, 0xFF);
    }
    for addr in 0xFF38..0xFF40 {
        apu.write_register(addr, 0x00);
    }
    apu.write_register(0xFF1C, 0x20); // NR32: 100% volume
    apu.write_register(0xFF1D, 0x00); // NR33: frequency low
    apu.write_register(0xFF1E, 0x87); // NR34: trigger, frequency high = 7

    run_cycles(&mut apu, 2000);
    let samples = apu.take_samples();
    assert!(samples.iter().any(|&s| s.abs() > 0.01), "wave channel audible");
    assert_eq!(apu.read_register(0xFF26) & 0x04, 0x04, "channel 3 active");
}

#[test]
fn noise_channel_produces_output() {
    let mut apu = powered_apu();
    apu.write_register(0xFF20, 0x00); // NR41: length
    apu.write_register(0xFF21, 0xF0); // NR42: volume 15, DAC on
    apu.write_register(0xFF22, 0x00); // NR43: divisor/shift
    apu.write_register(0xFF23, 0x80); // NR44: trigger

    run_cycles(&mut apu, 2000);
    let samples = apu.take_samples();
    assert!(samples.iter().any(|&s| s.abs() > 0.01), "noise channel audible");
}

#[test]
fn power_off_silences_and_resets() {
    let mut apu = powered_apu();
    apu.write_register(0xFF11, 0x80);
    apu.write_register(0xFF12, 0xF0);
    apu.write_register(0xFF14, 0x87);
    run_cycles(&mut apu, 200);
    let _ = apu.take_samples();

    apu.write_register(0xFF26, 0x00); // power off
    assert_eq!(apu.read_register(0xFF26) & 0x80, 0x00, "power bit cleared");
    assert_eq!(apu.read_register(0xFF26) & 0x0F, 0x00, "all channels off");

    // Control-register writes are ignored while powered off.
    apu.write_register(0xFF12, 0xF0);
    assert_eq!(apu.read_register(0xFF12), 0x00);

    // Output settles to silence once the high-pass transient has decayed.
    run_cycles(&mut apu, 20_000);
    let _ = apu.take_samples();
    run_cycles(&mut apu, 2000);
    let tail = apu.take_samples();
    assert!(rms(&tail) < 1e-4, "output settles to silence, rms={}", rms(&tail));
}

#[test]
fn register_read_masks() {
    let mut apu = powered_apu();
    // NR13/NR23/NR33 are write-only and read back as 0xFF.
    assert_eq!(apu.read_register(0xFF13), 0xFF);
    assert_eq!(apu.read_register(0xFF18), 0xFF);
    // NR11 duty bits are readable, length bits read as 1.
    apu.write_register(0xFF11, 0x80);
    assert_eq!(apu.read_register(0xFF11), 0xBF);
    // Powered off, NR52 reads 0x70 (unused bits set).
    apu.write_register(0xFF26, 0x00);
    assert_eq!(apu.read_register(0xFF26), 0x70);
}

// --- Obscure-behaviour regression tests (dmg_sound 03/05/08/11) --------------
// These pin the length/sweep/power quirks directly, rather than relying only on
// the env-gated Blargg dmg_sound ROM. Channel-enable is observed via NR52.

/// Tick `n` T-cycles in <=255 chunks (`tick` takes a `u8`). 8192 T-cycles = one
/// frame-sequencer step; a step landing on 0/2/4/6 clocks the length counters.
fn tick_cycles(apu: &mut Apu, mut n: u32) {
    while n > 0 {
        let chunk = n.min(255) as u8;
        apu.tick(chunk);
        n -= chunk as u32;
    }
}

/// CH1's enabled bit from NR52 (bit 0).
fn ch1_on(apu: &Apu) -> bool {
    apu.read_register(0xFF26) & 0x01 != 0
}

/// dmg_sound 03: enabling the length counter (NRx4 bit 6, 0→1) while the frame
/// sequencer is in the *first half* of a length period clocks it once — a length
/// of 1 then reaches 0 and the channel disables. In the second half it doesn't.
#[test]
fn enabling_length_in_first_half_clocks_it_once() {
    fn ch1_survives_length_enable(first_half: bool) -> bool {
        let mut apu = powered_apu();
        if first_half {
            tick_cycles(&mut apu, 8192); // step 0→1: odd step = first half
        }
        apu.write_register(0xFF12, 0xF0); // NR12: DAC on
        apu.write_register(0xFF11, 0x3F); // NR11: length load 63 → counter = 1
        apu.write_register(0xFF14, 0x80); // NR14: trigger, length disabled
        assert!(ch1_on(&apu), "on after trigger");
        apu.write_register(0xFF14, 0x40); // NR14: enable length, no trigger
        ch1_on(&apu)
    }
    assert!(!ch1_survives_length_enable(true), "first half: extra clock disables ch1");
    assert!(ch1_survives_length_enable(false), "second half: no extra clock");
}

/// dmg_sound 05: after a sweep calculation in negate mode, clearing NR10's
/// negate bit disables the channel. Without a negate calc, it doesn't.
#[test]
fn clearing_sweep_negate_after_a_negate_calc_disables_channel() {
    fn disabled_by_clearing_negate(negate_calc: bool) -> bool {
        let mut apu = powered_apu();
        apu.write_register(0xFF12, 0xF0); // NR12: DAC on
        // NR10: period 1, negate on; shift 1 does a calc on trigger, shift 0 none.
        apu.write_register(0xFF10, 0x18 | if negate_calc { 0x01 } else { 0x00 });
        apu.write_register(0xFF13, 0x00); // freq low = 0 (calc can't overflow)
        apu.write_register(0xFF14, 0x80); // trigger
        assert!(ch1_on(&apu), "on after trigger");
        apu.write_register(0xFF10, 0x10); // NR10: negate OFF
        !ch1_on(&apu)
    }
    assert!(disabled_by_clearing_negate(true), "negate calc then clear → disabled");
    assert!(!disabled_by_clearing_negate(false), "no negate calc → stays on");
}

/// dmg_sound 08: the NRx1 length-load register is writable while the APU is
/// powered off (DMG), and the value survives power-on. Load length 1 while off,
/// power on, trigger with length enabled — one clock must disable the channel.
#[test]
fn length_load_while_powered_off_takes_effect() {
    let mut apu = powered_apu();
    apu.write_register(0xFF26, 0x00); // power OFF
    apu.write_register(0xFF11, 0x3F); // NR11 length load → counter 1 (while off)
    apu.write_register(0xFF26, 0x80); // power ON
    apu.write_register(0xFF12, 0xF0); // NR12 DAC on
    apu.write_register(0xFF14, 0xC0); // NR14 trigger + length enable
    assert!(ch1_on(&apu), "on after trigger");
    tick_cycles(&mut apu, 8192); // one length clock: 1 → 0
    assert!(!ch1_on(&apu), "off-write length took effect (would be 64 if ignored)");
}

/// dmg_sound 11: on DMG the length counter survives a power-off (only the rest
/// of the channel is cleared). Set length 1 while on, power-cycle, then trigger
/// + enable length — the preserved length expires after one clock.
#[test]
fn length_counter_survives_power_off() {
    let mut apu = powered_apu();
    apu.write_register(0xFF12, 0xF0); // DAC on
    apu.write_register(0xFF11, 0x3F); // length load → counter 1 (while on)
    apu.write_register(0xFF26, 0x00); // power OFF (length preserved)
    apu.write_register(0xFF26, 0x80); // power ON
    apu.write_register(0xFF12, 0xF0); // DAC on again (cleared by power off)
    apu.write_register(0xFF14, 0xC0); // trigger + length enable
    assert!(ch1_on(&apu), "on after trigger");
    tick_cycles(&mut apu, 8192);
    assert!(!ch1_on(&apu), "preserved length (1) expires (would be 64 if reset)");
}
