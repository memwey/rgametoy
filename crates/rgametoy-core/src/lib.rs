//! rgametoy-core — the emulated DMG Game Boy: CPU, PPU, APU, timer, cartridge,
//! and the system bus that ties them together.
//!
//! This crate is deterministic and free of host I/O — no window, audio device,
//! filesystem, or wall-clock. It exposes [`Console`]: feed it a cartridge, call
//! [`Console::run_frame`], read back the framebuffer / audio samples, and hand
//! it button state. A frontend (the `rgametoy-desktop` or `rgametoy-web`
//! crate) supplies the host I/O. Being dependency-free, it compiles to
//! `wasm32`.

pub mod apu;
pub mod bus;
pub mod cartridge;
pub mod cpu;
#[cfg(feature = "debug")]
pub mod debug;
pub mod dma;
pub mod hram;
pub mod interrupts;
pub mod intctrl;
pub mod joypad;
pub mod ppu;
pub mod serial;
/// Whole-machine save-state serialization — an *emulator* convenience, not part
/// of the Game Boy — behind the `serialize` feature. The core turns state into
/// bytes and back; persisting those bytes (or holding them in memory) is the
/// frontend's call. See the module docs.
#[cfg(feature = "serialize")]
pub mod state;
pub mod timer;
pub mod wram;

use crate::bus::{bus_read, Bus, BusView, Soc};
use crate::cartridge::Cartridge;
use crate::cpu::Cpu;
#[cfg(feature = "serialize")]
use crate::state::{
    crc32, write_u32_le, write_u64_le, Reader, SaveStateError, SAVE_STATE_MAGIC, SAVE_STATE_VERSION,
};

// The Game Boy runs at 4.194304 MHz. One frame is 154 scanlines × 456 dots =
// 70224 T-cycles.
const CYCLES_PER_FRAME: u64 = 70224;

/// The emulated Game Boy handheld: the CPU, the memory-mapped [`Soc`], and
/// the inserted [`Cartridge`]. The cartridge is a distinct unit (a real game pak
/// is separate hardware) — it is injected at [`power_on`](Console::power_on) and
/// reachable via [`cartridge`](Console::cartridge); the bus borrows it per step
/// (`BusView`) rather than owning it.
#[derive(Clone)]
pub struct Console {
    cpu: Cpu,
    soc: Soc,
    cartridge: Cartridge,
    total_cycles: u64,
}

impl Console {
    pub fn new() -> Console {
        Console {
            cpu: Cpu::new(),
            soc: Soc::new(),
            cartridge: Cartridge::new(),
            total_cycles: 0,
        }
    }

    /// Inject a small program into the inserted cartridge's ROM (used by tests).
    pub fn load_program(&mut self, program: &[u8]) {
        self.cartridge.load(program);
    }

    /// Insert `cartridge` and boot into the DMG post-boot state (as if the
    /// internal boot ROM had already run) — the faithful "insert game pak, flip
    /// the power switch". The cartridge stays owned by the console until the
    /// next `power_on`.
    pub fn power_on(&mut self, cartridge: Cartridge) {
        self.cartridge = cartridge;
        self.power_cycle();
    }

    /// Power-cycle the DMG with the currently inserted cartridge. The DMG has
    /// no user-facing reset button: this models turning the handheld off and
    /// back on. Battery RAM survives, while the cartridge controller, CPU and
    /// handheld hardware return to their power-on state.
    ///
    /// The internal boot ROM is not executed; instead the machine enters the
    /// deterministic post-boot state that the DMG boot ROM would leave behind.
    pub fn power_cycle(&mut self) {
        self.cpu = Cpu::new();
        self.soc = Soc::new();
        self.total_cycles = 0;
        self.cartridge.reset_controller();
        self.initialize_post_boot_state();
    }

    /// Apply the register and MMIO values observed after the DMG boot ROM.
    /// Kept separate from `power_cycle` so the boot-ROM bypass is explicit.
    fn initialize_post_boot_state(&mut self) {
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
        let mut bus = BusView::new(&mut self.soc, &mut self.cartridge);
        for (addr, value) in io_defaults {
            bus.write_byte(addr, value);
        }
        bus.write_byte(0xFF0F, 0xE1);
    }

    /// Fetch/execute one instruction (or service an interrupt), then advance
    /// the peripherals through the bus. Returns T-cycles consumed.
    pub fn step(&mut self) -> u8 {
        // Assemble the transient bus (system + inserted cartridge) and let the
        // CPU drive it; it ticks the peripherals itself, per M-cycle.
        let mut bus = BusView::new(&mut self.soc, &mut self.cartridge);
        let cycles = self.cpu.step(&mut bus);
        self.total_cycles += cycles as u64;
        cycles
    }

    /// Run until the PPU completes one frame (or a full frame of cycles
    /// elapses). Read [`Console::framebuffer`] afterwards to present.
    pub fn run_frame(&mut self) {
        let mut cycles_this_frame = 0u64;
        while cycles_this_frame < CYCLES_PER_FRAME {
            cycles_this_frame += self.step() as u64;
            if self.soc.take_frame_ready() {
                break;
            }
        }
    }

