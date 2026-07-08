//! Timer (DIV/TIMA/TMA/TAC), modelled on the real hardware: a 16-bit system
//! counter whose upper byte is DIV, with TIMA clocked by the *falling edge* of
//! a selected counter bit ANDed with the timer-enable. This reproduces the
//! DIV-write and TAC-change glitches and the one-M-cycle TIMA reload delay.

#[derive(Clone)]
pub struct Timer {
    /// 16-bit system counter; DIV (0xFF04) is its upper byte.
    counter: u16,
    /// Counter value before the most recent T-cycle increment — the state a
    /// register store landing on T3 (before that increment) observes.
    prev_counter: u16,
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
            prev_counter: 0,
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
            self.prev_counter = self.counter;
            self.counter = self.counter.wrapping_add(1);
            self.update_edge();
        }
        interrupt
    }

    /// The edge-detector input for an arbitrary TAC value and counter value:
    /// timer-enable ANDed with the counter bit selected by TAC bits 0-1
    /// (4096/262144/65536/16384 Hz).
    fn input_with(tac: u8, counter: u16) -> bool {
        let bit = match tac & 0x03 {
            0b00 => 9,
            0b01 => 3,
            0b10 => 5,
            _ => 7,
        };
        (tac & 0x04 != 0) && (counter >> bit) & 1 == 1
    }

    fn timer_input(&self) -> bool {
        Self::input_with(self.tac, self.counter)
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
                // The TAC store lands on T3 of the write M-cycle, but the bus
                // ticks the timer through T4 before delivering it. Replay the
                // hardware order — write applied at counter-1, then the final
                // counter increment evaluated under the *new* TAC — and
                // reconcile with the edge the already-run T4 saw under the old
                // TAC. This is what lets a TAC enable landing one T-cycle
                // before the selected bit falls still catch that falling edge
                // (mooneye `rapid_toggle`), without moving the global bus
                // write position (which is T4; moving it regresses the
                // control-flow write-timing tests).
                let new_tac = value & 0x07;
                let before = self.prev_counter;
                // Edge already counted by this M-cycle's T4 under the old TAC.
                let t4_old = Self::input_with(self.tac, before)
                    && !Self::input_with(self.tac, self.counter);
                // Edges the hardware ordering produces across write + T4.
                let hw = (Self::input_with(self.tac, before)
                    && !Self::input_with(new_tac, before))
                    || (Self::input_with(new_tac, before)
                        && !Self::input_with(new_tac, self.counter));
                self.tac = new_tac;
                self.prev_input = Self::input_with(new_tac, self.counter);
                if hw && !t4_old {
                    self.increment_tima();
                }
                // The reverse (t4_old && !hw — a phantom increment) needs a
                // frequency switch landing exactly on the old bit's falling
                // edge; leaving it uncorrected is the closest approximation
                // short of unwinding a TIMA increment.
            }
            _ => {}
        }
    }
}
