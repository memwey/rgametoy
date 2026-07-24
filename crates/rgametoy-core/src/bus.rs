use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::dma::DmaController;
use crate::hram::Hram;
use crate::intctrl::IntCtrl;
use crate::interrupts::InterruptType;
use crate::joypad::P1;
use crate::ppu::Ppu;
use crate::serial::Serial;
#[cfg(feature = "serialize")]
use crate::state::{Reader, SaveStateError};
use crate::timer::Timer;
use crate::wram::Wram;

/// Minimal memory/bus interface: address-decoded reads and writes, plus
/// `tick` to advance the memory-mapped peripherals. The CPU uses it to reach
/// memory; `Console::step` (the master clock) calls `tick` once per M-cycle to
/// advance the peripherals — the CPU does *not* drive it.
pub trait Bus {
    fn read_byte(&self, addr: u16) -> u8;
    fn write_byte(&mut self, addr: u16, value: u8);
    /// Advance the memory-mapped peripherals by `cycles` T-cycles. Called by
    /// the clock driver (`Console::step`), not by the CPU.
    fn tick(&mut self, cycles: u8);
}

/// The SoC (system-on-a-chip): every on-die component the CPU can reach
/// *except* the cartridge. On a real DMG the CPU core, PPU, APU, timer, serial,
/// joypad, interrupt controller, OAM-DMA unit, WRAM and HRAM are all one die;
/// the cartridge is a separate unit (inserted at power-on, owned by `Console`)
/// that the SoC borrows per step — see [`BusView`]. This split mirrors the
/// hardware: the handheld and the game pak are distinct.
///
/// The clock tree is driven from above: `Console::step` ticks these peripherals
/// once per M-cycle (via `Bus::tick`) and ticks the CPU's micro-op as a peer —
/// the crystal-driven model where no single component is the master.
#[derive(Clone)]
pub struct Soc {
    wram: Wram,
    hram: Hram,
    p1: P1,
    ppu: Ppu,
    timer: Timer,
    apu: Apu,
    serial: Serial,
    /// The OAM DMA controller — a bus master (the 0xFF46 unit), distinct from
    /// the PPU.
    dma: DmaController,
    intctrl: IntCtrl,
}

impl Soc {
    pub fn new() -> Soc {
        Soc {
            wram: Wram::new(),
            hram: Hram::new(),
            p1: P1::new(),
            ppu: Ppu::new(),
            timer: Timer::new(),
            apu: Apu::new(),
            serial: Serial::new(),
            dma: DmaController::new(),
            intctrl: IntCtrl::new(),
        }
    }

    /// Bytes the program has shifted out over the serial port (test-ROM output).
    pub fn take_serial_output(&mut self) -> Vec<u8> {
        self.serial.take_output()
    }

    /// Returns `true` once per completed frame (consumes the flag).
    pub fn take_frame_ready(&mut self) -> bool {
        self.ppu.take_frame_ready()
    }

    pub fn framebuffer(&self) -> &[u8] {
        self.ppu.framebuffer()
    }

    pub fn take_audio_samples(&mut self) -> Vec<f32> {
        self.apu.take_samples()
    }

    pub fn audio_output_rate(&self) -> u32 {
        self.apu.output_rate()
    }

    pub fn set_buttons(&mut self, state: u8) {
        // The joypad hardware raises the interrupt itself, gated by the P1
        // select lines — a press only interrupts if its group is selected.
        if self.p1.update_button_state(state) {
            self.intctrl.request(InterruptType::Joypad);
        }
    }

    pub fn request_interrupt(&mut self, interrupt_type: InterruptType) {
        self.intctrl.request(interrupt_type);
    }

    /// OAM DMA state `(active, source page)` for the `debug` inspector.
    #[cfg(feature = "debug")]
    pub fn debug_dma(&self) -> (bool, u8) {
        self.dma.debug_state()
    }

    /// Internal PPU state for the inspector (the system owns the PPU privately).
    #[cfg(feature = "debug")]
    pub fn debug_ppu(&self) -> (u8, u16, bool, bool) {
        self.ppu.debug_state()
    }