    /// The current frame as 160×144 shade values (0-3).
    pub fn framebuffer(&self) -> &[u8] {
        self.soc.framebuffer()
    }

    /// Drain the APU's buffered stereo samples (at [`audio_output_rate`]).
    ///
    /// [`audio_output_rate`]: Console::audio_output_rate
    pub fn take_audio_samples(&mut self) -> Vec<f32> {
        self.soc.take_audio_samples()
    }

    pub fn audio_output_rate(&self) -> u32 {
        self.soc.audio_output_rate()
    }

    /// Total T-cycles executed since power-on. Divided by the 70224 cycles in a
    /// DMG frame this is the emulated frame count — a frontend can sample it over
    /// a wall-clock second to show the true emulation rate (independent of how
    /// often it repaints).
    pub fn total_cycles(&self) -> u64 {
        self.total_cycles
    }

    /// The inserted cartridge — the owner of battery-RAM persistence. Battery
    /// saves are a cartridge concern, so a frontend flushes `.save_ram_bytes()`
    /// / checks `.ram_dirty()` here directly; the console does not mediate them.
    pub fn cartridge(&self) -> &Cartridge {
        &self.cartridge
    }

    pub fn cartridge_mut(&mut self) -> &mut Cartridge {
        &mut self.cartridge
    }

    /// Read a byte through the memory map (no side effects). For tests and the
    /// `debug` inspector.
    pub fn read_mem(&self, addr: u16) -> u8 {
        bus_read(&self.soc, &self.cartridge, addr)
    }

    /// Write a byte through the memory map (routes to the cartridge/MBC, VRAM,
    /// I/O, …). For tests that set up memory state.
    pub fn write_mem(&mut self, addr: u16, value: u8) {
        let mut bus = BusView::new(&mut self.soc, &mut self.cartridge);
        bus.write_byte(addr, value);
    }

    /// Bytes the program has printed over the serial port (test-ROM output).
    pub fn take_serial_output(&mut self) -> Vec<u8> {
        self.soc.take_serial_output()
    }

    /// Serialize the entire machine — including the inserted cartridge's state —
    /// to a portable byte blob (magic, version, a CRC32, then each module's
    /// state). This is the sole save-state mechanism: hold the bytes in memory
    /// for an instant slot, or write them to disk / IndexedDB to persist. The
    /// on-disk layout is in `state.rs`.
    #[cfg(feature = "serialize")]
    pub fn save_state_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&SAVE_STATE_MAGIC);
        out.push(SAVE_STATE_VERSION);
        let crc_pos = out.len();
        write_u32_le(&mut out, 0); // placeholder, filled in below
        let body_start = out.len();

        write_u64_le(&mut out, self.total_cycles);
        self.cpu.write_state(&mut out);
        self.cartridge.write_state(&mut out);
        self.soc.write_state(&mut out);

        let crc = crc32(&out[body_start..]);
        out[crc_pos..crc_pos + 4].copy_from_slice(&crc.to_le_bytes());
        out
    }

    /// Restore a machine from a `save_state_bytes` blob. The CRC32 is verified
    /// against the payload before any state is applied, so a corrupt blob never
    /// partially overwrites a running session. The cartridge's ROM is *not* part
    /// of the blob — the same ROM must be inserted (via [`Console::power_on`])
    /// before calling this, so the cartridge's `ram` size matches the save state.
    #[cfg(feature = "serialize")]
    pub fn load_state_bytes(&mut self, bytes: &[u8]) -> Result<(), SaveStateError> {
        if bytes.len() < SAVE_STATE_MAGIC.len() + 1 + 4 {
            return Err(SaveStateError::Truncated);
        }
        if bytes[0..4] != SAVE_STATE_MAGIC {
            return Err(SaveStateError::BadMagic);
        }
        let version = bytes[4];
        if version != SAVE_STATE_VERSION {
            return Err(SaveStateError::UnsupportedVersion(version));
        }
        let crc = u32::from_le_bytes(bytes[5..9].try_into().unwrap());
        let payload = &bytes[9..];
        if crc32(payload) != crc {
            return Err(SaveStateError::CrcMismatch);
        }
        // Parse into a clone and commit only on success, so a mid-parse error
        // (e.g. an unknown cartridge kind when the wrong ROM is inserted) never
        // leaves the live machine half-overwritten. The clone is cheap — the
        // read-only ROM is shared via `Arc`, not copied.
        let mut next = self.clone();
        let mut r = Reader::new(payload);
        next.total_cycles = r.read_u64_le()?;
        next.cpu.read_state(&mut r)?;
        next.cartridge.read_state(&mut r)?;
        next.soc.read_state(&mut r)?;
        if r.remaining() != 0 {
            return Err(SaveStateError::Corrupt);
        }
        *self = next;
        Ok(())
    }

    /// Update the joypad button state (0 = pressed). The joypad hardware raises
    /// its interrupt itself, gated by the P1 select lines (a press only
    /// interrupts if its group is currently selected).
    pub fn set_buttons(&mut self, state: u8) {
        self.soc.set_buttons(state);
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
    }
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}
