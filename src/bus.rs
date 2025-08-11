use crate::mmu::Mmu;
use crate::p1::P1; // Renamed GameBoyJoypad to P1
use crate::interrupts::InterruptType;

pub trait Bus {
    fn read_byte(&self, addr: u16) -> u8;
    fn write_byte(&mut self, addr: u16, value: u8);
    fn read_u16(&self, addr: u16) -> u16;
}

pub struct MemoryBus {
    mmu: Mmu,
    p1: P1, // Concrete P1 instance
    if_register: u8, // Interrupt Flag register (0xFF0F)
    ie_register: u8, // Interrupt Enable register (0xFFFF)
}

impl MemoryBus {
    pub fn new() -> MemoryBus {
        MemoryBus {
            mmu: Mmu::new(),
            p1: P1::new(), // Initialize concrete P1
            if_register: 0x00,
            ie_register: 0x00,
        }
    }

    pub fn get_p1_mut(&mut self) -> &mut P1 {
        &mut self.p1
    }

    pub fn request_interrupt(&mut self, interrupt_type: InterruptType) {
        self.if_register |= interrupt_type.to_bit();
    }
}

impl Bus for MemoryBus {
    fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            0xFF00 => self.p1.read_register(),
            0xFF0F => self.if_register,
            0xFFFF => self.ie_register,
            _ => self.mmu.read_byte(addr),
        }
    }

    fn write_byte(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF00 => self.p1.write_register(value),
            0xFF0F => self.if_register = value,
            0xFFFF => self.ie_register = value,
            _ => self.mmu.write_byte(addr, value),
        }
    }

    fn read_u16(&self, addr: u16) -> u16 {
        let lo = self.read_byte(addr) as u16;
        let hi = self.read_byte(addr + 1) as u16;
        (hi << 8) | lo
    }
}