    /// Internal timer state for the inspector.
    #[cfg(feature = "debug")]
    pub fn debug_timer(&self) -> (u8, u8, u8, u8) {
        self.timer.debug_state()
    }
}

impl Default for Soc {
    fn default() -> Self {
        Self::new()
    }
}

// -- Memory read path -------------------------------------------------------
// Free functions (same module, so they can touch `Soc`'s private fields)
// shared by the mutable `BusView` and by read-only callers such as the `debug`
// inspector, which need to read memory through a `&self` Console.

/// Address-decoded read with no OAM-DMA blocking applied. [`bus_read`] layers the
/// block on top; the DMA source copy uses this directly.
fn read_raw(soc: &Soc, cart: &Cartridge, addr: u16) -> u8 {
    match addr {
        0x0000..=0x7FFF => cart.read_rom(addr),
        0x8000..=0x9FFF => soc.ppu.read_vram(addr),
        0xA000..=0xBFFF => cart.read_ram(addr),
        0xC000..=0xDFFF => soc.wram.read_byte(addr),
        0xE000..=0xFDFF => soc.wram.read_byte(addr), // Echo RAM
        0xFE00..=0xFE9F => soc.ppu.read_oam(addr),
        0xFEA0..=0xFEFF => 0xFF, // Not usable
        0xFF00 => soc.p1.read_register(),
        0xFF01 | 0xFF02 => soc.serial.read_register(addr),
        0xFF04..=0xFF07 => soc.timer.read_register(addr),
        0xFF0F => soc.intctrl.read(addr),
        0xFF10..=0xFF3F => soc.apu.read_register(addr),
        // 0xFF46 is the DMA unit's register, owned by the DMA controller (see
        // `DmaController`) — it sits in the PPU's address range but is not a
        // PPU register.
        0xFF46 => soc.dma.source(),
        0xFF40..=0xFF4B => soc.ppu.read_register(addr),
        0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => 0xFF,
        0xFF80..=0xFFFE => soc.hram.read_byte(addr),
        0xFFFF => soc.intctrl.read(addr),
    }
}

/// A CPU read: open bus (0xFF) if it conflicts with an active OAM DMA, else the
/// address-decoded byte. Immutable, so the inspector can peek through `&self`.
pub(crate) fn bus_read(soc: &Soc, cart: &Cartridge, addr: u16) -> u8 {
    if soc.dma.conflicts(addr) {
        return 0xFF;
    }
    read_raw(soc, cart, addr)
}

/// A transient pairing of the console's [`Soc`] with the borrowed
/// [`Cartridge`], assembled per step so the clock driver and the CPU can both
/// reach the whole memory map. It owns neither side.
pub struct BusView<'a> {
    soc: &'a mut Soc,
    cart: &'a mut Cartridge,
}

impl<'a> BusView<'a> {
    pub fn new(soc: &'a mut Soc, cart: &'a mut Cartridge) -> BusView<'a> {
        BusView { soc, cart }
    }

    /// Activate a pending OAM DMA once its startup delay elapses: copy 160 bytes
    /// from `source << 8` into OAM and open the 640-T-cycle busy window (during
    /// which the CPU keeps I/O, HRAM and whichever of the two buses the transfer
    /// is *not* driving — see [`DmaController::conflicts`]). The copy is atomic
    /// here; since OAM is blocked for the whole window the CPU cannot tell it
    /// from a byte-by-byte transfer.
    fn start_oam_dma(&mut self) {
        // Source pages E0-FF read the WRAM echo (mirror C0-DF).
        let src = self.soc.dma.source();
        let page = if src >= 0xE0 { src - 0x20 } else { src };
        let source = (page as u16) << 8;
        for i in 0..0xA0u16 {
            // The DMA unit's own source fetches are never blocked.
            let byte = read_raw(self.soc, self.cart, source + i);
            self.soc.ppu.dma_write_oam(i as usize, byte);
        }
        self.soc.dma.begin(); // open the 640-T busy window
    }
}

