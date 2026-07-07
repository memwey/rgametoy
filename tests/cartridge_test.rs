extern crate rgametoy;

use rgametoy::cartridge::Cartridge;

/// Build a 4-bank (64 KB) ROM whose first byte of each 16 KB bank is the bank
/// index, declared as an MBC1 cartridge with 8 KB of RAM.
fn banked_rom() -> Vec<u8> {
    let mut data = vec![0u8; 0x1_0000];
    data[0x0147] = 0x03; // MBC1 + RAM + battery
    data[0x0149] = 0x02; // 8 KB external RAM
    for bank in 0..4 {
        data[bank * 0x4000] = bank as u8;
    }
    data
}

#[test]
fn mbc1_rom_bank_switching() {
    let mut cart = Cartridge::from_bytes(banked_rom());

    // Bank 0 is fixed in the low window.
    assert_eq!(cart.read_rom(0x0000), 0);
    // The switchable window defaults to bank 1.
    assert_eq!(cart.read_rom(0x4000), 1);

    cart.write_rom(0x2000, 0x02); // select ROM bank 2
    assert_eq!(cart.read_rom(0x4000), 2);

    cart.write_rom(0x2000, 0x03); // select ROM bank 3
    assert_eq!(cart.read_rom(0x4000), 3);

    // Bank 0 requested in the switchable window maps to bank 1 on MBC1.
    cart.write_rom(0x2000, 0x00);
    assert_eq!(cart.read_rom(0x4000), 1);
}

#[test]
fn mbc1_external_ram_enable() {
    let mut cart = Cartridge::from_bytes(banked_rom());

    // RAM is disabled by default -> reads return 0xFF, writes are ignored.
    cart.write_ram(0xA000, 0x42);
    assert_eq!(cart.read_ram(0xA000), 0xFF);

    cart.write_rom(0x0000, 0x0A); // enable RAM
    cart.write_ram(0xA000, 0x42);
    assert_eq!(cart.read_ram(0xA000), 0x42);

    cart.write_rom(0x0000, 0x00); // disable RAM again
    assert_eq!(cart.read_ram(0xA000), 0xFF);
}

#[test]
fn header_title_is_parsed() {
    let mut data = vec![0u8; 0x8000];
    for (i, b) in b"TESTROM".iter().enumerate() {
        data[0x0134 + i] = *b;
    }
    let cart = Cartridge::from_bytes(data);
    assert_eq!(cart.title(), "TESTROM");
}
