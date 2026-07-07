extern crate rgametoy;

use rgametoy::bus::Bus;
use rgametoy::cartridge::Cartridge;
use rgametoy::console::Console;

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
    console.load_cartridge(Cartridge::from_bytes(battery_rom()));

    // The game enables external RAM (MBC control write) then stores save data.
    console.get_bus_mut().write_byte(0x0000, 0x0A);
    console.get_bus_mut().write_byte(0xA000, 0x42);
    console.get_bus_mut().write_byte(0xBFFF, 0x99);

    assert!(console.get_bus_mut().cartridge().has_battery());
    assert!(console.get_bus_mut().cartridge().ram_dirty());
    assert_eq!(console.get_bus_mut().read_byte(0xA000), 0x42);

    // Emulator would write these bytes out to the .sav file.
    let saved = console.get_bus_mut().cartridge().ram().to_vec();

    // A fresh machine restores the save and reads it back.
    let mut console2 = Console::new();
    console2.load_cartridge(Cartridge::from_bytes(battery_rom()));
    console2.get_bus_mut().cartridge_mut().load_ram(&saved);
    console2.get_bus_mut().write_byte(0x0000, 0x0A); // enable RAM to read
    assert_eq!(console2.get_bus_mut().read_byte(0xA000), 0x42);
    assert_eq!(console2.get_bus_mut().read_byte(0xBFFF), 0x99);
}

/// With RAM disabled, external RAM reads are open-bus (0xFF) and writes are
/// ignored — the game must explicitly enable RAM around a save.
#[test]
fn disabled_ram_is_not_written() {
    let mut console = Console::new();
    console.load_cartridge(Cartridge::from_bytes(battery_rom()));

    // No enable write first: the store is dropped.
    console.get_bus_mut().write_byte(0xA000, 0x42);
    assert_eq!(console.get_bus_mut().read_byte(0xA000), 0xFF);
    assert!(!console.get_bus_mut().cartridge().ram_dirty());
}
