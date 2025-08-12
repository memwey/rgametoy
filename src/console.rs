use crate::cpu::Cpu;
use crate::bus::{Bus, MemoryBus};
use crate::interrupts::InterruptType;

// Game Boy operates at 4.194304 MHz, which is 4194304 cycles per second.
// A frame is 1/60th of a second.
// Cycles per frame = 4194304 / 60 = 69905.066...
// For simplicity, we'll use 70224 cycles per frame (common in emulators)
const CYCLES_PER_FRAME: u64 = 70224;

pub struct Console {
    cpu: Cpu,
    bus: MemoryBus,
    total_cycles: u64,
}

impl Console {
    pub fn new() -> Console {
        Console {
            cpu: Cpu::new(),
            bus: MemoryBus::new(),
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
        // In the future, distribute cycles to other components (PPU, etc.)
        cycles
    }

    pub fn run_frame(&mut self) {
        let mut cycles_this_frame = 0;
        while cycles_this_frame < CYCLES_PER_FRAME {
            let cycles_executed = self.step();
            cycles_this_frame += cycles_executed as u64;
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
