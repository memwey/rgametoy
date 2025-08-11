use crate::mmu::Mmu;

pub trait Bus {
    fn read_byte(&self, addr: u16) -> u8;
    fn write_byte(&mut self, addr: u16, value: u8);
    fn read_u16(&self, addr: u16) -> u16;
}

pub struct MemoryBus {
    mmu: Mmu,
}

impl MemoryBus {
    pub fn new() -> MemoryBus {
        MemoryBus {
            mmu: Mmu::new(),
        }
    }
}

impl Bus for MemoryBus {
    fn read_byte(&self, addr: u16) -> u8 {
        self.mmu.read_byte(addr)
    }

    fn write_byte(&mut self, addr: u16, value: u8) {
        self.mmu.write_byte(addr, value);
    }

    fn read_u16(&self, addr: u16) -> u16 {
        let lo = self.read_byte(addr) as u16;
        let hi = self.read_byte(addr + 1) as u16;
        (hi << 8) | lo
    }
}
