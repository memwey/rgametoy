extern crate rgametoy;

use rgametoy::console::bus::Bus;
use rgametoy::console::Console;

/// The bytes a program shifts out over the serial port are captured — this is
/// how Blargg's test ROMs report their results.
#[test]
fn serial_transfers_are_captured() {
    let mut console = Console::new();

    for &byte in b"OK" {
        // Program: write the byte to SB (0xFF01), then start a transfer with
        // the internal clock by writing 0x81 to SC (0xFF02).
        console.get_bus_mut().write_byte(0xFF01, byte);
        console.get_bus_mut().write_byte(0xFF02, 0x81);

        // A byte takes 4096 T-cycles; step long enough (ROM is NOPs) for the
        // transfer to complete.
        for _ in 0..1100 {
            console.step();
        }
    }

    assert_eq!(console.take_serial_output(), b"OK");
}

/// A transfer that never starts (SC start bit clear) captures nothing.
#[test]
fn no_output_without_transfer_start() {
    let mut console = Console::new();
    console.get_bus_mut().write_byte(0xFF01, b'X');
    // SC written without bit 7 -> no transfer.
    console.get_bus_mut().write_byte(0xFF02, 0x01);
    for _ in 0..2000 {
        console.step();
    }
    assert!(console.take_serial_output().is_empty());
}