impl Bus for BusView<'_> {
    /// Advance the memory-mapped peripherals by `cycles` T-cycles, folding any
    /// interrupts they raise into the IF register.
    fn tick(&mut self, cycles: u8) {
        if self.soc.timer.tick(cycles) {
            self.soc.intctrl.request(InterruptType::Timer);
        }
        if self.soc.serial.tick(cycles) {
            self.soc.intctrl.request(InterruptType::Serial);
        }
        self.soc.apu.tick(cycles);
        let ppu_interrupts = self.soc.ppu.tick(cycles);
        self.soc.intctrl.request_mask(ppu_interrupts);

        // Advance the OAM DMA: the controller runs down any active window and
        // the startup delay of a just-requested transfer, signalling when the
        // delay elapses so the copy can begin this cycle.
        if self.soc.dma.tick(cycles) {
            self.start_oam_dma();
        }
    }

    fn read_byte(&self, addr: u16) -> u8 {
        bus_read(self.soc, self.cart, addr)
    }

    fn write_byte(&mut self, addr: u16, value: u8) {
        // A write conflicting with the transfer is dropped (the DMA owns that
        // bus) — e.g. a PUSH with the stack in OAM does not land mid-transfer.
        if self.soc.dma.conflicts(addr) {
            return;
        }
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, value), // MBC control
            0x8000..=0x9FFF => self.soc.ppu.write_vram(addr, value),
            0xA000..=0xBFFF => self.cart.write_ram(addr, value),
            0xC000..=0xDFFF => self.soc.wram.write_byte(addr, value),
            0xE000..=0xFDFF => self.soc.wram.write_byte(addr, value), // Echo RAM
            0xFE00..=0xFE9F => self.soc.ppu.write_oam(addr, value),
            0xFEA0..=0xFEFF => {} // Not usable
            0xFF00 => {
                // Re-selecting a group that holds a pressed button is a
                // high→low edge on the input lines, so a write can interrupt.
                if self.soc.p1.write_register(value) {
                    self.soc.intctrl.request(InterruptType::Joypad);
                }
            }
            0xFF01 | 0xFF02 => self.soc.serial.write_register(addr, value),
            0xFF04..=0xFF07 => self.soc.timer.write_register(addr, value),
            0xFF0F => self.soc.intctrl.write(addr, value),
            0xFF10..=0xFF3F => self.soc.apu.write_register(addr, value),
            0xFF46 => {
                // Request an OAM DMA. It does not start now: an idle M-cycle
                // passes before the busy window opens. A request while a
                // previous transfer runs lets that one keep blocking until the
                // new one takes over.
                self.soc.dma.request(value);
            }
            0xFF40..=0xFF4B => {
                self.soc.ppu.write_register(addr, value);
                if self.soc.ppu.take_stat_irq() {
                    self.soc.intctrl.request(InterruptType::LCDStat);
                }
            }
            0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => {}
            0xFF80..=0xFFFE => self.soc.hram.write_byte(addr, value),
            0xFFFF => self.soc.intctrl.write(addr, value),
        }
    }
}

// -- Save state -------------------------------------------------------------
// Only the system's own parts; the cartridge is serialized separately by
// `Console` (it is a distinct, externally-owned unit).

#[cfg(feature = "serialize")]
impl Soc {
    pub fn write_state(&self, out: &mut Vec<u8>) {
        self.wram.write_state(out);
        self.hram.write_state(out);
        self.p1.write_state(out);
        self.ppu.write_state(out);
        self.timer.write_state(out);
        self.apu.write_state(out);
        self.serial.write_state(out);
        self.dma.write_state(out);
        self.intctrl.write_state(out);
    }

    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.wram.read_state(r)?;
        self.hram.read_state(r)?;
        self.p1.read_state(r)?;
        self.ppu.read_state(r)?;
        self.timer.read_state(r)?;
        self.apu.read_state(r)?;
        self.serial.read_state(r)?;
        self.dma.read_state(r)?;
        self.intctrl.read_state(r)?;
        Ok(())
    }
}
