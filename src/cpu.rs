use crate::registers::Registers;
use crate::bus::Bus;
use crate::opcode_table::OPCODE_TABLE;
use crate::interrupts::InterruptType; // Import InterruptType

pub struct Cpu {
    registers: Registers,
    ime: bool, // Interrupt Master Enable
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            registers: Registers::new(),
            ime: false, // IME is disabled on startup
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
        // Handle interrupts before fetching next instruction
        if self.ime {
            let if_reg = bus.read_byte(0xFF0F);
            let ie_reg = bus.read_byte(0xFFFF);
            let pending_interrupts = if_reg & ie_reg;

            if pending_interrupts != 0 {
                // Find the highest priority interrupt
                let interrupt_type = if pending_interrupts & InterruptType::VBlank.to_bit() != 0 {
                    Some(InterruptType::VBlank)
                } else if pending_interrupts & InterruptType::LCDStat.to_bit() != 0 {
                    Some(InterruptType::LCDStat) 
                } else if pending_interrupts & InterruptType::Timer.to_bit() != 0 {
                    Some(InterruptType::Timer)
                } else if pending_interrupts & InterruptType::Serial.to_bit() != 0 {
                    Some(InterruptType::Serial)
                } else if pending_interrupts & InterruptType::Joypad.to_bit() != 0 {
                    Some(InterruptType::Joypad)
                } else {
                    None
                };

                if let Some(int_type) = interrupt_type {
                    self.ime = false; // Disable master interrupt enable
                    // Push PC onto stack
                    let sp = self.registers.sp;
                    bus.write_byte(sp - 1, (self.registers.pc >> 8) as u8);
                    bus.write_byte(sp - 2, self.registers.pc as u8);
                    self.registers.sp -= 2;

                    self.registers.pc = int_type.to_handler_address(); // Jump to handler
                    bus.write_byte(0xFF0F, if_reg & !int_type.to_bit()); // Clear interrupt flag
                    return 20; // Cycles for interrupt handling (approx)
                }
            }
        }

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

    pub fn enable_interrupts(&mut self) {
        self.ime = true;
    }

    pub fn disable_interrupts(&mut self) {
        self.ime = false;
    }
}
