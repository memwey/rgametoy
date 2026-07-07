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

fn battery_rom() -> Vec<u8> {
    let mut data = vec![0u8; 0x8000];
    data[0x0147] = 0x03; // MBC1 + RAM + BATTERY
    data[0x0149] = 0x02; // 8 KB external RAM
    data
}

#[test]
fn battery_ram_round_trips_through_a_save() {
    let mut cart = Cartridge::from_bytes(battery_rom());
    assert!(cart.has_battery());
    assert!(!cart.ram_dirty());

    // The game enables RAM and writes save data.
    cart.write_rom(0x0000, 0x0A);
    cart.write_ram(0xA000, 0xAB);
    cart.write_ram(0xA123, 0xCD);
    assert!(cart.ram_dirty(), "external RAM writes mark the save dirty");

    // The emulator would dump this to <rom>.sav.
    let saved = cart.ram().to_vec();
    cart.clear_ram_dirty();
    assert!(!cart.ram_dirty());

    // A fresh cartridge restores the save and reads it back.
    let mut restored = Cartridge::from_bytes(battery_rom());
    restored.load_ram(&saved);
    assert!(!restored.ram_dirty(), "restoring a save is not a game write");
    restored.write_rom(0x0000, 0x0A); // enable RAM to read
    assert_eq!(restored.read_ram(0xA000), 0xAB);
    assert_eq!(restored.read_ram(0xA123), 0xCD);
}

#[test]
fn cartridge_without_battery_is_not_persisted() {
    let mut data = vec![0u8; 0x8000];
    data[0x0147] = 0x01; // MBC1, no RAM/battery
    data[0x0149] = 0x02;
    let cart = Cartridge::from_bytes(data);
    assert!(!cart.has_battery());
}

#[test]
fn battery_type_without_ram_is_not_persisted() {
    let mut data = vec![0u8; 0x8000];
    data[0x0147] = 0x0F; // MBC3 + TIMER + BATTERY, but...
    data[0x0149] = 0x00; // ...no external RAM
    let cart = Cartridge::from_bytes(data);
    assert!(!cart.has_battery(), "nothing to persist without RAM");
}
