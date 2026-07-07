const WRAM_SIZE: usize = 0x2000; // 8KB

#[derive(Clone)]
pub struct Wram {
    data: [u8; WRAM_SIZE],
}

impl Wram {
    pub fn new() -> Wram {
        Wram {
            data: [0; WRAM_SIZE],
        }
    }

    pub fn read_byte(&self, addr: u16) -> u8 {
        // WRAM addresses are 0xC000-0xDFFF, mapped to 0x0000-0x1FFF internally
        self.data[(addr & 0x1FFF) as usize]
    }

    pub fn write_byte(&mut self, addr: u16, value: u8) {
        self.data[(addr & 0x1FFF) as usize] = value;
    }
}
