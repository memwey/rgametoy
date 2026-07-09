//! The five DMG interrupt sources. Each owns a bit in the IE (0xFFFF) / IF
//! (0xFF0F) registers and a fixed handler entry in the interrupt vector table;
//! priority runs highest (VBlank) to lowest (Joypad), matching bit order.

/// One of the five interrupt sources, ordered by priority / bit position.
pub enum InterruptType {
    VBlank,
    LCDStat,
    Timer,
    Serial,
    Joypad,
}

impl InterruptType {
    /// This source's bit in the IE / IF registers (VBlank = 0x01 … Joypad = 0x10).
    pub fn to_bit(&self) -> u8 {
        match self {
            InterruptType::VBlank => 0x01,
            InterruptType::LCDStat => 0x02,
            InterruptType::Timer => 0x04,
            InterruptType::Serial => 0x08,
            InterruptType::Joypad => 0x10,
        }
    }

    /// The fixed vector the CPU jumps to when dispatching this interrupt
    /// (VBlank = 0x0040, then +8 per source down to Joypad = 0x0060).
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
