//! Read-only machine inspection (`--features debug`).
//!
//! A [`Snapshot`] captures the full observable state of the machine in one
//! shot — CPU, interrupts, PPU (including the *internal* mode/dot and the
//! STAT-line / LY==LYC latch that no register exposes), timer and OAM DMA — and
//! [`Console::run_until`] steps to a condition. Together they replace the
//! throwaway trace harnesses used to calibrate against hardware test ROMs.
//!
//! ```no_run
//! # use rgametoy_core::Console;
//! # let mut console = Console::new();
//! // Break when the CPU reaches a PC, then print everything.
//! console.run_until(1_000_000, |c| c.cpu().get_pc() == 0x0048);
//! println!("{}", console.snapshot());
//! ```

use super::Console;

/// A one-shot view of the whole machine. Fields mirror hardware registers plus
/// internal PPU/DMA state that is otherwise invisible.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub cycle: u64,
    // CPU
    pub pc: u16,
    pub sp: u16,
    pub af: u16,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub ime: bool,
    pub halted: bool,
    pub if_reg: u8,
    pub ie_reg: u8,
    // PPU
    pub lcdc: u8,
    pub stat: u8,     // as read (mode bits lag the internal mode)
    pub ppu_mode: u8, // internal mode, no lag
    pub ly: u8,
    pub lyc: u8,
    pub dots: u16,
    pub lyc_match: bool,
    pub stat_line: bool,
    // Timer
    pub div: u8,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    // OAM DMA
    pub dma_active: bool,
    pub dma_src: u8,
}

impl std::fmt::Display for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cyc={:>8} PC={:04X} SP={:04X} AF={:04X} BC={:04X} DE={:04X} HL={:04X} \
             IME={} HALT={} IF={:02X} IE={:02X} | LCDC={:02X} STAT={:02X} m{} \
             LY={:>3} LYC={:>3} dot={:>3} lyc_m={} stat_l={} | \
             DIV={:02X} TIMA={:02X} TMA={:02X} TAC={:02X} | DMA={} src={:02X}",
            self.cycle,
            self.pc,
            self.sp,
            self.af,
            self.bc,
            self.de,
            self.hl,
            self.ime as u8,
            self.halted as u8,
            self.if_reg,
            self.ie_reg,
            self.lcdc,
            self.stat,
            self.ppu_mode,
            self.ly,
            self.lyc,
            self.dots,
            self.lyc_match as u8,
            self.stat_line as u8,
            self.div,
            self.tima,
            self.tma,
            self.tac,
            self.dma_active as u8,
            self.dma_src,
        )
    }
}

impl Console {
    /// Capture the full observable state right now.
    pub fn snapshot(&self) -> Snapshot {
        let r = self.cpu.get_registers();
        let (ppu_mode, dots, lyc_match, stat_line) = self.soc.debug_ppu();
        let (div, tima, tma, tac) = self.soc.debug_timer();
        let (dma_active, dma_src) = self.soc.debug_dma();
        Snapshot {
            cycle: self.total_cycles,
            pc: r.get_pc(),
            sp: r.get_sp(),
            af: r.get_af(),
            bc: r.get_bc(),
            de: r.get_de(),
            hl: r.get_hl(),
            ime: self.cpu.ime_enabled(),
            halted: self.cpu.is_halted(),
            if_reg: self.read_mem(0xFF0F) & 0x1F,
            ie_reg: self.read_mem(0xFFFF) & 0x1F,
            lcdc: self.read_mem(0xFF40),
            stat: self.read_mem(0xFF41),
            ppu_mode,
            ly: self.read_mem(0xFF44),
            lyc: self.read_mem(0xFF45),
            dots,
            lyc_match,
            stat_line,
            div,
            tima,
            tma,
            tac,
            dma_active,
            dma_src,
        }
    }

    /// Read an arbitrary byte through the bus (no side effects on read).
    pub fn peek(&self, addr: u16) -> u8 {
        self.read_mem(addr)
    }

    /// Step until `pred` holds (checked *before* each step, so it can break at a
    /// breakpoint PC before that instruction runs) or `max_steps` elapse.
    /// Returns `true` if the predicate was met.
    pub fn run_until(&mut self, max_steps: u64, mut pred: impl FnMut(&Console) -> bool) -> bool {
        for _ in 0..max_steps {
            if pred(self) {
                return true;
            }
            self.step();
        }
        pred(self)
    }
}
