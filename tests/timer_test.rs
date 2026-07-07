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
