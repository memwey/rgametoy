use crate::cpu::Cpu;
use crate::bus::{Bus, MemoryBus};

pub struct Console {
    cpu: Cpu,
    bus: MemoryBus,
}

impl Console {
    pub fn new() -> Console {
        Console {
            cpu: Cpu::new(),
            bus: MemoryBus::new(),
        }
    }

    pub fn load_program(&mut self, program: &[u8]) {
        self.cpu.load_program(&mut self.bus as &mut dyn Bus, program);
    }

    pub fn step(&mut self) -> u8 {
        self.cpu.step(&mut self.bus as &mut dyn Bus)
    }

    pub fn get_cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn get_cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
    }
}
