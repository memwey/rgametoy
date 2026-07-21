//! Game Boy cartridge: ROM, optional battery-backed RAM and a memory bank
//! controller (MBC). Supports no-MBC (32KB), MBC1, MBC3 (no RTC) and MBC5.

use std::sync::Arc;

#[cfg(feature = "serialize")]
use crate::state::{write_u16_le, write_u32_le, write_u8, Reader, SaveStateError};

const ROM_BANK_SIZE: usize = 0x4000; // 16 KB
const RAM_BANK_SIZE: usize = 0x2000; // 8 KB

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum MbcKind {
    None,
    Mbc1,
    Mbc3,
    Mbc5,
}

/// On-disk tag for the MBC kind. Bumping these means changing the byte stored
/// in save states; the new tag must be different from any reserved/in-use value
/// to keep older reads distinguishable. Kept separate from `MbcKind` so
/// refactors that re-order the enum don't silently shift the on-disk values.
#[cfg(feature = "serialize")]
#[repr(u8)]
#[derive(Clone, Copy)]
enum MbcKindTag {
    None = 0,
    Mbc1 = 1,
    Mbc3 = 3,
    Mbc5 = 5,
}

#[cfg(feature = "serialize")]
impl MbcKind {
    fn tag(self) -> u8 {
        match self {
            MbcKind::None => MbcKindTag::None as u8,
            MbcKind::Mbc1 => MbcKindTag::Mbc1 as u8,
            MbcKind::Mbc3 => MbcKindTag::Mbc3 as u8,
            MbcKind::Mbc5 => MbcKindTag::Mbc5 as u8,
        }
    }
}

