use rgametoy::console::ppu::{Ppu, PpuMode};
// Measure mode-3 length on line 2 (a normal line; line 0 after enable is special).
fn mode3(scx: u8, sprite_x: Option<u8>) -> u32 {
    let mut ppu = Ppu::new();
    if let Some(x) = sprite_x {
        ppu.write_oam(0xFE00, 18); ppu.write_oam(0xFE01, x); // Y=18 -> screen line 2
        ppu.write_oam(0xFE02, 0); ppu.write_oam(0xFE03, 0);
        ppu.write_register(0xFF40, 0x93);
    } else { ppu.write_register(0xFF40, 0x91); }
    ppu.write_register(0xFF43, scx);
    while ppu.ly != 2 { ppu.tick(1); }
    let (mut o, mut d, mut dot) = (0u32, 0u32, 0u32);
    while ppu.ly == 2 {
        let p = ppu.get_mode(); ppu.tick(1); dot += 1;
        if p==PpuMode::OamScan && ppu.get_mode()==PpuMode::Drawing { o = dot; }
        if p==PpuMode::Drawing && ppu.get_mode()==PpuMode::HBlank { d = dot; }
    }
    d - o
}
fn main() {
    println!("no sprite, SCX=0: mode3={} (hw 172)", mode3(0, None));
    for x in [0u8, 8, 9, 12, 15] {
        let hw = 172 + if x==0 {11} else {11 - (x as u32 % 8).min(5)};
        println!("sprite OAM X={}: mode3={} (hw {})", x, mode3(0, Some(x)), hw);
    }
}
