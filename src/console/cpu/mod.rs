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
/// Instruction execution is *instruction-stepped* and returns the number of
/// T-cycles consumed. Every opcode of the base and `0xCB`-prefixed instruction
/// sets is implemented with hardware-accurate flag behaviour and M-cycle
/// timing. Interrupt dispatch, the `EI` one-instruction delay, `HALT`
/// (including the halt bug) and `DI`/`EI` are handled in [`Cpu::step`].
///
/// The opcode decode lives in the sibling `execute` (base), `cb`
/// (`0xCB`-prefixed) and `alu` submodules.
#[derive(Clone)]
pub struct Cpu {
    registers: Registers,
    /// Interrupt Master Enable.
    ime: bool,
    /// Countdown implementing the one-instruction delay of `EI`.
    /// Set to 2 by `EI`; decremented at the start of each step; when it
    /// reaches 0 the IME flag is enabled.
    ei_delay: u8,
    /// Set while the CPU is halted (waiting for an interrupt).
    halted: bool,
    /// Set when the "halt bug" is triggered: the byte following `HALT` is
    /// fetched without advancing the program counter, so it is executed twice.
    halt_bug: bool,
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            registers: Registers::new(),
            ime: false,
            ei_delay: 0,
            halted: false,
            halt_bug: false,
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
        // Apply the delayed effect of a previous `EI`.
        if self.ei_delay > 0 {
            self.ei_delay -= 1;
            if self.ei_delay == 0 {
                self.ime = true;
            }
        }

        // A pending interrupt wakes the CPU from HALT and, if IME is set,
        // is dispatched before the next instruction.
        let pending = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;
        if pending != 0 {
            self.halted = false;
            if self.ime {
                return self.service_interrupt(bus, pending);
            }
        }

        if self.halted {
            // The CPU is idle for one machine cycle while halted.
            return 4;
        }

        let opcode = self.fetch_byte(bus);
        if self.halt_bug {
            // Undo the PC increment so the byte after HALT executes twice.
            self.halt_bug = false;
            self.registers.pc = self.registers.pc.wrapping_sub(1);
        }
        self.execute(opcode, bus)
    }

    /// Dispatch the highest-priority pending interrupt.
    fn service_interrupt(&mut self, bus: &mut impl Bus, pending: u8) -> u8 {
        self.ime = false;
        // The lowest set bit has the highest priority (VBlank first).
        let bit = pending.trailing_zeros() as u8;
        let if_reg = bus.read_byte(0xFF0F);
        bus.write_byte(0xFF0F, if_reg & !(1 << bit));
        self.push(bus, self.registers.pc);
        self.registers.pc = 0x0040 + (bit as u16) * 8;
        20
    }

    // --- Fetch helpers (advance PC) ---

    fn fetch_byte(&mut self, bus: &mut impl Bus) -> u8 {
        let byte = bus.read_byte(self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        byte
    }

    fn fetch_word(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch_byte(bus) as u16;
        let hi = self.fetch_byte(bus) as u16;
        (hi << 8) | lo
    }

    // --- Register-index helpers (B,C,D,E,H,L,(HL),A -> 0..=7) ---

    fn read_reg(&self, index: u8, bus: &mut impl Bus) -> u8 {
        match index {
            0 => self.registers.get_b(),
            1 => self.registers.get_c(),
            2 => self.registers.get_d(),
            3 => self.registers.get_e(),
            4 => self.registers.get_h(),
            5 => self.registers.get_l(),
            6 => bus.read_byte(self.registers.get_hl()),
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
            6 => bus.write_byte(self.registers.get_hl(), value),
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
        bus.write_byte(self.registers.sp, (value >> 8) as u8);
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        bus.write_byte(self.registers.sp, value as u8);
    }

    fn pop(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = bus.read_byte(self.registers.sp) as u16;
        self.registers.sp = self.registers.sp.wrapping_add(1);
        let hi = bus.read_byte(self.registers.sp) as u16;
        self.registers.sp = self.registers.sp.wrapping_add(1);
        (hi << 8) | lo
    }

    // --- Control flow ---

    fn jr(&mut self, offset: i8) {
        self.registers.pc = self.registers.pc.wrapping_add(offset as i16 as u16);
    }

    fn call(&mut self, bus: &mut impl Bus, addr: u16) {
        self.push(bus, self.registers.pc);
        self.registers.pc = addr;
    }

    fn ret(&mut self, bus: &mut impl Bus) {
        self.registers.pc = self.pop(bus);
    }

    fn halt(&mut self, bus: &mut impl Bus) -> u8 {
        let pending = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;
        if !self.ime && pending != 0 {
            // HALT with interrupts pending but disabled: the CPU does not halt
            // and the next byte is read twice (the halt bug).
            self.halt_bug = true;
        } else {
            self.halted = true;
        }
        4
    }

    /// Conditional relative jump. The operand is always consumed.
    fn jr_cond(&mut self, bus: &mut impl Bus, take: bool) -> u8 {
        let e = self.fetch_byte(bus) as i8;
        if take {
            self.jr(e);
            12
        } else {
            8
        }
    }

    /// Conditional absolute jump. The operand is always consumed.
    fn jp_cond(&mut self, bus: &mut impl Bus, take: bool) -> u8 {
        let addr = self.fetch_word(bus);
        if take {
            self.registers.pc = addr;
            16
        } else {
            12
        }
    }

    /// Conditional call. The operand is always consumed.
    fn call_cond(&mut self, bus: &mut impl Bus, take: bool) -> u8 {
        let addr = self.fetch_word(bus);
        if take {
            self.call(bus, addr);
            24
        } else {
            12
        }
    }

    /// Conditional return.
    fn ret_cond(&mut self, bus: &mut impl Bus, take: bool) -> u8 {
        if take {
            self.ret(bus);
            20
        } else {
            8
        }
    }
}