#[cfg(feature = "serialize")]
impl MbcKindTag {
    fn from(b: u8) -> Option<MbcKind> {
        match b {
            0 => Some(MbcKind::None),
            1 => Some(MbcKind::Mbc1),
            3 => Some(MbcKind::Mbc3),
            5 => Some(MbcKind::Mbc5),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Cartridge {
    /// The ROM image. Shared (`Arc`) because it is read-only at runtime, so
    /// cloning a cartridge — e.g. for an instant save state — doesn't copy the
    /// (up to several MB) ROM; only the small mutable state below is duplicated.
    rom: Arc<Vec<u8>>,
    ram: Vec<u8>,
    kind: MbcKind,
    /// Whether the cartridge has battery-backed RAM (persistent save data).
    has_battery: bool,
    /// MBC5 rumble cartridges use RAM-bank bit 3 as the motor control line, so
    /// only bits 0-2 select RAM. The motor itself is a host-side effect we do
    /// not expose, but the narrower bank mask is part of cartridge behaviour.
    has_rumble: bool,
    /// Set when the game writes to external RAM; cleared once flushed to disk.
    ram_dirty: bool,

    rom_bank: usize,
    ram_bank: usize,
    ram_enabled: bool,
    /// MBC1 banking mode: 0 = simple ROM banking, 1 = RAM/upper-ROM banking.
    banking_mode: u8,
}

impl Cartridge {
    /// An empty 32 KB cartridge, used before a ROM is loaded (and by tests that
    /// inject a small program with [`Cartridge::load`]).
    pub fn new() -> Cartridge {
        Cartridge {
            rom: Arc::new(vec![0; ROM_BANK_SIZE * 2]),
            ram: Vec::new(),
            kind: MbcKind::None,
            has_battery: false,
            has_rumble: false,
            ram_dirty: false,
            rom_bank: 1,
            ram_bank: 0,
            ram_enabled: false,
            banking_mode: 0,
        }
    }

    /// Build a cartridge from a raw ROM image, reading the header to pick the
    /// memory bank controller and external RAM size.
    pub fn from_bytes(data: Vec<u8>) -> Cartridge {
        let type_byte = data.get(0x0147).copied().unwrap_or(0);
        // Lenient at the library level: an unimplemented type falls back to
        // MBC1 so `from_bytes` remains infallible. Frontends refuse unsupported
        // ROMs up front via `is_type_supported`; importantly, the core performs
        // no logging or other host I/O here.
        let kind = Self::classify(type_byte).unwrap_or(MbcKind::Mbc1);

        // Cartridge types whose external RAM is battery-backed (persistent).
        let has_battery = matches!(
            type_byte,
            0x03 | 0x06 | 0x09 | 0x0D | 0x0F | 0x10 | 0x13 | 0x1B | 0x1E | 0x22 | 0xFF
        );

        let ram_size = match data.get(0x0149).copied().unwrap_or(0) {
            0x02 => 0x0000_2000, // 8 KB
            0x03 => 0x0000_8000, // 32 KB
            0x04 => 0x0002_0000, // 128 KB
            0x05 => 0x0001_0000, // 64 KB
            _ => 0,
        };

        let mut rom = data;
        if rom.len() < ROM_BANK_SIZE * 2 {
            rom.resize(ROM_BANK_SIZE * 2, 0);
        }

        Cartridge {
            rom: Arc::new(rom),
            ram: vec![0; ram_size],
            kind,
            has_battery,
            has_rumble: matches!(type_byte, 0x1C..=0x1E),
            ram_dirty: false,
            rom_bank: 1,
            ram_bank: 0,
            ram_enabled: false,
            banking_mode: 0,
        }
    }

    /// The MBC a cartridge-type byte (header 0x0147) selects, or `None` if we
    /// don't implement it (e.g. MBC2, MMM01). The single source of truth for
    /// both `from_bytes` and [`Cartridge::is_type_supported`].
    fn classify(type_byte: u8) -> Option<MbcKind> {
        match type_byte {
            0x00 => Some(MbcKind::None),
            0x01..=0x03 => Some(MbcKind::Mbc1),
            0x0F..=0x13 => Some(MbcKind::Mbc3),
            0x19..=0x1E => Some(MbcKind::Mbc5),
            _ => None,
        }
    }

    /// Whether the cartridge-type byte (header 0x0147) is one we implement. The
    /// frontend calls this to refuse a ROM it would otherwise mis-emulate (the
    /// library `from_bytes` falls back to MBC1 for unknown types, which is wrong
    /// for e.g. MBC2's different bank layout).
    pub fn is_type_supported(type_byte: u8) -> bool {
        Self::classify(type_byte).is_some()
    }

    /// Human-readable name of the MBC a cartridge-type byte selects, for
    /// frontend startup banners. Derived from `classify`, so it can never
    /// drift from what the core actually emulates.
    pub fn type_name(type_byte: u8) -> &'static str {
        match Self::classify(type_byte) {
            Some(MbcKind::None) => "ROM only",
            Some(MbcKind::Mbc1) => "MBC1",
            Some(MbcKind::Mbc3) => "MBC3",
            Some(MbcKind::Mbc5) => "MBC5",
            None => "unknown",
        }
    }

    /// Whether this cartridge persists its external RAM (has a battery).
    pub fn has_battery(&self) -> bool {
        self.has_battery && !self.ram.is_empty()
    }

    /// The current external RAM contents (the save data).
    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    /// Restore previously saved external RAM (from a `.sav` file). Extra bytes
    /// are ignored; a shorter save leaves the remainder zeroed.
    pub fn load_ram(&mut self, data: &[u8]) {
        let n = self.ram.len().min(data.len());
        self.ram[..n].copy_from_slice(&data[..n]);
        self.ram_dirty = false;
    }

    /// True if the game has written to RAM since the last flush.
    pub fn ram_dirty(&self) -> bool {
        self.ram_dirty
    }

    pub fn clear_ram_dirty(&mut self) {
        self.ram_dirty = false;
    }

    /// Reset the memory-bank controller as a cartridge power cycle would,
    /// while preserving the ROM and battery-backed RAM contents.
    pub(crate) fn reset_controller(&mut self) {
        self.rom_bank = 1;
        self.ram_bank = 0;
        self.ram_enabled = false;
        self.banking_mode = 0;
    }

    /// Overwrite the start of ROM bank 0 with `program`. Used to inject small
    /// test programs; real ROMs come through [`Cartridge::from_bytes`].
    pub fn load(&mut self, program: &[u8]) {
        let rom = Arc::make_mut(&mut self.rom);
        if program.len() > rom.len() {
            rom.resize(program.len(), 0);
        }
        rom[..program.len()].copy_from_slice(program);
    }

    /// ASCII title from the cartridge header (0x0134..0x0143).
    pub fn title(&self) -> String {
        self.rom
            .get(0x0134..0x0144)
            .map(|bytes| {
                bytes
                    .iter()
                    .take_while(|&&b| b != 0)
                    .filter(|&&b| (0x20..0x7F).contains(&b))
                    .map(|&b| b as char)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        let index = match addr {
            0x0000..=0x3FFF => {
                // MBC1 mode 1 can remap the low bank; other controllers keep 0.
                if self.kind == MbcKind::Mbc1 && self.banking_mode == 1 {
                    let bank = (self.ram_bank << 5) & self.rom_bank_mask();
                    bank * ROM_BANK_SIZE + addr as usize
                } else {
                    addr as usize
                }
            }
            0x4000..=0x7FFF => self.rom_bank_number() * ROM_BANK_SIZE + (addr as usize - 0x4000),
            _ => return 0xFF,
        };
        self.rom.get(index).copied().unwrap_or(0xFF)
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        match self.kind {
            MbcKind::None => {}
            MbcKind::Mbc1 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    let low = (value & 0x1F) as usize;
                    self.rom_bank = (self.rom_bank & 0x60) | if low == 0 { 1 } else { low };
                }
                0x4000..=0x5FFF => self.ram_bank = (value & 0x03) as usize,
                0x6000..=0x7FFF => self.banking_mode = value & 0x01,
                _ => {}
            },
            MbcKind::Mbc3 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    let bank = (value & 0x7F) as usize;
                    self.rom_bank = if bank == 0 { 1 } else { bank };
                }
                0x4000..=0x5FFF => self.ram_bank = (value & 0x0F) as usize,
                _ => {}
            },
            MbcKind::Mbc5 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
                0x2000..=0x2FFF => self.rom_bank = (self.rom_bank & 0x100) | value as usize,
                0x3000..=0x3FFF => {
                    self.rom_bank = (self.rom_bank & 0xFF) | (((value & 0x01) as usize) << 8);
                }
                0x4000..=0x5FFF => {
                    let mask = if self.has_rumble { 0x07 } else { 0x0F };
                    self.ram_bank = (value & mask) as usize;
                }
                _ => {}
            },
        }
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enabled || self.ram.is_empty() {
            return 0xFF;
        }
        let index = self.ram_bank * RAM_BANK_SIZE + (addr as usize - 0xA000);
        self.ram.get(index).copied().unwrap_or(0xFF)
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enabled || self.ram.is_empty() {
            return;
        }
        let index = self.ram_bank * RAM_BANK_SIZE + (addr as usize - 0xA000);
        if let Some(slot) = self.ram.get_mut(index) {
            *slot = value;
            self.ram_dirty = true;
        }
    }

    /// Effective ROM bank for the 0x4000..0x7FFF window.
    fn rom_bank_number(&self) -> usize {
        let bank = match self.kind {
            MbcKind::None => 1,
            MbcKind::Mbc1 => {
                if self.banking_mode == 0 {
                    (self.ram_bank << 5) | (self.rom_bank & 0x1F)
                } else {
                    self.rom_bank & 0x1F
                }
            }
            MbcKind::Mbc3 | MbcKind::Mbc5 => self.rom_bank,
        };
        (bank & self.rom_bank_mask()).max(if self.kind == MbcKind::Mbc5 { 0 } else { 1 })
    }

    /// Mask of valid bank numbers, derived from the ROM size. Real cartridges
    /// are power-of-two sized; for the rare odd size this rounds *up*, so the
    /// mask may admit one nonexistent bank — reads there fall off the end of
    /// the ROM slice and return 0xFF (see `read_rom`), which is the intended
    /// simplification rather than a precise per-size bank count.
    fn rom_bank_mask(&self) -> usize {
        let banks = (self.rom.len() / ROM_BANK_SIZE).max(2);
        banks.next_power_of_two() - 1
    }

    /// Append the cartridge's mutable state to `out`. The ROM image itself is
    /// *not* serialized — the host is expected to have already loaded the
    /// same ROM into the console before loading a state. Saving the ROM
    /// would multiply every state save by 32 KB..8 MB and add a
    /// copyright-attribution hazard.
    #[cfg(feature = "serialize")]
    pub fn write_state(&self, out: &mut Vec<u8>) {
        write_u8(out, self.kind.tag());
        write_u8(out, self.has_battery as u8);
        write_u16_le(out, self.rom_bank as u16);
        write_u8(out, self.ram_bank as u8);
        write_u8(out, self.ram_enabled as u8);
        write_u8(out, self.banking_mode);
        write_u32_le(out, self.ram.len() as u32);
        out.extend_from_slice(&self.ram);
    }

    /// Parse the cartridge's mutable state out of a save-state blob. The
    /// caller is responsible for having already populated `self.rom` via
    /// [`Cartridge::from_bytes`] (so `self.ram.len()` matches the header).
    #[cfg(feature = "serialize")]
    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        let tag = r.read_u8()?;
        let kind = MbcKindTag::from(tag).ok_or(SaveStateError::UnknownCartridgeKind(tag))?;
        let has_battery = r.read_u8()? != 0;
        let rom_bank = r.read_u16_le()? as usize;
        let ram_bank = r.read_u8()? as usize;
        let ram_enabled = r.read_u8()? != 0;
        let banking_mode = r.read_u8()?;
        let n = r.read_u32_le()? as usize;
        let max_ram_bank = match self.kind {
            MbcKind::None => 0,
            MbcKind::Mbc1 => 3,
            MbcKind::Mbc3 => 15,
            MbcKind::Mbc5 if self.has_rumble => 7,
            MbcKind::Mbc5 => 15,
        };
        if kind != self.kind
            || has_battery != self.has_battery
            || n != self.ram.len()
            || banking_mode > 1
            || rom_bank > 0x1FF
            || ram_bank > max_ram_bank
        {
            return Err(SaveStateError::Corrupt);
        }
        let bytes = r.read_exact(n)?;
        self.rom_bank = rom_bank;
        self.ram_bank = ram_bank;
        self.ram_enabled = ram_enabled;
        self.banking_mode = banking_mode;
        self.ram.copy_from_slice(bytes);
        // The just-loaded RAM may differ from what's persisted (.sav / IDB), so
        // mark it dirty rather than clean — otherwise a save state taken with
        // unsaved battery RAM would never get flushed after loading, losing it.
        self.ram_dirty = true;
        Ok(())
    }
}

impl Default for Cartridge {
    fn default() -> Self {
        Self::new()
    }
}
