use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::hram::Hram;
use crate::interrupts::InterruptType;
use crate::joypad::P1;
use crate::ppu::Ppu;
use crate::serial::Serial;
#[cfg(feature = "serialize")]
use crate::state::{write_u16_le, write_u8, Reader, SaveStateError};
use crate::timer::Timer;
use crate::wram::Wram;

/// Minimal interface the CPU (the bus master) uses to reach memory and to
/// advance the rest of the machine one M-cycle at a time.
pub trait Bus {
    fn read_byte(&self, addr: u16) -> u8;
    fn write_byte(&mut self, addr: u16, value: u8);
    /// Advance the memory-mapped peripherals by `cycles` T-cycles.
    fn tick(&mut self, cycles: u8);
}

/// The console's memory-mapped system: every component the CPU can reach
/// *except* the cartridge. The cartridge is a separate unit (inserted at
/// power-on, owned by `Console`) that the bus borrows per step — see
/// [`BusView`]. This split mirrors the hardware: the handheld and the game pak
/// are distinct.
#[derive(Clone)]
pub struct System {
    wram: Wram,
    hram: Hram,
    p1: P1,
    ppu: Ppu,
    timer: Timer,
    apu: Apu,
    serial: Serial,
    /// T-cycles remaining in an active OAM DMA transfer (0 = idle). While it
    /// runs the CPU can only reach HRAM.
    dma_remaining: u16,
    /// T-cycles until a just-requested OAM DMA actually starts. Writing FF46
    /// does not begin the transfer immediately: there is a one-M-cycle idle gap
    /// (OAM stays accessible) before the busy window opens.
    dma_delay: u8,
    /// High byte of the pending/active DMA source address (`FF46` value).
    dma_source: u8,
    if_register: u8, // Interrupt Flag register (0xFF0F)
    ie_register: u8, // Interrupt Enable register (0xFFFF)
}

impl System {
    pub fn new() -> System {
        System {
            wram: Wram::new(),
            hram: Hram::new(),
            p1: P1::new(),
            ppu: Ppu::new(),
            timer: Timer::new(),
            apu: Apu::new(),
            serial: Serial::new(),
            dma_remaining: 0,
            dma_delay: 0,
            dma_source: 0,
            if_register: 0x00,
            ie_register: 0x00,
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
            self.if_register |= InterruptType::Joypad.to_bit();
        }
    }

    pub fn request_interrupt(&mut self, interrupt_type: InterruptType) {
        self.if_register |= interrupt_type.to_bit();
    }

