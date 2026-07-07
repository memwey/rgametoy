use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::hram::Hram;
use crate::interrupts::InterruptType;
use crate::p1::P1;
use crate::ppu::Ppu;
use crate::timer::Timer;
use crate::wram::Wram;
use std::cell::RefCell;
use std::rc::Rc;

pub trait Bus {
    fn read_byte(&self, addr: u16) -> u8;
    fn write_byte(&mut self, addr: u16, value: u8);
    fn read_u16(&self, addr: u16) -> u16;
}

pub struct MemoryBus {
    cartridge: Cartridge,
    wram: Wram,
    hram: Hram,
    p1: Rc<RefCell<P1>>,
    ppu: Rc<RefCell<Ppu>>,
    timer: Rc<RefCell<Timer>>,
    apu: Rc<RefCell<Apu>>,
    serial: [u8; 2], // 0xFF01 data, 0xFF02 control (stubbed)
    if_register: u8, // Interrupt Flag register (0xFF0F)
    ie_register: u8, // Interrupt Enable register (0xFFFF)
}

impl MemoryBus {
    pub fn new(
        p1: Rc<RefCell<P1>>,
        ppu: Rc<RefCell<Ppu>>,
        timer: Rc<RefCell<Timer>>,
        apu: Rc<RefCell<Apu>>,
    ) -> MemoryBus {
        MemoryBus {
            cartridge: Cartridge::new(),
            wram: Wram::new(),
            hram: Hram::new(),
            p1,
            ppu,
            timer,
            apu,
            serial: [0x00, 0x00],
            if_register: 0x00,
            ie_register: 0x00,
        }
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

    pub fn request_interrupt(&mut self, interrupt_type: InterruptType) {
        self.if_register |= interrupt_type.to_bit();
    }

    /// OR raw IF-register bits (e.g. those returned by the PPU).
    pub fn request_interrupt_bits(&mut self, bits: u8) {
        self.if_register |= bits & 0x1F;
    }

    /// Copy 160 bytes from `value << 8` into OAM (OAM DMA transfer).
    fn oam_dma(&mut self, value: u8) {
        let source = (value as u16) << 8;
        for i in 0..0xA0u16 {
            let byte = self.read_byte(source + i);
            self.ppu.borrow_mut().dma_write_oam(i as usize, byte);
        }
    }
}

impl Bus for MemoryBus {
    fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cartridge.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.borrow().read_vram(addr),
            0xA000..=0xBFFF => self.cartridge.read_ram(addr),
            0xC000..=0xDFFF => self.wram.read_byte(addr),
            0xE000..=0xFDFF => self.wram.read_byte(addr), // Echo RAM
            0xFE00..=0xFE9F => self.ppu.borrow().read_oam(addr),
            0xFEA0..=0xFEFF => 0xFF, // Not usable
            0xFF00 => self.p1.borrow().read_register(),
            0xFF01 => self.serial[0],
            0xFF02 => self.serial[1],
            0xFF04..=0xFF07 => self.timer.borrow().read_register(addr),
            0xFF0F => self.if_register | 0xE0, // top 3 bits read as 1
            0xFF10..=0xFF3F => self.apu.borrow().read_register(addr),
            0xFF40..=0xFF4B => self.ppu.borrow().read_register(addr),
            0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => 0xFF,
            0xFF80..=0xFFFE => self.hram.read_byte(addr),
            0xFFFF => self.ie_register,
        }
    }

    fn write_byte(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x7FFF => self.cartridge.write_rom(addr, value), // MBC control
            0x8000..=0x9FFF => self.ppu.borrow_mut().write_vram(addr, value),
            0xA000..=0xBFFF => self.cartridge.write_ram(addr, value),
            0xC000..=0xDFFF => self.wram.write_byte(addr, value),
            0xE000..=0xFDFF => self.wram.write_byte(addr, value), // Echo RAM
            0xFE00..=0xFE9F => self.ppu.borrow_mut().write_oam(addr, value),
            0xFEA0..=0xFEFF => {} // Not usable
            0xFF00 => self.p1.borrow_mut().write_register(value),
            0xFF01 => self.serial[0] = value,
            0xFF02 => self.serial[1] = value,
            0xFF04..=0xFF07 => self.timer.borrow_mut().write_register(addr, value),
            0xFF0F => self.if_register = value & 0x1F,
            0xFF10..=0xFF3F => self.apu.borrow_mut().write_register(addr, value),
            0xFF46 => {
                self.ppu.borrow_mut().write_register(addr, value);
                self.oam_dma(value);
            }
            0xFF40..=0xFF4B => self.ppu.borrow_mut().write_register(addr, value),
            0xFF03 | 0xFF08..=0xFF0E | 0xFF4C..=0xFF7F => {}
            0xFF80..=0xFFFE => self.hram.write_byte(addr, value),
            0xFFFF => self.ie_register = value,
        }
    }

    fn read_u16(&self, addr: u16) -> u16 {
        let lo = self.read_byte(addr) as u16;
        let hi = self.read_byte(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }
}
