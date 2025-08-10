


pub struct Registers {
    // Accumulator and Flags
    pub af: u16,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    // Stack Pointer
    pub sp: u16,
    // Program Counter
    pub pc: u16,
}

const ZERO_FLAG_BYTE_POSITION: u8 = 7; // z
const SUBTRACT_FLAG_BYTE_POSITION: u8 = 6; // n
const HALF_CARRY_FLAG_BYTE_POSITION: u8 = 5; // h
const CARRY_FLAG_BYTE_POSITION: u8 = 4; // c

impl Registers {
    pub fn new() -> Registers {
        Registers {
            af: 0,
            bc: 0,
            de: 0,
            hl: 0,
            pc: 0,
            sp: 0,
        }
    }

    pub fn get_flag_z(&self) -> bool {
        (self.af >> ZERO_FLAG_BYTE_POSITION) & 0b1 != 0
    }

    pub fn set_flag_z(&mut self, value: bool) {
        if value {
            self.af |= 1 << ZERO_FLAG_BYTE_POSITION;
        } else {
            self.af &= !(1 << ZERO_FLAG_BYTE_POSITION);
        }
    }

    pub fn get_flag_n(&self) -> bool {
        (self.af >> SUBTRACT_FLAG_BYTE_POSITION) & 0b1 != 0
    }

    pub fn set_flag_n(&mut self, value: bool) {
        if value {
            self.af |= 1 << SUBTRACT_FLAG_BYTE_POSITION;
        } else {
            self.af &= !(1 << SUBTRACT_FLAG_BYTE_POSITION);
        }
    }

    pub fn get_flag_h(&self) -> bool {
        (self.af >> HALF_CARRY_FLAG_BYTE_POSITION) & 0b1 != 0
    }

    pub fn set_flag_h(&mut self, value: bool) {
        if value {
            self.af |= 1 << HALF_CARRY_FLAG_BYTE_POSITION;
        } else {
            self.af &= !(1 << HALF_CARRY_FLAG_BYTE_POSITION);
        }
    }

    pub fn get_flag_c(&self) -> bool {
        (self.af >> CARRY_FLAG_BYTE_POSITION) & 0b1 != 0
    }

    pub fn set_flag_c(&mut self, value: bool) {
        if value {
            self.af |= 1 << CARRY_FLAG_BYTE_POSITION;
        } else {
            self.af &= !(1 << CARRY_FLAG_BYTE_POSITION);
        }
    }

    pub fn get_af(&self) -> u16 {
        self.af
    }

    pub fn set_af(&mut self, value: u16) {
        self.af = value & 0xFFF0;
    }

    pub fn get_bc(&self) -> u16 {
        self.bc
    }

    pub fn set_bc(&mut self, value: u16) {
        self.bc = value;
    }

    pub fn get_de(&self) -> u16 {
        self.de
    }

    pub fn set_de(&mut self, value: u16) {
        self.de = value;
    }

    pub fn get_hl(&self) -> u16 {
        self.hl
    }

    pub fn set_hl(&mut self, value: u16) {
        self.hl = value;
    }
}