    /// OAM DMA state `(active, source page)` for the `debug` inspector.
    #[cfg(feature = "debug")]
    pub fn debug_dma(&self) -> (bool, u8) {
        (self.dma_remaining > 0, self.dma_source)
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

impl Default for System {
    fn default() -> Self {
        Self::new()
    }
}

// -- Memory read path -------------------------------------------------------
// Free functions (same module, so they can touch `System`'s private fields)
// shared by the mutable `BusView` and by read-only callers such as the `debug`
// inspector, which need to read memory through a `&self` Console.

/// Whether a CPU access to `addr` conflicts with an in-progress OAM DMA.
///
/// The transfer drives one of the two buses depending on its source: a VRAM
/// source ($80-$9F) drives the *video* bus (VRAM + OAM), any other source drives
/// the *external* bus (ROM / cart RAM / WRAM + echo). The CPU may freely use the
/// other bus, plus I/O and HRAM; only OAM (the destination) is locked
/// regardless.
fn dma_conflicts(sys: &System, addr: u16) -> bool {
    if sys.dma_remaining == 0 {
        return false;
    }
    let video_dma = (0x80..=0x9F).contains(&sys.dma_source);
    match addr {
        0xFE00..=0xFE9F => true,                          // OAM (destination)
        0x8000..=0x9FFF => video_dma,                     // VRAM (video bus)
        0x0000..=0x7FFF | 0xA000..=0xFDFF => !video_dma,  // external bus
        _ => false,                                       // FEA0-FEFF, I/O, HRAM
    }
}

/// Address-decoded read with no OAM-DMA blocking applied. [`bus_read`] layers the
/// block on top; the DMA source copy uses this directly.
fn read_raw(sys: &System, cart: &Cartridge, addr: u16) -> u8 {
    match addr {
        0x0000..=0x7FFF => cart.read_rom(addr),
        0x8000..=0x9FFF => sys.ppu.read_vram(addr),
        0xA000..=0xBFFF => cart.read_ram(addr),
        0xC000..=0xDFFF => sys.wram.read_byte(addr),
        0xE000..=0xFDFF => sys.wram.read_byte(addr), // Echo RAM
        0xFE00..=0xFE9F => sys.ppu.read_oam(addr),
        0xFEA0..=0xFEFF => 0xFF, // Not usable
        0xFF00 => sys.p1.read_register(),
        0xFF01 | 0xFF02 => sys.serial.read_register(addr),
        0xFF04..=0xFF07 => sys.timer.read_register(addr),
        0xFF0F => sys.if_register | 0xE0, // top 3 bits read as 1
        0xFF10..=0xFF3F => sys.apu.read_register(addr),
        0xFF40..=0xFF4B => sys.ppu.read_register(addr),
        0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => 0xFF,
        0xFF80..=0xFFFE => sys.hram.read_byte(addr),
        0xFFFF => sys.ie_register,
    }
}

/// A CPU read: open bus (0xFF) if it conflicts with an active OAM DMA, else the
/// address-decoded byte. Immutable, so the inspector can peek through `&self`.
pub(crate) fn bus_read(sys: &System, cart: &Cartridge, addr: u16) -> u8 {
    if dma_conflicts(sys, addr) {
        return 0xFF;
    }
    read_raw(sys, cart, addr)
}

/// A transient pairing of the console's [`System`] with the borrowed
/// [`Cartridge`], assembled per step so the CPU can reach the whole memory map.
/// This is the actual `Bus` the CPU drives; it owns neither side.
pub struct BusView<'a> {
    sys: &'a mut System,
    cart: &'a mut Cartridge,
}

impl<'a> BusView<'a> {
    pub fn new(sys: &'a mut System, cart: &'a mut Cartridge) -> BusView<'a> {
        BusView { sys, cart }
    }

    /// Activate a pending OAM DMA once its startup delay elapses: copy 160 bytes
    /// from `dma_source << 8` into OAM and open the 640-T-cycle busy window
    /// (during which only HRAM is accessible). The copy is atomic here; since
    /// OAM is blocked for the whole window the CPU cannot tell it from a
    /// byte-by-byte transfer.
    fn start_oam_dma(&mut self) {
        // Source pages E0-FF read the WRAM echo (mirror C0-DF).
        let page = if self.sys.dma_source >= 0xE0 {
            self.sys.dma_source - 0x20
        } else {
            self.sys.dma_source
        };
        let source = (page as u16) << 8;
        for i in 0..0xA0u16 {
            // The DMA unit's own source fetches are never blocked.
            let byte = read_raw(self.sys, self.cart, source + i);
            self.sys.ppu.dma_write_oam(i as usize, byte);
        }
        self.sys.dma_remaining = 160 * 4; // 160 M-cycles
    }
}

