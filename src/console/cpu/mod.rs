use crate::console::bus::Bus;

pub mod registers;

// Instruction decode is split across these submodules, which add `impl Cpu`
// methods to the type defined here.
mod alu;
mod cb;
mod execute;

pub use self::registers::Registers;

/// The Sharp SM83 (LR35902) CPU core.
///
/// Execution is *cycle-stepped*: every memory access and every internal delay
/// advances the rest of the system by one M-cycle (4 T-cycles) via
/// [`Bus::tick`], so peripherals move within an instruction — not just between
/// instructions. This makes read/write timing observable (Blargg `mem_timing`).
///
/// The opcode decode lives in the sibling `execute` (base), `cb`
/// (`0xCB`-prefixed) and `alu` submodules.
#[derive(Clone)]
pub struct Cpu {
    registers: Registers,
    /// Interrupt Master Enable.
    ime: bool,
    /// Set by `EI`; `IME` becomes true after the *next* instruction retires
    /// (the one-instruction delay). A later `DI` clears it before it takes hold.
    ime_pending: bool,
    /// Set while the CPU is halted (waiting for an interrupt).
    halted: bool,
    /// Set when the "halt bug" is triggered: the byte following `HALT` is
    /// fetched without advancing the program counter, so it is executed twice.
    halt_bug: bool,
    /// T-cycles consumed by the current step (accrued as the machine ticks).
    cycles: u8,
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            registers: Registers::new(),
            ime: false,
            ime_pending: false,
            halted: false,
            halt_bug: false,
            cycles: 0,
        }
    }

    pub fn get_registers(&self) -> &Registers {
        &self.registers
    }

    pub fn get_registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }

    pub fn set_hl(&mut self, value: u16) {
        self.registers.set_hl(value);
    }

    pub fn set_bc(&mut self, value: u16) {
        self.registers.set_bc(value);
    }

    pub fn set_de(&mut self, value: u16) {
        self.registers.set_de(value);
    }

    pub fn set_af(&mut self, value: u16) {
        self.registers.set_af(value);
    }

    pub fn get_pc(&self) -> u16 {
        self.registers.pc
    }

    pub fn set_pc(&mut self, value: u16) {
        self.registers.pc = value;
    }

    pub fn get_sp(&self) -> u16 {
        self.registers.sp
    }

    pub fn set_sp(&mut self, value: u16) {
        self.registers.sp = value;
    }

    pub fn is_halted(&self) -> bool {
        self.halted
    }

    pub fn ime_enabled(&self) -> bool {
        self.ime
    }

    pub fn enable_interrupts(&mut self) {
        self.ime = true;
    }

    pub fn disable_interrupts(&mut self) {
        self.ime = false;
    }

    /// Execute a single CPU step: service a pending interrupt if one is due,
    /// otherwise fetch and execute one instruction. Returns the number of
    /// T-cycles consumed.
    pub fn step(&mut self, bus: &mut impl Bus) -> u8 {
        self.cycles = 0;

        // Capture whether an `EI` from the *previous* instruction is waiting to
        // take effect. Its `IME` promotion happens after this instruction runs,
        // so a chain of `EI`s still enables interrupts after just one step.
        let ei_was_pending = self.ime_pending;

        // A pending interrupt wakes the CPU from HALT and, if IME is set, is
        // dispatched before the next instruction. Polling IF/IE does not
        // consume a cycle, so raw (non-ticking) reads are used.
        let pending = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;
        if pending != 0 {
            self.halted = false;
            if self.ime {
                self.service_interrupt(bus, pending);
                return self.cycles;
            }
        }

        if self.halted {
            // The CPU idles one machine cycle while halted.
            self.tick(bus);
            return self.cycles;
        }

        let opcode = self.fetch_byte(bus);
        if self.halt_bug {
            // Undo the PC increment so the byte after HALT executes twice.
            self.halt_bug = false;
            self.registers.pc = self.registers.pc.wrapping_sub(1);
        }
        self.execute(opcode, bus);

        // The delayed `EI` now takes effect — unless a `DI` in this very
        // instruction cancelled it (which clears `ime_pending`).
        if ei_was_pending && self.ime_pending {
            self.ime = true;
            self.ime_pending = false;
        }
        self.cycles
    }

    /// Dispatch the highest-priority pending interrupt (5 M-cycles).
    ///
    /// The interrupt vector is only decided *after* the high byte of the return
    /// address has been pushed: if `SP` points at `0xFFFF`, that push overwrites
    /// `IE`, which can retarget the vector or — if it clears every enabled bit —
    /// cancel the dispatch entirely and jump to `0x0000` (the `ie_push` quirk).
    fn service_interrupt(&mut self, bus: &mut impl Bus, _pending: u8) {
        self.ime = false;
        self.tick(bus); // internal
        self.tick(bus); // internal

        // Push the high byte, then re-sample IE & IF to choose the vector.
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        self.write(bus, self.registers.sp, (self.registers.pc >> 8) as u8);
        let pending = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;

        // Push the low byte.
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        self.write(bus, self.registers.sp, self.registers.pc as u8);

        if pending == 0 {
            // Every enabled interrupt was cancelled mid-dispatch: vector to 0.
            self.registers.pc = 0x0000;
        } else {
            // The lowest set bit has the highest priority (VBlank first).
            let bit = pending.trailing_zeros() as u8;
            let if_reg = bus.read_byte(0xFF0F);
            bus.write_byte(0xFF0F, if_reg & !(1 << bit));
            self.registers.pc = 0x0040 + (bit as u16) * 8;
        }
        self.tick(bus); // set PC
    }

    // --- Timing primitives: every access / internal delay ticks the system ---

    /// Advance the rest of the machine by one M-cycle.
    fn tick(&mut self, bus: &mut impl Bus) {
        bus.tick(4);
        self.cycles = self.cycles.wrapping_add(4);
    }

    fn read(&mut self, bus: &mut impl Bus, addr: u16) -> u8 {
        self.tick(bus);
        bus.read_byte(addr)
    }

    fn write(&mut self, bus: &mut impl Bus, addr: u16, value: u8) {
        self.tick(bus);
        bus.write_byte(addr, value);
    }

    fn fetch_byte(&mut self, bus: &mut impl Bus) -> u8 {
        let byte = self.read(bus, self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        byte
    }

    fn fetch_word(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch_byte(bus) as u16;
        let hi = self.fetch_byte(bus) as u16;
        (hi << 8) | lo
    }

    // --- Register-index helpers (B,C,D,E,H,L,(HL),A -> 0..=7) ---
    // Index 6 accesses (HL) through memory, which ticks; the rest do not.

    fn read_reg(&mut self, index: u8, bus: &mut impl Bus) -> u8 {
        match index {
            0 => self.registers.get_b(),
            1 => self.registers.get_c(),
            2 => self.registers.get_d(),
            3 => self.registers.get_e(),
            4 => self.registers.get_h(),
            5 => self.registers.get_l(),
            6 => self.read(bus, self.registers.get_hl()),
            7 => self.registers.get_a(),
            _ => unreachable!(),
        }
    }

    fn write_reg(&mut self, index: u8, value: u8, bus: &mut impl Bus) {
        match index {
            0 => self.registers.set_b(value),
            1 => self.registers.set_c(value),
            2 => self.registers.set_d(value),
            3 => self.registers.set_e(value),
            4 => self.registers.set_h(value),
            5 => self.registers.set_l(value),
            6 => {
                let hl = self.registers.get_hl();
                self.write(bus, hl, value);
            }
            7 => self.registers.set_a(value),
            _ => unreachable!(),
        }
    }

    fn set_flags(&mut self, z: bool, n: bool, h: bool, c: bool) {
        self.registers.set_flag_z(z);
        self.registers.set_flag_n(n);
        self.registers.set_flag_h(h);
        self.registers.set_flag_c(c);
    }

    // --- Stack ---

    fn push(&mut self, bus: &mut impl Bus, value: u16) {
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        self.write(bus, self.registers.sp, (value >> 8) as u8);
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        self.write(bus, self.registers.sp, value as u8);
    }

    fn pop(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.read(bus, self.registers.sp) as u16;
        self.registers.sp = self.registers.sp.wrapping_add(1);
        let hi = self.read(bus, self.registers.sp) as u16;
        self.registers.sp = self.registers.sp.wrapping_add(1);
        (hi << 8) | lo
    }

    // --- Control flow (internal M-cycles are included where the timing is
    //     unconditional). ---

    fn jr(&mut self, offset: i8) {
        self.registers.pc = self.registers.pc.wrapping_add(offset as i16 as u16);
    }

    /// CALL / RST: one internal M-cycle, then push the return address.
    fn call(&mut self, bus: &mut impl Bus, addr: u16) {
        self.tick(bus);
        self.push(bus, self.registers.pc);
        self.registers.pc = addr;
    }

    /// RET / RETI: pop the return address, then one internal M-cycle.
    fn ret(&mut self, bus: &mut impl Bus) {
        let addr = self.pop(bus);
        self.tick(bus);
        self.registers.pc = addr;
    }

    fn halt(&mut self, bus: &mut impl Bus) {
        let pending = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;
        if !self.ime && pending != 0 {
            // HALT with interrupts pending but disabled: the CPU does not halt
            // and the next byte is read twice (the halt bug).
            self.halt_bug = true;
        } else {
            self.halted = true;
        }
    }

    /// Conditional relative jump. The operand is always consumed; a taken
    /// branch costs one extra internal M-cycle.
    fn jr_cond(&mut self, bus: &mut impl Bus, take: bool) {
        let e = self.fetch_byte(bus) as i8;
        if take {
            self.tick(bus);
            self.jr(e);
        }
    }

    fn jp_cond(&mut self, bus: &mut impl Bus, take: bool) {
        let addr = self.fetch_word(bus);
        if take {
            self.tick(bus);
            self.registers.pc = addr;
        }
    }

    fn call_cond(&mut self, bus: &mut impl Bus, take: bool) {
        let addr = self.fetch_word(bus);
        if take {
            self.call(bus, addr);
        }
    }

    /// RET cc: one internal M-cycle to test the condition, then a normal RET if
    /// taken.
    fn ret_cond(&mut self, bus: &mut impl Bus, take: bool) {
        self.tick(bus);
        if take {
            self.ret(bus);
        }
    }
}
