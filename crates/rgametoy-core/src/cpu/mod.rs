use crate::bus::Bus;
#[cfg(feature = "serialize")]
use crate::state::{write_bool, write_u8, Reader, SaveStateError};

pub mod registers;

// Instruction decode is split across these submodules, which add `impl Cpu`
// methods to the type defined here.
mod alu;
mod cb;
mod execute;

pub use self::registers::Registers;

/// One M-cycle (or a zero-cycle register action) of an instruction's work,
/// expressed as a closure so the decode can stay a direct transcription of the
/// opcode semantics. The CPU runs one micro-op per M-cycle: each closure either
/// performs exactly one bus access / internal cycle (ticking the machine by 4
/// T-cycles via the `read`/`write`/`tick` helpers) or — riding one of those —
/// only updates registers.
///
/// An instruction whose *whole* body is register work occupies no cycle of its
/// own and so builds no micro-op at all: `decode` runs it inline in the fetch
/// M-cycle. Only zero-cycle work that has to *follow* a later M-cycle (the pair
/// load ending `LD BC, nn`, say) becomes a [`MicroOp::zero`].
///
/// A micro-op is *popped* before it runs, so it owns itself and can reach the
/// rest of the queue back through `cpu.ops` — which is how a *run-time-decoded*
/// sequence enqueues its tail: the `0xCB` prefix only learns its real opcode
/// when it fetches the second byte, so that micro-op appends the CB operation's
/// own micro-ops from inside.
type MicroFn = dyn FnOnce(&mut Cpu, &mut dyn Bus);

pub(crate) struct MicroOp {
    /// Whether this micro-op occupies its own M-cycle. A *ticking* micro-op is
    /// one M-cycle: the machine's clock advances once for it (the caller ticks
    /// the bus), and the micro-op performs that cycle's bus access / internal
    /// work. A *zero-cycle* micro-op does only register work and rides the
    /// preceding ticking micro-op's M-cycle — the clock does not advance for it.
    ticking: bool,
    f: Box<MicroFn>,
}

impl MicroOp {
    /// A micro-op that occupies its own M-cycle (a bus access or an internal
    /// delay): the clock advances once for it.
    fn new(f: impl FnOnce(&mut Cpu, &mut dyn Bus) + 'static) -> MicroOp {
        MicroOp {
            ticking: true,
            f: Box::new(f),
        }
    }

    /// A micro-op that does only register work, riding the preceding ticking
    /// micro-op's M-cycle (the clock does not advance for it).
    fn zero(f: impl FnOnce(&mut Cpu, &mut dyn Bus) + 'static) -> MicroOp {
        MicroOp {
            ticking: false,
            f: Box::new(f),
        }
    }

    /// Execute this micro-op. It performs only its bus access / register work;
    /// advancing the clock (ticking the peripherals) is the *caller's* job,
    /// done once per ticking micro-op.
    fn run(self, cpu: &mut Cpu, bus: &mut dyn Bus) {
        (self.f)(cpu, bus)
    }
}

/// The longest micro-op program an instruction decodes into: `CALL nn` is two
/// operand fetches, an internal cycle, two stack writes and the PC load; the
/// interrupt dispatch is five. Eight leaves headroom and is asserted below.
const OP_CAPACITY: usize = 8;

/// The pending micro-ops of the instruction in flight — a fixed-capacity ring
/// living inline in the [`Cpu`].
///
/// This is popped and re-checked on every M-cycle, which makes it the single
/// hottest data structure in the emulator; a heap-backed `VecDeque` put a load
/// of the buffer pointer and a capacity check on that path for a queue that
/// never exceeds [`OP_CAPACITY`] entries.
pub(crate) struct OpQueue {
    slots: [Option<MicroOp>; OP_CAPACITY],
    head: usize,
    len: usize,
}

