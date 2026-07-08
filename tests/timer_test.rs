extern crate rgametoy;

use rgametoy::console::timer::Timer;

#[test]
fn div_increments_every_256_cycles() {
    let mut t = Timer::new();
    t.tick(255);
    assert_eq!(t.read_register(0xFF04), 0, "DIV not yet incremented at 255");
    t.tick(1);
    assert_eq!(t.read_register(0xFF04), 1, "DIV increments at 256 T-cycles");
}

#[test]
fn writing_div_resets_it() {
    let mut t = Timer::new();
    t.tick(200);
    t.tick(56); // 256 total
    assert_eq!(t.read_register(0xFF04), 1);
    t.write_register(0xFF04, 0xAB); // any write clears DIV
    assert_eq!(t.read_register(0xFF04), 0);
}

#[test]
fn tima_increments_at_the_selected_frequency() {
    let mut t = Timer::new();
    t.write_register(0xFF07, 0x05); // enabled, freq 01 -> every 16 T-cycles

    t.tick(16);
    assert_eq!(t.read_register(0xFF05), 1);
    t.tick(16);
    assert_eq!(t.read_register(0xFF05), 2);
}

#[test]
fn disabled_timer_does_not_increment_tima() {
    let mut t = Timer::new();
    t.write_register(0xFF07, 0x01); // freq bits set but enable (bit 2) clear
    for _ in 0..5 {
        t.tick(200); // 1000 T-cycles
    }
    assert_eq!(t.read_register(0xFF05), 0);
}

/// The DMG timer's TAC-disable glitch, the mechanism behind mooneye
/// `rapid_toggle`: turning the timer off while the selected counter bit is
/// high drops the increment input from 1 to 0, and that falling edge ticks
/// TIMA once. Enabling on an already-high bit is a *rising* edge and must not
/// tick.
#[test]
fn toggling_tac_off_on_a_high_counter_bit_ticks_tima() {
    let mut t = Timer::new();
    // Freq 00 selects counter bit 9 (period 512). Keep the timer disabled and
    // advance the counter until bit 9 is high (counter in [512, 1023]).
    for _ in 0..600 {
        t.tick(1);
    }
    assert_eq!(t.read_register(0xFF05), 0, "disabled timer never ticked TIMA");

    t.write_register(0xFF07, 0x04); // enable, freq 00 — bit 9 already high
    assert_eq!(
        t.read_register(0xFF05),
        0,
        "enabling on a high bit is a rising edge, no tick"
    );

    t.write_register(0xFF07, 0x00); // disable — input falls from 1 to 0
    assert_eq!(
        t.read_register(0xFF05),
        1,
        "disabling on a high bit ticks TIMA once"
    );
}

/// Writing DIV resets the counter; if the selected bit was high that reset is a
/// falling edge and also ticks TIMA once (the DIV-write glitch).
#[test]
fn writing_div_on_a_high_counter_bit_ticks_tima() {
    let mut t = Timer::new();
    t.write_register(0xFF07, 0x04); // enable, freq 00 (bit 9)
    for _ in 0..600 {
        t.tick(1); // counter into [512, 1023]; no falling edge of bit 9 yet
    }
    let before = t.read_register(0xFF05);
    t.write_register(0xFF04, 0x00); // counter -> 0: bit 9 falls 1 -> 0
    assert_eq!(
        t.read_register(0xFF05),
        before.wrapping_add(1),
        "DIV reset while the selected bit is high ticks TIMA"
    );
}

#[test]
fn tima_overflow_reloads_from_tma_after_a_delay_and_interrupts() {
    let mut t = Timer::new();
    t.write_register(0xFF06, 0x42); // TMA
    t.write_register(0xFF05, 0xFF); // TIMA about to overflow
    t.write_register(0xFF07, 0x05); // enabled, every 16 T-cycles

    let irq = t.tick(16); // TIMA overflows on this step
    assert!(!irq, "interrupt is delayed one M-cycle after overflow");
    assert_eq!(t.read_register(0xFF05), 0x00, "TIMA reads 0 during the delay");

    let irq = t.tick(4); // reload completes
    assert!(irq, "interrupt fires when TIMA reloads");
    assert_eq!(t.read_register(0xFF05), 0x42, "TIMA reloaded from TMA");
}
