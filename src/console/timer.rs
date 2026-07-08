//! Timer (DIV/TIMA/TMA/TAC), modelled on the real hardware: a 16-bit system
//! counter whose upper byte is DIV, with TIMA clocked by the *falling edge* of
//! a selected counter bit ANDed with the timer-enable. This reproduces the
//! DIV-write and TAC-change glitches and the one-M-cycle TIMA reload delay.

#[derive(Clone)]
pub struct Timer {
    /// 16-bit system counter; DIV (0xFF04) is its upper byte.
    counter: u16,
    tima: u8, // 0xFF05
    tma: u8,  // 0xFF06
    tac: u8,  // 0xFF07 (lower 3 bits)
    /// Previous state of the TIMA-increment input, for falling-edge detection.
    prev_input: bool,
    /// T-cycles until TIMA is reloaded from TMA after an overflow (0 = none).
    /// TIMA reads 0 during this window.
    reload_delay: u8,
    /// Set for the duration of the M-cycle in which TIMA was just reloaded from
    /// TMA. While set, a TIMA write (which lands at the end of that same
    /// M-cycle) is ignored — on hardware the reload wins over a same-cycle write.
    just_reloaded: bool,
}

impl Timer {
    pub fn new() -> Timer {
        Timer {
            counter: 0,
            tima: 0,
            tma: 0,
            tac: 0,
            prev_input: false,
            reload_delay: 0,
            just_reloaded: false,
        }
    }

    /// Advance the timer by `cycles` T-cycles. Returns `true` if TIMA overflowed
    /// and its interrupt should fire this step.
    pub fn tick(&mut self, cycles: u8) -> bool {
        let mut interrupt = false;
        // Fresh M-cycle: any reload guard only lasts until the write that lands
        // at this M-cycle's end (see `just_reloaded`).
        self.just_reloaded = false;
        for _ in 0..cycles {
            if self.reload_delay > 0 {
                self.reload_delay -= 1;
                if self.reload_delay == 0 {
                    self.tima = self.tma;
                    self.just_reloaded = true;
                    interrupt = true;
                }
            }
            self.counter = self.counter.wrapping_add(1);
            self.update_edge();
        }
        interrupt
    }

    /// The counter bit that drives TIMA, per TAC bits 0-1 (4096/262144/65536/
    /// 16384 Hz).
    fn timer_bit(&self) -> u16 {
        match self.tac & 0x03 {
            0b00 => 9,
            0b01 => 3,
            0b10 => 5,
            _ => 7,
        }
    }

    fn timer_input(&self) -> bool {
        (self.tac & 0x04 != 0) && (self.counter >> self.timer_bit()) & 1 == 1
    }

    /// TIMA increments on the falling edge of the timer input.
    fn update_edge(&mut self) {
        let input = self.timer_input();
        if self.prev_input && !input {
            self.increment_tima();
        }
        self.prev_input = input;
    }

    fn increment_tima(&mut self) {
        let (result, overflow) = self.tima.overflowing_add(1);
        if overflow {
            self.tima = 0; // reads 0 until the reload one M-cycle later
            self.reload_delay = 4;
        } else {
            self.tima = result;
        }
    }

    /// `(DIV, TIMA, TMA, TAC)` for the `debug` inspector.
    #[cfg(feature = "debug")]
    pub fn debug_state(&self) -> (u8, u8, u8, u8) {
        ((self.counter >> 8) as u8, self.tima, self.tma, self.tac)
    }

    pub fn read_register(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.counter >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            0xFF07 => self.tac | 0xF8, // unused bits read as 1
            _ => 0xFF,
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF04 => {
                // Writing DIV resets the counter; if the timer input was high
                // this is a falling edge (a spurious TIMA increment).
                self.counter = 0;
                self.update_edge();
            }
            0xFF05 => {
                // A write on the exact reload cycle is ignored (the TMA reload
                // wins); otherwise the write lands and cancels a pending reload.
                if !self.just_reloaded {
                    self.tima = value;
                    self.reload_delay = 0;
                }
            }
            0xFF06 => {
                self.tma = value;
                // TMA written on the exact reload cycle: TIMA takes the new
                // value, since TMA is updated before it is latched into TIMA.
                if self.just_reloaded {
                    self.tima = value;
                }
            }
            0xFF07 => {
                self.tac = value & 0x07;
                // Changing enable/frequency can also produce a falling edge.
                self.update_edge();
            }
            _ => {}
        }
    }
}
