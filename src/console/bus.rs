use crate::console::apu::Apu;
use crate::console::cartridge::Cartridge;
use crate::console::hram::Hram;
use crate::console::interrupts::InterruptType;
use crate::console::joypad::P1;
use crate::console::ppu::Ppu;
use crate::console::serial::Serial;
use crate::console::timer::Timer;
use crate::console::wram::Wram;

/// Minimal interface the CPU (the bus master) uses to reach memory and to
/// advance the rest of the machine one M-cycle at a time.
pub trait Bus {
    fn read_byte(&self, addr: u16) -> u8;
    fn write_byte(&mut self, addr: u16, value: u8);
    /// Advance the memory-mapped peripherals by `cycles` T-cycles.
    fn tick(&mut self, cycles: u8);
}

/// The system bus: it owns every memory-mapped component and routes the CPU's
/// accesses to them by address, mirroring the DMG memory map. The CPU (and the
/// OAM DMA) are the only bus masters; peripherals are slaves reached through
/// here.
#[derive(Clone)]
pub struct MemoryBus {
    cartridge: Cartridge,
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
    /// (OAM stays accessible) before the busy window opens (see `start_oam_dma`).
    dma_delay: u8,
    /// High byte of the pending/active DMA source address (`FF46` value).
    dma_source: u8,
    if_register: u8, // Interrupt Flag register (0xFF0F)
    ie_register: u8, // Interrupt Enable register (0xFFFF)
}

impl MemoryBus {
    pub fn new() -> MemoryBus {
        MemoryBus {
            cartridge: Cartridge::new(),
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
        self.p1.update_button_state(state);
    }

    pub fn load_cartridge(&mut self, cartridge: Cartridge) {
        self.cartridge = cartridge;
    }

    /// Inject a small program into ROM bank 0 (used by tests).
    pub fn load_rom(&mut self, program: &[u8]) {
        self.cartridge.load(program);
    }

    pub fn cartridge(&self) -> &Cartridge {
        &self.cartridge
    }

    pub fn cartridge_mut(&mut self) -> &mut Cartridge {
        &mut self.cartridge
    }

    pub fn request_interrupt(&mut self, interrupt_type: InterruptType) {
        self.if_register |= interrupt_type.to_bit();
    }

    /// OAM DMA state `(active, source page)` for the `debug` inspector.
    #[cfg(feature = "debug")]
    pub fn debug_dma(&self) -> (bool, u8) {
        (self.dma_remaining > 0, self.dma_source)
    }

    /// Internal PPU state for the inspector (the bus owns the PPU privately).
    #[cfg(feature = "debug")]
    pub fn debug_ppu(&self) -> (u8, u16, bool, bool) {
        self.ppu.debug_state()
    }

    /// Internal timer state for the inspector.
    #[cfg(feature = "debug")]
    pub fn debug_timer(&self) -> (u8, u8, u8, u8) {
        self.timer.debug_state()
    }

    /// Activate a pending OAM DMA once its startup delay elapses: copy 160 bytes
    /// from `dma_source << 8` into OAM and open the 640-T-cycle busy window
    /// (during which only HRAM is accessible). The copy is done atomically here;
    /// since OAM is blocked for the whole window the CPU cannot observe the
    /// difference from a byte-by-byte transfer, and OAM holds the new data by
    /// the time the window closes.
    fn start_oam_dma(&mut self) {
        // Source pages E0-FF read the WRAM echo (the transfer does not see the
        // OAM/IO map), so they mirror C0-DF.
        let page = if self.dma_source >= 0xE0 {
            self.dma_source - 0x20
        } else {
            self.dma_source
        };
        let source = (page as u16) << 8;
        for i in 0..0xA0u16 {
            // Read past the DMA block: the transfer's own source fetches are
            // never blocked (this is the DMA unit acting as bus master).
            let byte = self.read_raw(source + i);
            self.ppu.dma_write_oam(i as usize, byte);
        }
        self.dma_remaining = 160 * 4; // 160 M-cycles
    }

    /// Whether a CPU access to `addr` conflicts with an in-progress OAM DMA.
    ///
    /// The transfer drives one of the two buses depending on its source: a VRAM
    /// source ($80-$9F) drives the *video* bus (VRAM + OAM), any other source
    /// drives the *external* bus (ROM / cart RAM / WRAM + echo). The CPU may
    /// freely use the other bus, plus I/O and HRAM; only OAM (the destination)
    /// is locked regardless. So e.g. a DMA from VRAM still lets the CPU fetch
    /// opcodes from ROM while its OAM reads return open bus.
    fn dma_conflicts(&self, addr: u16) -> bool {
        if self.dma_remaining == 0 {
            return false;
        }
        let video_dma = (0x80..=0x9F).contains(&self.dma_source);
        match addr {
            0xFE00..=0xFE9F => true,                         // OAM (destination)
            0x8000..=0x9FFF => video_dma,                    // VRAM (video bus)
            0x0000..=0x7FFF | 0xA000..=0xFDFF => !video_dma, // external bus
            _ => false,                                       // FEA0-FEFF, I/O, HRAM
        }
    }

    /// Address-decoded read with no OAM-DMA blocking applied. `read_byte` layers
    /// the block on top; the DMA source copy uses this directly.
    fn read_raw(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cartridge.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.read_vram(addr),
            0xA000..=0xBFFF => self.cartridge.read_ram(addr),
            0xC000..=0xDFFF => self.wram.read_byte(addr),
            0xE000..=0xFDFF => self.wram.read_byte(addr), // Echo RAM
            0xFE00..=0xFE9F => self.ppu.read_oam(addr),
            0xFEA0..=0xFEFF => 0xFF, // Not usable
            0xFF00 => self.p1.read_register(),
            0xFF01 | 0xFF02 => self.serial.read_register(addr),
            0xFF04..=0xFF07 => self.timer.read_register(addr),
            0xFF0F => self.if_register | 0xE0, // top 3 bits read as 1
            0xFF10..=0xFF3F => self.apu.read_register(addr),
            0xFF40..=0xFF4B => self.ppu.read_register(addr),
            0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => 0xFF,
            0xFF80..=0xFFFE => self.hram.read_byte(addr),
            0xFFFF => self.ie_register,
        }
    }
}

