use crate::cpu::Cpu;
use crate::bus::{Bus, MemoryBus};
use crate::interrupts::InterruptType;
use crate::lcd::Lcd;

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
        let pixel_data = self.bus.get_ppu_mut().tick(cycles); // Tick the PPU

        if let Some((r, g, b, a)) = pixel_data {
            let ly = self.bus.get_ppu_mut().ly;
            let current_pixel_x = self.bus.get_ppu_mut().current_pixel_x - 1; // -1 because it's incremented after pixel generation
            self.lcd.receive_pixel(current_pixel_x, ly, r, g, b, a);
        }

        cycles
    }

    pub fn run_frame(&mut self) -> Option<&[u8]> {
        let mut cycles_this_frame = 0;
        while cycles_this_frame < CYCLES_PER_FRAME {
            let cycles_executed = self.step();
            cycles_this_frame += cycles_executed as u64;

            // Check if a full frame is ready (PPU enters VBlank)
            if self.bus.get_ppu_mut().ly == 144 {
                return Some(self.lcd.get_frame_data());
            }
        }
        None
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