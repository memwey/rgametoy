use crate::registers::Registers;
use crate::bus::Bus;
use crate::opcode_table::OPCODE_TABLE;

pub struct Cpu {
    registers: Registers,
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            registers: Registers::new(),
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

    pub fn get_pc(&self) -> u16 {
        self.registers.pc
    }

    pub fn set_pc(&mut self, value: u16) {
        self.registers.pc = value;
    }

    pub fn get_sp(&self) -> u16 {
        self.registers.sp
    }

    pub fn set_sp(&mut self, value: u16) {
        self.registers.sp = value;
    }

    pub fn get_registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }

    pub fn load_program(&mut self, bus: &mut dyn Bus, program: &[u8]) {
        for (i, &byte) in program.iter().enumerate() {
            bus.write_byte(i as u16, byte);
        }
    }

    pub fn step(&mut self, bus: &mut dyn Bus) -> u8 {
        // 1. Fetch opcode
        let opcode = bus.read_byte(self.registers.pc);

        // 2. Get instruction info from table
        let instruction_info = &OPCODE_TABLE[opcode as usize];

        // 3. Execute instruction
        let cycles = (instruction_info.execute_fn)(self, bus);

        // 4. Advance PC
        self.registers.pc += instruction_info.bytes as u16;

        cycles
    }
}
