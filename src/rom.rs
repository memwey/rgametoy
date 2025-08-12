const ROM_SIZE: usize = 0x8000; // 32KB

pub struct Rom {
    data: [u8; ROM_SIZE],
}

impl Rom {
    pub fn new() -> Rom {
        Rom {
            data: [0; ROM_SIZE],
        }
    }

    pub fn read_byte(&self, addr: u16) -> u8 {
        self.data[addr as usize]
    }

    pub fn write_byte(&mut self, addr: u16, value: u8) {
        // ROM is generally read-only. Writes are typically ignored or used for MBCs.
        // For now, allow writes during initial loading.
        self.data[addr as usize] = value;
    }
}
