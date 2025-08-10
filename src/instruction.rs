#[derive(Copy, Clone)]
pub enum Instruction {
    NOP,
    LdBcU16,
    AddHlBc,
    // Add other instructions here
}

pub struct InstructionInfo {
    pub instruction: Instruction,
    pub bytes: u8,
    pub cycles: u8,
}

