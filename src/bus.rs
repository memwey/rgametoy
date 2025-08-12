use crate::hram::Hram;
use crate::interrupts::InterruptType;
use crate::p1::P1;
use crate::ppu::Ppu;
use crate::rom::Rom;
use crate::timer::Timer;
use crate::wram::Wram;

pub trait Bus {
    fn read_byte(&self, addr: u16) -> u8;
    fn write_byte(&mut self, addr: u16, value: u8);
    fn read_u16(&self, addr: u16) -> u16;
}

pub struct MemoryBus {
    rom: Rom,
    wram: Wram,
    hram: Hram,
    p1: P1,
    ppu: Ppu,
    timer: Timer,
    if_register: u8, // Interrupt Flag register (0xFF0F)
    ie_register: u8, // Interrupt Enable register (0xFFFF)
}

impl MemoryBus {
    pub fn new() -> MemoryBus {
        MemoryBus {
            rom: Rom::new(),
            wram: Wram::new(),
            hram: Hram::new(),
            p1: P1::new(),
            ppu: Ppu::new(),
            timer: Timer::new(),
            if_register: 0x00,
            ie_register: 0x00,
        }
    }

    pub fn get_p1_mut(&mut self) -> &mut P1 {
        &mut self.p1
    }

    pub fn get_timer_mut(&mut self) -> &mut Timer {
        &mut self.timer
    }

    pub fn get_ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    pub fn request_interrupt(&mut self, interrupt_type: InterruptType) {
        self.if_register |= interrupt_type.to_bit();
    }
}

impl Bus for MemoryBus {
    fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            // 32KB ROM Area
            0x0000..=0x7FFF => self.rom.read_byte(addr),
            // 8KB VRAM
            0x8000..=0x9FFF => self.ppu.read_vram(addr),
            // 8KB External RAM (from cartridge, not implemented)
            0xA000..=0xBFFF => 0xFF, // Placeholder
            // 8KB Work RAM (WRAM)
            0xC000..=0xDFFF => self.wram.read_byte(addr),
            // Echo RAM (mirror of 0xC000-0xDDFF)
            0xE000..=0xFDFF => self.wram.read_byte(addr),
            // OAM (Sprite Attribute Table)
            0xFE00..=0xFE9F => self.ppu.read_oam(addr),
            // Not Usable
            0xFEA0..=0xFEFF => 0xFF,
            // I/O Registers
            0xFF00 => self.p1.read_register(),
            0xFF04..=0xFF07 => self.timer.read_register(addr),
            0xFF0F => self.if_register,
            // PPU I/O Registers
            0xFF40..=0xFF4B => self.ppu.read_register(addr),
            // TODO: Add other I/O registers (APU, Serial, Joypad)
            0xFF01..=0xFF03 | 0xFF08..=0xFF0E | 0xFF10..=0xFF3F | 0xFF4C..=0xFF7F => 0xFF, // Placeholders
            // High RAM (HRAM)
            0xFF80..=0xFFFE => self.hram.read_byte(addr),
            // Interrupt Enable Register
            0xFFFF => self.ie_register,
        }
    }

    fn write_byte(&mut self, addr: u16, value: u8) {
        match addr {
            // ROM Area (writes are only for loading program, otherwise ignored)
            0x0000..=0x7FFF => self.rom.write_byte(addr, value),
            // 8KB VRAM
            0x8000..=0x9FFF => self.ppu.write_vram(addr, value),
            // 8KB External RAM
            0xA000..=0xBFFF => { /* No-op */ }
            // 8KB Work RAM (WRAM)
            0xC000..=0xDFFF => self.wram.write_byte(addr, value),
            // Echo RAM (mirror of 0xC000-0xDDFF)
            0xE000..=0xFDFF => self.wram.write_byte(addr, value),
            // OAM
            0xFE00..=0xFE9F => self.ppu.write_oam(addr, value),
            // Not Usable
            0xFEA0..=0xFEFF => { /* No-op */ }
            // I/O Registers
            0xFF00 => self.p1.write_register(value),
            0xFF04..=0xFF07 => self.timer.write_register(addr, value),
            0xFF0F => self.if_register = value,
            // PPU I/O Registers
            0xFF40..=0xFF4B => self.ppu.write_register(addr, value),
            // TODO: Add other I/O registers (APU, Serial, Joypad)
            0xFF01..=0xFF03 | 0xFF08..=0xFF0E | 0xFF10..=0xFF3F | 0xFF4C..=0xFF7F => { /* No-op */ }
            // High RAM (HRAM)
            0xFF80..=0xFFFE => self.hram.write_byte(addr, value),
            // Interrupt Enable Register
            0xFFFF => self.ie_register = value,
        }
    }

    fn read_u16(&self, addr: u16) -> u16 {
        let lo = self.read_byte(addr) as u16;
        let hi = self.read_byte(addr + 1) as u16;
        (hi << 8) | lo
    }
}