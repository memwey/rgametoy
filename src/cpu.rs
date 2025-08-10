use crate::registers::Registers;
use crate::mmu::Mmu;
use crate::instruction::Instruction;
use crate::opcode_table::OPCODE_TABLE;

pub struct Cpu {
    registers: Registers,
    mmu: Mmu,
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            registers: Registers::new(),
            mmu: Mmu::new(),
        }
    }

    pub fn get_registers(&self) -> &Registers {
        &self.registers
    }

    pub fn set_hl(&mut self, value: u16) {
        self.registers.set_hl(value);
    }

    pub fn set_bc(&mut self, value: u16) {
        self.registers.set_bc(value);
    }

    pub fn set_de(&mut self, value: u16) {
        self.registers.set_de(value);
    }

    pub fn set_af(&mut self, value: u16) {
        self.registers.set_af(value);
    }

    pub fn set_pc(&mut self, value: u16) {
        self.registers.pc = value;
    }

    pub fn set_sp(&mut self, value: u16) {
        self.registers.sp = value;
    }

    pub fn load_program(&mut self, program: &[u8]) {
        for (i, &byte) in program.iter().enumerate() {
            self.mmu.write_byte(i as u16, byte);
        }
    }

    pub fn step(&mut self) -> u8 {
        // 1. Fetch opcode
        let opcode = self.mmu.read_byte(self.registers.pc);

        // 2. Get instruction info from table
        let instruction_info = &OPCODE_TABLE[opcode as usize];

        // 3. Execute instruction
        let cycles = self.execute(&instruction_info.instruction);

        // 4. Advance PC
        self.registers.pc += instruction_info.bytes as u16;

        cycles
    }

    fn execute(&mut self, instruction: &Instruction) -> u8 {
        match instruction {
            Instruction::NOP => {
                // Do nothing
                4 // Return number of cycles
            }
            Instruction::LdBcU16 => {
                let value = self.mmu.read_u16(self.registers.pc);
                self.registers.set_bc(value);
                12 // Return number of cycles
            }
            Instruction::AddHlBc => {
                let hl = self.registers.get_hl();
                let bc = self.registers.get_bc();
                let (new_hl, carry) = hl.overflowing_add(bc);

                self.registers.set_hl(new_hl);
                self.registers.set_flag_n(false);
                self.registers.set_flag_h((hl & 0x0FFF) + (bc & 0x0FFF) > 0x0FFF); // Half-carry for 16-bit addition
                self.registers.set_flag_c(carry);
                8 // Return number of cycles
            }
        }
    }
}
