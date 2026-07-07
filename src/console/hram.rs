const HRAM_SIZE: usize = 0x007F; // 127 bytes

#[derive(Clone)]
pub struct Hram {
    data: [u8; HRAM_SIZE],
}

impl Hram {
    pub fn new() -> Hram {
        Hram {
            data: [0; HRAM_SIZE],
        }
    }

    pub fn read_byte(&self, addr: u16) -> u8 {
        // HRAM addresses are 0xFF80-0xFFFE, mapped to 0x00-0x7E internally
        self.data[(addr - 0xFF80) as usize]
    }

    pub fn write_byte(&mut self, addr: u16, value: u8) {
        self.data[(addr - 0xFF80) as usize] = value;
    }
}
