use crate::cpu::Cpu;
use crate::bus::{Bus, MemoryBus};
use crate::interrupts::InterruptType;
use crate::lcd::Lcd;
use crate::display::Display;

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
}

impl Console {
    pub fn new() -> Console {
        Console {
            cpu: Cpu::new(),
            bus: MemoryBus::new(),
            lcd: Lcd::new(),
            total_cycles: 0,
        }
    }

    pub fn load_program(&mut self, program: &[u8]) {
        self.cpu.load_program(&mut self.bus as &mut dyn Bus, program);
    }

    pub fn step(&mut self) -> u8 {
        let cycles = self.cpu.step(&mut self.bus as &mut dyn Bus);
        self.total_cycles += cycles as u64;
        if self.bus.get_timer_mut().tick(cycles) {
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
            let pixel_data = self.bus.get_ppu_mut().tick(cycles_executed);
            let current_ly = self.bus.get_ppu_mut().ly;

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
            if self.bus.get_ppu_mut().ly == 144 && self.bus.get_ppu_mut().get_mode() == crate::ppu::PpuMode::VBlank {
                display.present_frame();
                break; // Exit loop once a frame is ready
            }
        }
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