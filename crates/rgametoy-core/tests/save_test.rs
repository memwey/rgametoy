extern crate rgametoy_core;

use rgametoy_core::cartridge::Cartridge;
use rgametoy_core::Console;

fn battery_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0147] = 0x03; // MBC1 + RAM + BATTERY
    rom[0x0149] = 0x02; // 8 KB external RAM
    rom
}

/// Save data written by the game through the bus survives a dump/restore into
/// a fresh machine — the emulator flushes `cartridge().ram()` to `<rom>.sav`
/// and reloads it with `load_ram`.
#[test]
fn external_ram_saves_and_restores_via_bus() {
    let mut console = Console::new();
    console.power_on(Cartridge::from_bytes(battery_rom()));

    // The game enables external RAM (MBC control write) then stores save data.
    console.write_mem(0x0000, 0x0A);
    console.write_mem(0xA000, 0x42);
    console.write_mem(0xBFFF, 0x99);

    assert!(console.cartridge().has_battery());
    assert!(console.cartridge().ram_dirty());
    assert_eq!(console.read_mem(0xA000), 0x42);

    // Emulator would write these bytes out to the .sav file.
    let saved = console.cartridge().ram().to_vec();

    // A fresh machine restores the save and reads it back.
    let mut console2 = Console::new();
    console2.power_on(Cartridge::from_bytes(battery_rom()));
    console2.cartridge_mut().load_ram(&saved);
    console2.write_mem(0x0000, 0x0A); // enable RAM to read
    assert_eq!(console2.read_mem(0xA000), 0x42);
    assert_eq!(console2.read_mem(0xBFFF), 0x99);
}

/// With RAM disabled, external RAM reads are open-bus (0xFF) and writes are
/// ignored — the game must explicitly enable RAM around a save.
#[test]
fn disabled_ram_is_not_written() {
    let mut console = Console::new();
    console.power_on(Cartridge::from_bytes(battery_rom()));

    // No enable write first: the store is dropped.
    console.write_mem(0xA000, 0x42);
    assert_eq!(console.read_mem(0xA000), 0xFF);
    assert!(!console.cartridge().ram_dirty());
}