impl OpQueue {
    fn new() -> OpQueue {
        OpQueue {
            slots: [const { None }; OP_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    fn push_back(&mut self, op: MicroOp) {
        debug_assert!(self.len < OP_CAPACITY, "micro-op queue overflow");
        self.slots[(self.head + self.len) % OP_CAPACITY] = Some(op);
        self.len += 1;
    }

    fn pop_front(&mut self) -> Option<MicroOp> {
        if self.len == 0 {
            return None;
        }
        let op = self.slots[self.head].take();
        self.head = (self.head + 1) % OP_CAPACITY;
        self.len -= 1;
        op
    }

    /// Whether the next micro-op is a zero-cycle one (a rider on the M-cycle
    /// just run). False when the queue is empty.
    fn next_is_zero_cycle(&self) -> bool {
        self.len != 0 && self.slots[self.head].as_ref().is_some_and(|op| !op.ticking)
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for OpQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// The Sharp SM83 (LR35902) CPU core.
///
/// Execution is *micro-op based*: each opcode decodes into a queue of one
/// M-cycle micro-ops (the crate-internal `MicroOp`), and the CPU runs one
/// micro-op per M-cycle. Every bus access and internal delay is its own, so
/// peripherals move within an instruction — not just between instructions —
/// which is what makes read/write timing observable (Blargg `mem_timing`).
/// This is also what lets the machine later be driven T-cycle by T-cycle from
/// the crystal: the CPU is already decomposed into per-M-cycle steps.
///
/// The opcode decode lives in the sibling `execute` (base), `cb`
/// (`0xCB`-prefixed) and `alu` submodules.
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
    /// Set when an illegal opcode hangs the CPU. Unlike `HALT` this never
    /// resumes (only a reset recovers on real hardware): the CPU stops fetching
    /// and ignores interrupts.
    locked: bool,
    /// T-cycles consumed by the current step (accrued as the machine ticks).
    cycles: u8,
    /// Scratch latches for data flow between an instruction's micro-ops: a byte
    /// fetched on one M-cycle and consumed on a later one (e.g. the low half of
    /// an immediate word, or the value read from `(HL)` before it is written
    /// back). Transient within an instruction; never serialized.
    tmp8: u8,
    tmp16: u16,
    /// The current instruction's pending micro-ops (the fetch micro-op is
    /// enqueued by `fill`; decode appends the rest). Empty at every instruction
    /// boundary, which is the only time the CPU is cloned or serialized — so
    /// the queue is never cloned or written to a save state.
    ops: OpQueue,
    /// Whether an `EI` from the *previous* instruction is still waiting to take
    /// effect, captured at the start of the instruction now running. Together
    /// with `ime_pending` this is what makes `EI`'s promotion fire one
    /// instruction late, and it is live *across* an instruction boundary — so
    /// it is part of the machine's state, cloned and serialized like `ime`. (It
    /// cannot be re-derived on load: it holds `ime_pending` as it stood at the
    /// *previous* boundary, which the current value has already overwritten.)
    ei_was_pending: bool,
}

impl Clone for Cpu {
    fn clone(&self) -> Cpu {
        debug_assert!(
            self.ops.is_empty(),
            "Cpu cloned mid-instruction (ops queue non-empty)"
        );
        Cpu {
            registers: self.registers.clone(),
            ime: self.ime,
            ime_pending: self.ime_pending,
            halted: self.halted,
            halt_bug: self.halt_bug,
            locked: self.locked,
            cycles: self.cycles,
            tmp8: self.tmp8,
            tmp16: self.tmp16,
            ops: OpQueue::new(),
            ei_was_pending: self.ei_was_pending,
        }
    }
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            registers: Registers::new(),
            ime: false,
            ime_pending: false,
            halted: false,
            halt_bug: false,
            locked: false,
            cycles: 0,
            tmp8: 0,
            tmp16: 0,
            ops: OpQueue::new(),
            ei_was_pending: false,
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

    /// Advance the CPU by one M-cycle: run the next *ticking* micro-op (this
    /// M-cycle's bus access / internal work) plus any *zero-cycle* micro-ops
    /// that ride it, then accrue 4 T-cycles. Advancing the clock (ticking the
    /// peripherals) is the *caller's* job, done once per `tick_m`, so a bus
    /// access lands at the end of its M-cycle exactly as before.
    ///
    /// Popping a micro-op moves it out of the queue, so running it can hold
    /// `&mut Cpu` — and reach `cpu.ops` to append (e.g. `0xCB`) — without a
    /// double mutable borrow.
    pub fn tick_m(&mut self, bus: &mut dyn Bus) {
        if let Some(op) = self.ops.pop_front() {
            debug_assert!(op.ticking, "tick_m: expected a ticking micro-op");
            op.run(self, bus);
            self.cycles = self.cycles.wrapping_add(4);
        }
        // Run any zero-cycle micro-ops riding this M-cycle (register work that
        // the clock does not advance for).
        while self.ops.next_is_zero_cycle() {
            let op = self.ops.pop_front().unwrap();
            op.run(self, bus);
        }
    }

    /// Whether the CPU is at an instruction boundary (no micro-ops pending).
    /// This is when `fill` runs, when the machine may be cloned or serialized,
    /// and when `Console` stops a whole-instruction step.
    pub fn at_boundary(&self) -> bool {
        self.ops.is_empty()
    }

    /// T-cycles consumed by the current instruction so far.
    pub fn cycles(&self) -> u8 {
        self.cycles
    }

    /// Decide and queue the next micro-op program: an idle cycle (locked or
    /// halted), an interrupt dispatch, or a normal instruction fetch. Also
    /// applies the previous instruction's delayed `EI` promotion — this runs at
    /// the instruction boundary, which is exactly where the old end-of-step
    /// promotion landed.
    pub fn fill(&mut self, bus: &mut dyn Bus) {
        self.cycles = 0;
        // The delayed `EI` from the previous instruction now takes effect —
        // unless a `DI` in that instruction cancelled it (clearing
        // `ime_pending`). This is the same boundary as the old end-of-step
        // promotion, so the one-instruction delay is preserved.
        if self.ei_was_pending && self.ime_pending {
            self.ime = true;
            self.ime_pending = false;
        }
        self.ei_was_pending = self.ime_pending;

        // The two HALT outcomes are mutually exclusive: `halt()` sets exactly one
        // of them (halted, or the halt-bug), and each is cleared before the other
        // could be set.
        debug_assert!(
            !(self.halted && self.halt_bug),
            "halted and halt_bug are exclusive"
        );

        // An illegal opcode has hung the CPU: it no longer fetches or responds
        // to interrupts. The master clock keeps running, so keep ticking the
        // rest of the machine — the PPU re-renders its frozen state and the
        // frame loop still advances (the game is simply stuck).
        if self.locked {
            Self::emit_tick(&mut self.ops);
            return;
        }

        // A pending interrupt wakes the CPU from HALT and, if IME is set, is
        // dispatched before the next instruction. Polling IF/IE does not
        // consume a cycle, so raw (non-ticking) reads are used.
        let pending = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;
        if pending != 0 {
            self.halted = false;
            if self.ime {
                self.enqueue_interrupt_service();
                return;
            }
        }

        if self.halted {
            // The CPU idles one machine cycle while halted.
            Self::emit_tick(&mut self.ops);
            return;
        }

        // A normal instruction: queue the fetch micro-op. It reads the opcode
        // (one M-cycle), then decodes — appending the remaining micro-ops (and,
        // for `0xCB`, a further run-time decode).
        self.ops.push_back(MicroOp::new(|cpu, bus| {
            let opcode = cpu.fetch_byte(bus);
            if cpu.halt_bug {
                // Undo the PC increment so the byte after HALT executes twice.
                cpu.halt_bug = false;
                cpu.registers.pc = cpu.registers.pc.wrapping_sub(1);
            }
            cpu.decode(opcode, bus);
        }));
    }

    /// Queue the five M-cycle micro-ops of an interrupt dispatch.
    ///
    /// The dispatch is two internal cycles, push the return address high then
    /// low, and a final internal cycle. The interrupt vector is decided only
    /// *after* the high byte has been pushed: the IF/IE re-poll rides the
    /// high-byte write (M3), and the vector decision (read IF, clear the
    /// serviced bit, load PC) rides the low-byte write (M4) — so if `SP` points
    /// at `0xFFFF` the high-byte push overwrites `IE` and is re-sampled,
    /// retargeting or cancelling the dispatch (the `ie_push` quirk).
    fn enqueue_interrupt_service(&mut self) {
        // M1 (internal): clear IME so a second interrupt cannot nest. The clock
        // advances for this M-cycle; the micro-op does only the register work.
        self.ops.push_back(MicroOp::new(|cpu, _| {
            cpu.ime = false;
        }));
        // M2 (internal).
        Self::emit_tick(&mut self.ops);
        // M3: push the high byte of the return address, then re-sample IE & IF
        // into `tmp8` (a non-ticking read on the tail of the write M-cycle).
        self.ops.push_back(MicroOp::new(|cpu, bus| {
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
            let sp = cpu.registers.sp;
            let pc_hi = (cpu.registers.pc >> 8) as u8;
            cpu.write(bus, sp, pc_hi);
            cpu.tmp8 = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;
        }));
        // M4: push the low byte, then choose the vector from the re-sampled
        // `tmp8` and load PC (all non-ticking, on the tail of the write).
        self.ops.push_back(MicroOp::new(|cpu, bus| {
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
            let sp = cpu.registers.sp;
            let pc_lo = cpu.registers.pc as u8;
            cpu.write(bus, sp, pc_lo);
            let pending = cpu.tmp8;
            if pending == 0 {
                // Every enabled interrupt was cancelled mid-dispatch: vector to 0.
                cpu.registers.pc = 0x0000;
            } else {
                // The lowest set bit has the highest priority (VBlank first).
                let bit = pending.trailing_zeros() as u8;
                let if_reg = bus.read_byte(0xFF0F);
                bus.write_byte(0xFF0F, if_reg & !(1 << bit));
                cpu.registers.pc = 0x0040 + (bit as u16) * 8;
            }
        }));
        // M5 (internal): the cycle that loads PC.
        Self::emit_tick(&mut self.ops);
    }

    // --- Timing primitives: every access / internal delay ticks the system ---

    /// Read a byte from the bus. This performs *only* the access — it does not
    /// advance the clock. Advancing time (ticking the peripherals) is the
    /// caller's job, done once per ticking micro-op, so a read lands at the end
    /// of its M-cycle exactly as before.
    fn read(&mut self, bus: &mut dyn Bus, addr: u16) -> u8 {
        bus.read_byte(addr)
    }

    /// Write a byte to the bus (access only; see [`Self::read`]).
    fn write(&mut self, bus: &mut dyn Bus, addr: u16, value: u8) {
        bus.write_byte(addr, value);
    }

    fn fetch_byte(&mut self, bus: &mut dyn Bus) -> u8 {
        let byte = self.read(bus, self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        byte
    }

    // --- Micro-op emitters -------------------------------------------------
    // The decode builds each instruction as a queue of one-M-cycle micro-ops.
    // These helpers push the shared multi-cycle patterns (word fetch, stack
    // push/pop) so the per-opcode decode stays a direct transcription.

    /// Push a micro-op that occupies one M-cycle purely as an internal delay
    /// (no bus access, no register work): the clock advances for it and nothing
    /// else happens.
    fn emit_tick(ops: &mut OpQueue) {
        ops.push_back(MicroOp::new(|_, _| {}));
    }

    /// Push the two fetch micro-ops that read a 16-bit immediate little-endian
    /// into `tmp16` (low byte first).
    fn emit_fetch_word(ops: &mut OpQueue) {
        ops.push_back(MicroOp::new(|cpu, bus| {
            cpu.tmp16 = (cpu.tmp16 & 0xFF00) | cpu.fetch_byte(bus) as u16;
        }));
        ops.push_back(MicroOp::new(|cpu, bus| {
            cpu.tmp16 = ((cpu.fetch_byte(bus) as u16) << 8) | (cpu.tmp16 & 0x00FF);
        }));
    }

    /// Push the two write micro-ops of a stack push: the high byte to `SP-1`,
    /// then the low byte to `SP-2` (SP ends decremented by 2). The value pushed
    /// is read from the CPU at run time by `value`, so a caller can push a
    /// register pair or the program counter (for CALL/RST).
    fn emit_push(ops: &mut OpQueue, value: fn(&Cpu) -> u16) {
        ops.push_back(MicroOp::new(move |cpu, bus| {
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
            let sp = cpu.registers.sp;
            cpu.write(bus, sp, (value(cpu) >> 8) as u8);
        }));
        ops.push_back(MicroOp::new(move |cpu, bus| {
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
            let sp = cpu.registers.sp;
            cpu.write(bus, sp, value(cpu) as u8);
        }));
    }

    /// Push the two read micro-ops of a stack pop into `tmp16` (low byte from
    /// `SP`, high byte from `SP+1`; SP ends incremented by 2).
    fn emit_pop(ops: &mut OpQueue) {
        ops.push_back(MicroOp::new(|cpu, bus| {
            let lo = cpu.read(bus, cpu.registers.sp) as u16;
            cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
            cpu.tmp16 = (cpu.tmp16 & 0xFF00) | lo;
        }));
        ops.push_back(MicroOp::new(|cpu, bus| {
            let hi = cpu.read(bus, cpu.registers.sp) as u16;
            cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
            cpu.tmp16 = (hi << 8) | (cpu.tmp16 & 0x00FF);
        }));
    }

    // --- Register-index helpers (B,C,D,E,H,L,(HL),A -> 0..=7) ---
    // Index 6 accesses (HL) through memory, which ticks; the rest do not.

    fn read_reg(&mut self, index: u8, bus: &mut dyn Bus) -> u8 {
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

    fn write_reg(&mut self, index: u8, value: u8, bus: &mut dyn Bus) {
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

    // --- Control flow ---

    fn jr(&mut self, offset: i8) {
        self.registers.pc = self.registers.pc.wrapping_add(offset as i16 as u16);
    }

    fn halt(&mut self, bus: &mut dyn Bus) {
        let pending = bus.read_byte(0xFFFF) & bus.read_byte(0xFF0F) & 0x1F;
        if !self.ime && pending != 0 {
            // HALT with interrupts pending but disabled: the CPU does not halt
            // and the next byte is read twice (the halt bug).
            self.halt_bug = true;
        } else {
            self.halted = true;
        }
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

// -- Save state -------------------------------------------------------------

impl Cpu {
    /// Append the CPU state: 12 bytes of registers, then seven flag/control
    /// bytes (`ime`, `ime_pending`, `ei_was_pending`, `halted`, `halt_bug`,
    /// `locked`, `cycles`). Order must match [`Self::read_state`].
    #[cfg(feature = "serialize")]
    pub fn write_state(&self, out: &mut Vec<u8>) {
        self.registers.write_state(out);
        write_bool(out, self.ime);
        write_bool(out, self.ime_pending);
        write_bool(out, self.ei_was_pending);
        write_bool(out, self.halted);
        write_bool(out, self.halt_bug);
        write_bool(out, self.locked);
        write_u8(out, self.cycles);
    }

    #[cfg(feature = "serialize")]
    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.registers.read_state(r)?;
        self.ime = r.read_bool()?;
        self.ime_pending = r.read_bool()?;
        self.ei_was_pending = r.read_bool()?;
        self.halted = r.read_bool()?;
        self.halt_bug = r.read_bool()?;
        self.locked = r.read_bool()?;
        self.cycles = r.read_u8()?;
        if self.cycles > 24 || !self.cycles.is_multiple_of(4) || (self.halted && self.halt_bug) {
            return Err(SaveStateError::Corrupt);
        }
        Ok(())
    }
}
