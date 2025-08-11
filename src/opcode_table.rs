use crate::instruction::InstructionInfo;
use crate::instructions; // Import the new instructions module

pub const OPCODE_TABLE: [InstructionInfo; 256] = [
    // 0x00 NOP
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 },
    // 0x01 LD BC, u16
    InstructionInfo { execute_fn: instructions::ld_bc_u16, bytes: 3, cycles: 12 },
    // 0x02
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 },
    // 0x03
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 },
    // 0x04
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 },
    // 0x05
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 },
    // 0x06
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 },
    // 0x07
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 },
    // 0x08
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 },
    // 0x09 ADD HL, BC
    InstructionInfo { execute_fn: instructions::add_hl_bc, bytes: 1, cycles: 8 },
    // 0x0A to 0xFF (Fill with NOP for now)
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x0A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x0B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x0C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x0D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x0E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x0F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x10
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x11
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x12
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x13
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x14
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x15
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x16
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x17
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x18
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x19
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x1A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x1B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x1C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x1D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x1E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x1F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x20
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x21
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x22
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x23
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x24
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x25
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x26
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x27
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x28
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x29
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x2A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x2B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x2C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x2D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x2E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x2F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x30
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x31
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x32
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x33
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x34
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x35
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x36
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x37
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x38
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x39
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x3A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x3B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x3C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x3D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x3E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x3F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x40
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x41
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x42
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x43
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x44
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x45
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x46
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x47
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x48
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x49
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x4A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x4B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x4C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x4D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x4E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x4F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x50
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x51
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x52
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x53
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x54
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x55
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x56
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x57
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x58
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x59
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x5A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x5B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x5C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x5D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x5E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x5F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x60
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x61
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x62
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x63
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x64
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x65
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x66
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x67
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x68
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x69
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x6A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x6B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x6C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x6D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x6E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x6F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x70
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x71
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x72
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x73
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x74
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x75
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x76
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x77
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x78
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x79
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x7A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x7B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x7C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x7D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x7E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x7F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x80
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x81
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x82
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x83
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x84
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x85
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x86
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x87
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x88
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x89
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x8A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x8B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x8C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x8D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x8E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x8F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x90
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x91
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x92
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x93
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x94
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x95
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x96
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x97
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x98
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x99
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x9A
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x9B
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x9C
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x9D
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x9E
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0x9F
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA0
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA1
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA2
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA3
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA4
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA5
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA6
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA7
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA8
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xA9
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xAA
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xAB
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xAC
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xAD
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xAE
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xAF
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB0
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB1
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB2
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB3
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB4
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB5
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB6
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB7
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB8
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xB9
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xBA
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xBB
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xBC
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xBD
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xBE
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xBF
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC0
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC1
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC2
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC3
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC4
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC5
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC6
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC7
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC8
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xC9
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xCA
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xCB (Prefix for CB instructions)
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xCC
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xCD
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xCE
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xCF
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD0
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD1
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD2
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD3
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD4
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD5
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD6
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD7
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD8
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xD9
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xDA
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xDB
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xDC
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xDD
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xDE
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xDF
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE0
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE1
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE2
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE3
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE4
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE5
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE6
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE7
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE8
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xE9
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xEA
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xEB
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xEC
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xED
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xEE
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xEF
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF0
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF1
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF2
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF3
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF4
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF5
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF6
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF7
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF8
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xF9
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xFA
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xFB
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xFC
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xFD
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xFE
    InstructionInfo { execute_fn: instructions::nop, bytes: 1, cycles: 4 }, // 0xFF
];