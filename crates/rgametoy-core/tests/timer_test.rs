extern crate rgametoy_core;

use rgametoy_core::timer::Timer;

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

// The four TAC frequencies select counter bits 9/3/5/7, so TIMA ticks every
// 1024 / 16 / 64 / 256 T-cycles (mooneye tim00/01/10/11).
#[test]
fn tima_frequency_00_ticks_every_1024_cycles() {
    let mut t = Timer::new();
    t.write_register(0xFF07, 0x04); // enable, freq 00
    for _ in 0..5 {
        t.tick(200); // 1000 T
    }
    assert_eq!(t.read_register(0xFF05), 0, "no tick before 1024 T");
    t.tick(24); // 1024 T total
    assert_eq!(t.read_register(0xFF05), 1);
}

#[test]
fn tima_frequency_10_ticks_every_64_cycles() {
    let mut t = Timer::new();
    t.write_register(0xFF07, 0x06); // enable, freq 10
    t.tick(63);
    assert_eq!(t.read_register(0xFF05), 0);
    t.tick(1); // 64 T
    assert_eq!(t.read_register(0xFF05), 1);
}

#[test]
fn tima_frequency_11_ticks_every_256_cycles() {
    let mut t = Timer::new();
    t.write_register(0xFF07, 0x07); // enable, freq 11
    t.tick(200);
    t.tick(55);
    assert_eq!(t.read_register(0xFF05), 0);
    t.tick(1); // 256 T
    assert_eq!(t.read_register(0xFF05), 1);
}

/// A TIMA write in the 1-M-cycle delay after an overflow (before the reload)
/// lands and cancels the pending reload (mooneye `tima_write_reloading`).
#[test]
fn tima_write_during_the_reload_delay_cancels_the_reload() {
    let mut t = Timer::new();
    t.write_register(0xFF06, 0x42); // TMA
    t.write_register(0xFF05, 0xFF); // TIMA about to overflow
    t.write_register(0xFF07, 0x05); // enable, every 16 T

    t.tick(16); // overflow -> reload pending
    t.write_register(0xFF05, 0x50); // write during the delay: cancels reload
    t.tick(8);
    assert_eq!(
        t.read_register(0xFF05),
        0x50,
        "the written value survives; no reload from TMA"
    );
}

/// A TIMA write on the exact reload cycle is ignored — the TMA reload wins
/// (mooneye `tima_write_reloading`).
#[test]
fn tima_write_on_the_reload_cycle_is_ignored() {
    let mut t = Timer::new();
    t.write_register(0xFF06, 0x42); // TMA
    t.write_register(0xFF05, 0xFF); // TIMA
    t.write_register(0xFF07, 0x05); // enable, every 16 T

    t.tick(16); // overflow
    t.tick(4); // reload cycle: TIMA <- TMA
    t.write_register(0xFF05, 0x99); // ignored: reload wins
    assert_eq!(t.read_register(0xFF05), 0x42);
}

/// A TMA write before the reload changes the value TIMA reloads to
/// (mooneye `tma_write_reloading`).
#[test]
fn tma_write_before_the_reload_changes_the_reloaded_value() {
    let mut t = Timer::new();
    t.write_register(0xFF06, 0x42); // TMA
    t.write_register(0xFF05, 0xFF);
    t.write_register(0xFF07, 0x05);

    t.tick(16); // overflow
    t.write_register(0xFF06, 0x77); // new TMA, before the reload
    t.tick(4); // reload uses the new TMA
    assert_eq!(t.read_register(0xFF05), 0x77);
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
    assert_eq!(
        t.read_register(0xFF05),
        0,
        "disabled timer never ticked TIMA"
    );

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
    assert_eq!(
        t.read_register(0xFF05),
        0x00,
        "TIMA reads 0 during the delay"
    );

    let irq = t.tick(4); // reload completes
    assert!(irq, "interrupt fires when TIMA reloads");
    assert_eq!(t.read_register(0xFF05), 0x42, "TIMA reloaded from TMA");
}

/// The system counter free-runs through the boot ROM, so a cartridge starts
/// executing with DIV already at 0xAB — not 0. We bypass the boot ROM, so
/// `Console` seeds the counter instead; getting this wrong puts every later
/// TIMA increment at the wrong absolute cycle. Pinned by gbmicrotest
/// `poweron_div_*` / `halt_bug` and mooneye `boot_div-dmgABCmgb`, none of which
/// can run without the ROM bundle — hence this local guard.
#[test]
fn post_boot_div_matches_what_the_boot_rom_leaves_behind() {
    use rgametoy_core::cartridge::Cartridge;
    use rgametoy_core::Console;

    /// A console at the 0x0100 hand-off, run for `t` T-cycles (ROM is all NOPs,
    /// so `step` advances in exact 4-T units).
    fn div_after(t: u64) -> u8 {
        let mut c = Console::new();
        c.power_on(Cartridge::from_bytes(vec![0u8; 0x8000]));
        while c.total_cycles() < t {
            c.step();
        }
        assert_eq!(
            c.total_cycles(),
            t,
            "NOP stepping should land exactly on {t}"
        );
        c.read_mem(0xFF04)
    }

    assert_eq!(div_after(0), 0xAB, "DIV at the 0x0100 hand-off");
    // The seeded counter is 0xABCA, so the roll to 0xAC is 54 T-cycles away.
    // Bracketing it pins the phase, not just the register's starting value.
    assert_eq!(div_after(52), 0xAB, "not rolled over yet at 52 T-cycles");
    assert_eq!(div_after(56), 0xAC, "rolled over by 56 T-cycles");
}
