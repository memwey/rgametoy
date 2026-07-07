pub mod apu;
pub mod bus;
pub mod cartridge;
pub mod cpu;
pub mod hram;
pub mod interrupts;
pub mod joypad;
pub mod ppu;
pub mod timer;
pub mod wram;

use crate::console::apu::Apu;
use crate::console::bus::{Bus, MemoryBus};
use crate::console::cartridge::Cartridge;
use crate::console::cpu::Cpu;
use crate::console::interrupts::InterruptType;
use crate::console::joypad::P1;
use crate::console::ppu::Ppu;
use crate::console::timer::Timer;
use std::cell::{Ref, RefCell};
use std::rc::Rc;

// The Game Boy runs at 4.194304 MHz. One frame is 154 scanlines × 456 dots =
// 70224 T-cycles.
const CYCLES_PER_FRAME: u64 = 70224;

pub struct Console {
    cpu: Cpu,
    bus: MemoryBus,
    total_cycles: u64,
    p1: Rc<RefCell<P1>>,
    ppu: Rc<RefCell<Ppu>>,
    timer: Rc<RefCell<Timer>>,
    apu: Rc<RefCell<Apu>>,
}

impl Console {
    pub fn new() -> Console {
        let p1 = Rc::new(RefCell::new(P1::new()));
        let ppu = Rc::new(RefCell::new(Ppu::new()));
        let timer = Rc::new(RefCell::new(Timer::new()));
        let apu = Rc::new(RefCell::new(Apu::new()));

        Console {
            cpu: Cpu::new(),
            bus: MemoryBus::new(
                Rc::clone(&p1),
                Rc::clone(&ppu),
                Rc::clone(&timer),
                Rc::clone(&apu),
            ),
            total_cycles: 0,
            p1,
            ppu,
            timer,
            apu,
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

    pub fn step(&mut self) -> u8 {
        let cycles = self.cpu.step(&mut self.bus as &mut dyn Bus);
        self.total_cycles += cycles as u64;
        if self.timer.borrow_mut().tick(cycles) {
            self.bus.request_interrupt(InterruptType::Timer);
        }
        self.apu.borrow_mut().tick(cycles);
        cycles
    }

    /// Advance the machine until the PPU completes one frame (or a full frame's
    /// worth of cycles elapses). The core does no presentation; the frontend
    /// reads [`Console::framebuffer`] afterwards.
    pub fn run_frame(&mut self) {
        let mut cycles_this_frame = 0u64;
        while cycles_this_frame < CYCLES_PER_FRAME {
            let cycles = self.step();
            cycles_this_frame += cycles as u64;

            let ppu_interrupts = self.ppu.borrow_mut().tick(cycles);
            if ppu_interrupts != 0 {
                self.bus.request_interrupt_bits(ppu_interrupts);
            }
            if self.ppu.borrow_mut().take_frame_ready() {
                break;
            }
        }
    }

    /// The current frame as 160×144 shade values (0-3). Borrowed from the PPU.
    pub fn framebuffer(&self) -> Ref<'_, [u8]> {
        Ref::map(self.ppu.borrow(), |ppu| ppu.framebuffer())
    }

    pub fn get_p1(&self) -> Rc<RefCell<P1>> {
        Rc::clone(&self.p1)
    }

    pub fn get_ppu(&self) -> Rc<RefCell<Ppu>> {
        Rc::clone(&self.ppu)
    }

    pub fn get_apu(&self) -> Rc<RefCell<Apu>> {
        Rc::clone(&self.apu)
    }

    pub fn request_joypad_interrupt(&mut self) {
        self.bus.request_interrupt(InterruptType::Joypad);
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
