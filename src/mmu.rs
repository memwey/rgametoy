pub struct Mmu {
    memory: [u8; 0x10000],
}

impl Mmu {
    pub fn new() -> Mmu {
        Mmu {
            memory: [0; 0x10000],
        }
    }

    pub fn read_byte(&self, addr: u16) -> u8 {
        self.memory[addr as usize]
    }

    pub fn write_byte(&mut self, addr: u16, value: u8) {
        self.memory[addr as usize] = value;
    }
}