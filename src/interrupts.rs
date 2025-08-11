pub enum InterruptType {
    VBlank,
    LCDStat,
    Timer,
    Serial,
    Joypad,
}

impl InterruptType {
    pub fn to_bit(&self) -> u8 {
        match self {
            InterruptType::VBlank => 0x01,
            InterruptType::LCDStat => 0x02,
            InterruptType::Timer => 0x04,
            InterruptType::Serial => 0x08,
            InterruptType::Joypad => 0x10,
        }
    }

    pub fn to_handler_address(&self) -> u16 {
        match self {
            InterruptType::VBlank => 0x0040,
            InterruptType::LCDStat => 0x0048,
            InterruptType::Timer => 0x0050,
            InterruptType::Serial => 0x0058,
            InterruptType::Joypad => 0x0060,
        }
    }
}
