use crate::cpu::Cpu;
use crate::bus::Bus;

pub fn nop(_cpu: &mut Cpu, _bus: &mut dyn Bus) -> u8 {
    // Do nothing
    4
}

pub fn ld_bc_u16(cpu: &mut Cpu, bus: &mut dyn Bus) -> u8 {
    let value = bus.read_u16(cpu.get_pc() + 1); // PC points to opcode, value is after
    cpu.set_bc(value);
    12
}

pub fn add_hl_bc(cpu: &mut Cpu, _bus: &mut dyn Bus) -> u8 {
    let hl = cpu.get_registers().get_hl();
    let bc = cpu.get_registers().get_bc();
    let (new_hl, carry) = hl.overflowing_add(bc);

    cpu.get_registers_mut().set_hl(new_hl);
    cpu.get_registers_mut().set_flag_n(false);
    cpu.get_registers_mut().set_flag_h((hl & 0x0FFF) + (bc & 0x0FFF) > 0x0FFF); // Half-carry for 16-bit addition
    cpu.get_registers_mut().set_flag_c(carry);
    8
}
