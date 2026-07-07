use crate::console::apu::Apu;
use crate::console::cartridge::Cartridge;
use crate::console::hram::Hram;
use crate::console::interrupts::InterruptType;
use crate::console::joypad::P1;
use crate::console::ppu::Ppu;
use crate::console::timer::Timer;
use crate::console::wram::Wram;

/// Minimal interface the CPU (the bus master) uses to reach memory.
pub trait Bus {
    fn read_byte(&self, addr: u16) -> u8;
    fn write_byte(&mut self, addr: u16, value: u8);
}

/// The system bus: it owns every memory-mapped component and routes the CPU's
/// accesses to them by address, mirroring the DMG memory map. The CPU (and the
/// OAM DMA) are the only bus masters; peripherals are slaves reached through
/// here.
pub struct MemoryBus {
    cartridge: Cartridge,
    wram: Wram,
    hram: Hram,
    p1: P1,
    ppu: Ppu,
    timer: Timer,
    apu: Apu,
    serial: [u8; 2], // 0xFF01 data, 0xFF02 control (stubbed)
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
            serial: [0x00, 0x00],
            if_register: 0x00,
            ie_register: 0x00,
        }
    }

    /// Advance the memory-mapped peripherals by `cycles` T-cycles, folding any
    /// interrupts they raise into the IF register.
    pub fn tick(&mut self, cycles: u8) {
        if self.timer.tick(cycles) {
            self.if_register |= InterruptType::Timer.to_bit();
        }
        self.apu.tick(cycles);
        let ppu_interrupts = self.ppu.tick(cycles);
        self.if_register |= ppu_interrupts & 0x1F;
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

    /// Copy 160 bytes from `value << 8` into OAM (OAM DMA transfer).
    fn oam_dma(&mut self, value: u8) {
        let source = (value as u16) << 8;
        for i in 0..0xA0u16 {
            let byte = self.read_byte(source + i);
            self.ppu.dma_write_oam(i as usize, byte);
        }
    }
}

impl Default for MemoryBus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus for MemoryBus {
    fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cartridge.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.read_vram(addr),
            0xA000..=0xBFFF => self.cartridge.read_ram(addr),
            0xC000..=0xDFFF => self.wram.read_byte(addr),
            0xE000..=0xFDFF => self.wram.read_byte(addr), // Echo RAM
            0xFE00..=0xFE9F => self.ppu.read_oam(addr),
            0xFEA0..=0xFEFF => 0xFF, // Not usable
            0xFF00 => self.p1.read_register(),
            0xFF01 => self.serial[0],
            0xFF02 => self.serial[1],
            0xFF04..=0xFF07 => self.timer.read_register(addr),
            0xFF0F => self.if_register | 0xE0, // top 3 bits read as 1
            0xFF10..=0xFF3F => self.apu.read_register(addr),
            0xFF40..=0xFF4B => self.ppu.read_register(addr),
            0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => 0xFF,
            0xFF80..=0xFFFE => self.hram.read_byte(addr),
            0xFFFF => self.ie_register,
        }
    }

    fn write_byte(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x7FFF => self.cartridge.write_rom(addr, value), // MBC control
            0x8000..=0x9FFF => self.ppu.write_vram(addr, value),
            0xA000..=0xBFFF => self.cartridge.write_ram(addr, value),
            0xC000..=0xDFFF => self.wram.write_byte(addr, value),
            0xE000..=0xFDFF => self.wram.write_byte(addr, value), // Echo RAM
            0xFE00..=0xFE9F => self.ppu.write_oam(addr, value),
            0xFEA0..=0xFEFF => {} // Not usable
            0xFF00 => self.p1.write_register(value),
            0xFF01 => self.serial[0] = value,
            0xFF02 => self.serial[1] = value,
            0xFF04..=0xFF07 => self.timer.write_register(addr, value),
            0xFF0F => self.if_register = value & 0x1F,
            0xFF10..=0xFF3F => self.apu.write_register(addr, value),
            0xFF46 => {
                self.ppu.write_register(addr, value);
                self.oam_dma(value);
            }
            0xFF40..=0xFF4B => self.ppu.write_register(addr, value),
            0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => {}
            0xFF80..=0xFFFE => self.hram.write_byte(addr, value),
            0xFFFF => self.ie_register = value,
        }
    }
}
