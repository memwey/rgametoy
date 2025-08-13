use crate::cpu::Cpu;
use crate::bus::{Bus, MemoryBus};
use crate::interrupts::InterruptType;
use crate::lcd::Lcd;
use crate::display::Display;
use crate::p1::P1; // New import
use crate::ppu::Ppu; // New import
use crate::timer::Timer; // New import
use std::rc::Rc; // New import
use std::cell::RefCell; // New import

// Game Boy operates at 4.194304 MHz, which is 4194304 cycles per second.
// A frame is 1/60th of a second.
// Cycles per frame = 4194304 / 60 = 69905.066...
// For simplicity, we'll use 70224 cycles per frame (common in emulators)
const CYCLES_PER_FRAME: u64 = 70224;

pub struct Console {
    cpu: Cpu,
    bus: MemoryBus,
    lcd: Lcd,
    total_cycles: u64,
    p1: Rc<RefCell<P1>>,
    ppu: Rc<RefCell<Ppu>>,
    timer: Rc<RefCell<Timer>>,
}

impl Console {
    pub fn new() -> Console {
        let p1 = Rc::new(RefCell::new(P1::new()));
        let ppu = Rc::new(RefCell::new(Ppu::new()));
        let timer = Rc::new(RefCell::new(Timer::new()));

        Console {
            cpu: Cpu::new(),
            bus: MemoryBus::new(Rc::clone(&p1), Rc::clone(&ppu), Rc::clone(&timer)),
            lcd: Lcd::new(),
            total_cycles: 0,
            p1,
            ppu,
            timer,
        }
    }

    pub fn load_program(&mut self, program: &[u8]) {
        self.cpu.load_program(&mut self.bus as &mut dyn Bus, program);
    }

    pub fn step(&mut self) -> u8 {
        let cycles = self.cpu.step(&mut self.bus as &mut dyn Bus);
        self.total_cycles += cycles as u64;
        if self.timer.borrow_mut().tick(cycles) {
            self.bus.request_interrupt(InterruptType::Timer);
        }
        
        cycles
    }

    pub fn run_frame(&mut self, display: &mut Display) {
        let mut cycles_this_frame = 0;
        while cycles_this_frame < CYCLES_PER_FRAME {
            let cycles_executed = self.step();
            cycles_this_frame += cycles_executed as u64;

            // Tick the PPU and get pixel data
            let pixel_data = self.ppu.borrow_mut().tick(cycles_executed);
            let current_ly = self.ppu.borrow().ly;

            // Process each pixel from PPU
            for (_x, pixel) in pixel_data {
                // Receive pixel and check if scanline is complete
                if self.lcd.receive_pixel(pixel, current_ly) {
                    // Scanline is complete, send it to display
                    if current_ly < 144 { // Only send visible scanlines
                        display.receive_scanline(current_ly, self.lcd.get_line_data());
                    }
                }
            }

            // Check if a full frame is ready (PPU enters VBlank)
            if self.ppu.borrow().ly == 144 && self.ppu.borrow().get_mode() == crate::ppu::PpuMode::VBlank {
                display.present_frame();
                break; // Exit loop once a frame is ready
            }
        }
    }

    pub fn get_p1(&self) -> Rc<RefCell<P1>> {
        Rc::clone(&self.p1)
    }

    pub fn get_cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn get_bus_mut(&mut self) -> &mut MemoryBus {
        &mut self.bus
    }
}