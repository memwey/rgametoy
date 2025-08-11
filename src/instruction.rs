use crate::cpu::Cpu;
use crate::bus::Bus;

pub type InstructionFn = fn(&mut Cpu, &mut dyn Bus) -> u8;

pub struct InstructionInfo {
    pub execute_fn: InstructionFn,
    pub bytes: u8,
    pub cycles: u8,
}

