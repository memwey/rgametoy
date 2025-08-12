pub struct Timer {
    div: u16, // Divider Register (0xFF04)
    tima: u8, // Timer Counter (0xFF05)
    tma: u8,  // Timer Modulo (0xFF06)
    tac: u8,  // Timer Control (0xFF07)

    // Internal counters for timing
    div_counter: u16,
    tima_counter: u16,
}

impl Timer {
    pub fn new() -> Timer {
        Timer {
            div: 0x0000,
            tima: 0x00,
            tma: 0x00,
            tac: 0x00,
            div_counter: 0x0000,
            tima_counter: 0x0000,
        }
    }

    pub fn tick(&mut self, cycles: u8) -> bool {
        let mut interrupt_requested = false;

        // Update DIV register (increments at 16384 Hz, so every 256 CPU cycles)
        self.div_counter += cycles as u16;
        if self.div_counter >= 256 {
            self.div_counter -= 256;
            self.div = self.div.wrapping_add(1);
        }

        // Check if timer is enabled (TAC bit 2)
        if self.tac & 0x04 != 0 {
            self.tima_counter += cycles as u16;

            let clock_freq = match self.tac & 0x03 {
                0b00 => 1024, // 4096 Hz (CPU / 1024)
                0b01 => 16,   // 262144 Hz (CPU / 16)
                0b10 => 64,   // 65536 Hz (CPU / 64)
                0b11 => 256,  // 16384 Hz (CPU / 256)
                _ => unreachable!(),
            };

            while self.tima_counter >= clock_freq {
                self.tima_counter -= clock_freq;
                // TIMA increments, if it overflows, reload from TMA and request interrupt
                if self.tima == 0xFF {
                    self.tima = self.tma;
                    interrupt_requested = true;
                } else {
                    self.tima = self.tima.wrapping_add(1);
                }
            }
        }

        interrupt_requested
    }

    pub fn read_register(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.div >> 8) as u8, // Only upper 8 bits of DIV are readable
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            0xFF07 => self.tac,
            _ => 0xFF, // Should not happen
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF04 => self.div = 0, // Writing to DIV resets it
            0xFF05 => self.tima = value,
            0xFF06 => self.tma = value,
            0xFF07 => self.tac = value & 0x07, // Only lower 3 bits are writable
            _ => { /* Should not happen */ }
        }
    }
}
