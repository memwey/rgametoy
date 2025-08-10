pub struct Mmu {
    ram: [u8; 0x10000],
}

impl Mmu {
    pub fn new() -> Mmu {
        Mmu {
            ram: [0; 0x10000],
        }
    }

    pub fn read_byte(&self, address: u16) -> u8 {
        self.ram[address as usize]
    }

    pub fn read_u16(&self, address: u16) -> u16 {
        let low = self.read_byte(address) as u16;
        let high = self.read_byte(address + 1) as u16;
        (high << 8) | low
    }

    pub fn write_byte(&mut self, address: u16, value: u8) {
        self.ram[address as usize] = value;
    }
}