impl Default for MemoryBus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus for MemoryBus {
    /// Advance the memory-mapped peripherals by `cycles` T-cycles, folding any
    /// interrupts they raise into the IF register.
    fn tick(&mut self, cycles: u8) {
        if self.timer.tick(cycles) {
            self.if_register |= InterruptType::Timer.to_bit();
        }
        if self.serial.tick(cycles) {
            self.if_register |= InterruptType::Serial.to_bit();
        }
        self.apu.tick(cycles);
        let ppu_interrupts = self.ppu.tick(cycles);
        self.if_register |= ppu_interrupts & 0x1F;

        // Advance the OAM DMA: run down any active window, then the startup
        // delay of a just-requested transfer (which may activate this cycle).
        if self.dma_remaining > 0 {
            self.dma_remaining = self.dma_remaining.saturating_sub(cycles as u16);
        }
        if self.dma_delay > 0 {
            self.dma_delay = self.dma_delay.saturating_sub(cycles);
            if self.dma_delay == 0 {
                self.start_oam_dma();
            }
        }
    }

    fn read_byte(&self, addr: u16) -> u8 {
        // A read conflicting with the transfer sees open bus.
        if self.dma_conflicts(addr) {
            return 0xFF;
        }
        self.read_raw(addr)
    }

    fn write_byte(&mut self, addr: u16, value: u8) {
        // A write conflicting with the transfer is dropped (the DMA owns that
        // bus) — e.g. a PUSH with the stack in OAM does not land mid-transfer.
        if self.dma_conflicts(addr) {
            return;
        }
        match addr {
            0x0000..=0x7FFF => self.cartridge.write_rom(addr, value), // MBC control
            0x8000..=0x9FFF => self.ppu.write_vram(addr, value),
            0xA000..=0xBFFF => self.cartridge.write_ram(addr, value),
            0xC000..=0xDFFF => self.wram.write_byte(addr, value),
            0xE000..=0xFDFF => self.wram.write_byte(addr, value), // Echo RAM
            0xFE00..=0xFE9F => self.ppu.write_oam(addr, value),
            0xFEA0..=0xFEFF => {} // Not usable
            0xFF00 => self.p1.write_register(value),
            0xFF01 | 0xFF02 => self.serial.write_register(addr, value),
            0xFF04..=0xFF07 => self.timer.write_register(addr, value),
            0xFF0F => self.if_register = value & 0x1F,
            0xFF10..=0xFF3F => self.apu.write_register(addr, value),
            0xFF46 => {
                self.ppu.write_register(addr, value);
                // Request an OAM DMA. It does not start now: an idle M-cycle
                // passes (OAM stays accessible) before the busy window opens.
                // A request while a previous transfer is still running lets that
                // one keep blocking until the new one takes over.
                self.dma_source = value;
                self.dma_delay = 8;
            }
            0xFF40..=0xFF4B => {
                self.ppu.write_register(addr, value);
                if self.ppu.take_stat_irq() {
                    self.if_register |= InterruptType::LCDStat.to_bit();
                }
            }
            0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => {}
            0xFF80..=0xFFFE => self.hram.write_byte(addr, value),
            0xFFFF => self.ie_register = value,
        }
    }
}
