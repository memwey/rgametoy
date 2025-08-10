use rgametoy::registers::Registers;

#[test]
fn test_set_af_masking() {
    let mut registers = Registers::new();
    registers.set_af(0x123F);
    assert_eq!(registers.get_af(), 0x1230);
}

#[test]
fn test_flag_z() {
    let mut registers = Registers::new();
    registers.set_flag_z(true);
    assert_eq!(registers.get_af(), 0x0080);
    assert_eq!(registers.get_flag_z(), true);
    registers.set_flag_z(false);
    assert_eq!(registers.get_af(), 0x0000);
    assert_eq!(registers.get_flag_z(), false);
}

#[test]
fn test_flag_n() {
    let mut registers = Registers::new();
    registers.set_flag_n(true);
    assert_eq!(registers.get_af(), 0x0040);
    assert_eq!(registers.get_flag_n(), true);
    registers.set_flag_n(false);
    assert_eq!(registers.get_af(), 0x0000);
    assert_eq!(registers.get_flag_n(), false);
}

#[test]
fn test_flag_h() {
    let mut registers = Registers::new();
    registers.set_flag_h(true);
    assert_eq!(registers.get_af(), 0x0020);
    assert_eq!(registers.get_flag_h(), true);
    registers.set_flag_h(false);
    assert_eq!(registers.get_af(), 0x0000);
    assert_eq!(registers.get_flag_h(), false);
}

#[test]
fn test_flag_c() {
    let mut registers = Registers::new();
    registers.set_flag_c(true);
    assert_eq!(registers.get_af(), 0x0010);
    assert_eq!(registers.get_flag_c(), true);
    registers.set_flag_c(false);
    assert_eq!(registers.get_af(), 0x0000);
    assert_eq!(registers.get_flag_c(), false);
}

#[test]
fn test_combined_flags() {
    let mut registers = Registers::new();
    registers.set_flag_z(true);
    registers.set_flag_h(true);
    assert_eq!(registers.get_af(), 0x00A0);
    assert_eq!(registers.get_flag_z(), true);
    assert_eq!(registers.get_flag_n(), false);
    assert_eq!(registers.get_flag_h(), true);
    assert_eq!(registers.get_flag_c(), false);
}