impl Bus for BusView<'_> {
    /// Advance the memory-mapped peripherals by `cycles` T-cycles, folding any
    /// interrupts they raise into the IF register.
    fn tick(&mut self, cycles: u8) {
        if self.sys.timer.tick(cycles) {
            self.sys.if_register |= InterruptType::Timer.to_bit();
        }
        if self.sys.serial.tick(cycles) {
            self.sys.if_register |= InterruptType::Serial.to_bit();
        }
        self.sys.apu.tick(cycles);
        let ppu_interrupts = self.sys.ppu.tick(cycles);
        self.sys.if_register |= ppu_interrupts & 0x1F;

        // Advance the OAM DMA: run down any active window, then the startup
        // delay of a just-requested transfer (which may activate this cycle).
        if self.sys.dma_remaining > 0 {
            self.sys.dma_remaining = self.sys.dma_remaining.saturating_sub(cycles as u16);
        }
        if self.sys.dma_delay > 0 {
            self.sys.dma_delay = self.sys.dma_delay.saturating_sub(cycles);
            if self.sys.dma_delay == 0 {
                self.start_oam_dma();
            }
        }
    }

    fn read_byte(&self, addr: u16) -> u8 {
        bus_read(self.sys, self.cart, addr)
    }

    fn write_byte(&mut self, addr: u16, value: u8) {
        // A write conflicting with the transfer is dropped (the DMA owns that
        // bus) — e.g. a PUSH with the stack in OAM does not land mid-transfer.
        if dma_conflicts(self.sys, addr) {
            return;
        }
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, value), // MBC control
            0x8000..=0x9FFF => self.sys.ppu.write_vram(addr, value),
            0xA000..=0xBFFF => self.cart.write_ram(addr, value),
            0xC000..=0xDFFF => self.sys.wram.write_byte(addr, value),
            0xE000..=0xFDFF => self.sys.wram.write_byte(addr, value), // Echo RAM
            0xFE00..=0xFE9F => self.sys.ppu.write_oam(addr, value),
            0xFEA0..=0xFEFF => {} // Not usable
            0xFF00 => {
                // Re-selecting a group that holds a pressed button is a
                // high→low edge on the input lines, so a write can interrupt.
                if self.sys.p1.write_register(value) {
                    self.sys.if_register |= InterruptType::Joypad.to_bit();
                }
            }
            0xFF01 | 0xFF02 => self.sys.serial.write_register(addr, value),
            0xFF04..=0xFF07 => self.sys.timer.write_register(addr, value),
            0xFF0F => self.sys.if_register = value & 0x1F,
            0xFF10..=0xFF3F => self.sys.apu.write_register(addr, value),
            0xFF46 => {
                self.sys.ppu.write_register(addr, value);
                // Request an OAM DMA. It does not start now: an idle M-cycle
                // passes before the busy window opens. A request while a
                // previous transfer runs lets that one keep blocking until the
                // new one takes over.
                self.sys.dma_source = value;
                self.sys.dma_delay = 8;
            }
            0xFF40..=0xFF4B => {
                self.sys.ppu.write_register(addr, value);
                if self.sys.ppu.take_stat_irq() {
                    self.sys.if_register |= InterruptType::LCDStat.to_bit();
                }
            }
            0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => {}
            0xFF80..=0xFFFE => self.sys.hram.write_byte(addr, value),
            0xFFFF => self.sys.ie_register = value,
        }
    }
}

// -- Save state -------------------------------------------------------------
// Only the system's own parts; the cartridge is serialized separately by
// `Console` (it is a distinct, externally-owned unit).

#[cfg(feature = "serialize")]
impl System {
    pub fn write_state(&self, out: &mut Vec<u8>) {
        self.wram.write_state(out);
        self.hram.write_state(out);
        self.p1.write_state(out);
        self.ppu.write_state(out);
        self.timer.write_state(out);
        self.apu.write_state(out);
        self.serial.write_state(out);
        write_u16_le(out, self.dma_remaining);
        write_u8(out, self.dma_delay);
        write_u8(out, self.dma_source);
        write_u8(out, self.if_register);
        write_u8(out, self.ie_register);
    }

    pub fn read_state(&mut self, r: &mut Reader<'_>) -> Result<(), SaveStateError> {
        self.wram.read_state(r)?;
        self.hram.read_state(r)?;
        self.p1.read_state(r)?;
        self.ppu.read_state(r)?;
        self.timer.read_state(r)?;
        self.apu.read_state(r)?;
        self.serial.read_state(r)?;
        self.dma_remaining = r.read_u16_le()?;
        self.dma_delay = r.read_u8()?;
        self.dma_source = r.read_u8()?;
        self.if_register = r.read_u8()?;
        self.ie_register = r.read_u8()?;
        Ok(())
    }
}
