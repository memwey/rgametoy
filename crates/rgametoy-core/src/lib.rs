//! rgametoy-core — the emulated DMG Game Boy: CPU, PPU, APU, timer, cartridge,
//! and the system bus that ties them together.
//!
//! This crate is deterministic and free of host I/O — no window, audio device,
//! filesystem, or wall-clock. It exposes [`Console`]: feed it a cartridge, call
//! [`Console::run_frame`], read back the framebuffer / audio samples, and hand
//! it button state. A frontend (the `rgametoy-desktop` crate, or a future web
//! one) supplies the host I/O. Being dependency-free, it compiles to `wasm32`.

pub mod apu;
pub mod bus;
pub mod cartridge;
pub mod cpu;
#[cfg(feature = "debug")]
pub mod debug;
pub mod hram;
pub mod interrupts;
pub mod joypad;
pub mod ppu;
pub mod serial;
pub mod timer;
pub mod wram;

use crate::bus::{Bus, MemoryBus};
use crate::cartridge::Cartridge;
use crate::cpu::Cpu;

// The Game Boy runs at 4.194304 MHz. One frame is 154 scanlines × 456 dots =
// 70224 T-cycles.
const CYCLES_PER_FRAME: u64 = 70224;

/// The emulated Game Boy: the CPU plus the system bus that owns every
/// memory-mapped peripheral.
#[derive(Clone)]
pub struct Console {
    cpu: Cpu,
    bus: MemoryBus,
    total_cycles: u64,
}

/// A full snapshot of the machine, for instant save/load. It deep-copies the
/// mutable machine state (CPU/PPU/APU/timer, WRAM/HRAM, cartridge RAM); the
/// read-only ROM is shared via `Arc` rather than copied (see `Cartridge`), so a
/// snapshot is self-contained yet cheap.
#[derive(Clone)]
pub struct SaveState(Console);

impl Console {
    pub fn new() -> Console {
        Console {
            cpu: Cpu::new(),
            bus: MemoryBus::new(),
            total_cycles: 0,
        }
    }

    /// Inject a small program into ROM (used by tests).
    pub fn load_program(&mut self, program: &[u8]) {
        self.bus.load_rom(program);
    }

    /// Install a full cartridge and put the machine into its DMG post-boot
    /// state (as if the internal boot ROM had already run).
    pub fn load_cartridge(&mut self, cartridge: Cartridge) {
        self.bus.load_cartridge(cartridge);
        self.power_on();
    }

    /// Registers and I/O registers as left by the DMG boot ROM.
    pub fn power_on(&mut self) {
        self.cpu.set_af(0x01B0);
        self.cpu.set_bc(0x0013);
        self.cpu.set_de(0x00D8);
        self.cpu.set_hl(0x014D);
        self.cpu.set_sp(0xFFFE);
        self.cpu.set_pc(0x0100);

        let io_defaults: [(u16, u8); 12] = [
            (0xFF05, 0x00), // TIMA
            (0xFF06, 0x00), // TMA
            (0xFF07, 0x00), // TAC
            (0xFF40, 0x91), // LCDC: LCD on, BG on, tile data 0x8000
            (0xFF42, 0x00), // SCY
            (0xFF43, 0x00), // SCX
            (0xFF45, 0x00), // LYC
            (0xFF47, 0xFC), // BGP
            (0xFF48, 0xFF), // OBP0
            (0xFF49, 0xFF), // OBP1
            (0xFF4A, 0x00), // WY
            (0xFF4B, 0x00), // WX
        ];
        for (addr, value) in io_defaults {
            self.bus.write_byte(addr, value);
        }
        self.bus.write_byte(0xFF0F, 0xE1);
    }

    /// Fetch/execute one instruction (or service an interrupt), then advance
    /// the peripherals through the bus. Returns T-cycles consumed.
    pub fn step(&mut self) -> u8 {
        // The CPU ticks the bus (peripherals) itself, per M-cycle, as it runs.
        let cycles = self.cpu.step(&mut self.bus);
        self.total_cycles += cycles as u64;
        cycles
    }

    /// Run until the PPU completes one frame (or a full frame of cycles
    /// elapses). Read [`Console::framebuffer`] afterwards to present.
    pub fn run_frame(&mut self) {
        let mut cycles_this_frame = 0u64;
        while cycles_this_frame < CYCLES_PER_FRAME {
            cycles_this_frame += self.step() as u64;
            if self.bus.take_frame_ready() {
                break;
            }
        }
    }

    /// The current frame as 160×144 shade values (0-3).
    pub fn framebuffer(&self) -> &[u8] {
        self.bus.framebuffer()
    }

    /// Drain the APU's buffered stereo samples (at [`audio_output_rate`]).
    ///
    /// [`audio_output_rate`]: Console::audio_output_rate
    pub fn take_audio_samples(&mut self) -> Vec<f32> {
        self.bus.take_audio_samples()
    }

    pub fn audio_output_rate(&self) -> u32 {
        self.bus.audio_output_rate()
    }

    /// Bytes the program has printed over the serial port (test-ROM output).
    pub fn take_serial_output(&mut self) -> Vec<u8> {
        self.bus.take_serial_output()
    }

    /// Capture a full snapshot of the machine (instant save state).
    pub fn save_state(&self) -> SaveState {
        SaveState(self.clone())
    }

    /// Restore a previously captured snapshot (instant load state).
    pub fn load_state(&mut self, state: &SaveState) {
        self.clone_from(&state.0);
    }

    /// Update the joypad button state (0 = pressed). The joypad hardware raises
    /// its interrupt itself, gated by the P1 select lines (a press only
    /// interrupts if its group is currently selected).
    pub fn set_buttons(&mut self, state: u8) {
        self.bus.set_buttons(state);
    }

    pub fn get_cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn get_cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
    }

    pub fn get_bus_mut(&mut self) -> &mut MemoryBus {
        &mut self.bus
    }
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}
