# rgametoy

一个用 Rust 编写的 DMG (初代 Game Boy) 模拟器,核心逻辑不依赖第三方库(仅用 `minifb` 做窗口显示)。

## 构建与运行

```sh
cargo run --release -- path/to/rom.gb
```

按键映射:方向键 = 方向键,`Z` = A,`X` = B,`Enter` = Start,`Backspace` = Select,`Esc` = 退出。

支持的卡带:无 MBC (32KB)、MBC1、MBC3(不含 RTC)、MBC5,含外部 RAM 与 bank 切换。

已实现:完整 SM83 指令集(含 CB 前缀)、中断(VBlank/STAT/Timer/Joypad)、Timer、
PPU 扫描线渲染(背景 / 窗口 / 精灵)、OAM DMA、键盘输入。尚未实现:声音 (APU)、
串口、存档持久化、亚扫描线级 (FIFO) 时序精度。

## 参考资料
### 技术手册
* [Pan Docs](https://gbdev.io/pandocs/)
* [Game Boy / Color Architecture](https://www.copetti.org/writings/consoles/game-boy/)
* [Gameboy Emulator Development Guide](https://github.com/Hacktix/GBEDG)
* [Game Boy: Complete Technical Reference](https://github.com/Gekkio/gb-ctr)
### 教程文章
* [Building a Gameboy From Scratch](https://raphaelstaebler.medium.com/building-a-gameboy-from-scratch-part-1-51d05496783e)
* [从零开始实现GameBoy模拟器](https://zhuanlan.zhihu.com/p/676908347)
* [Rewriting My Game Boy Emulator: The Pixel FIFO](https://jsgroth.dev/blog/posts/gb-rewrite-pixel-fifo/)
### 模拟器项目
* [GB Studio](https://github.com/chrismaltby/gb-studio)
* [GoBoy](https://github.com/Humpheh/goboy/)
* [PyBoy](https://github.com/Baekalfen/PyBoy/)
* [DawnGB](https://github.com/akatsuki105/dawngb)
* [Azayaka](https://github.com/7thSamurai/Azayaka)
* [Rugby](https://github.com/kaplanz/rugby)
* [GameRoy](https://github.com/Rodrigodd/gameroy)
* [jgb](https://github.com/jsgroth/jgb)
* [Mooneye GB](https://github.com/Gekkio/mooneye-gb)
* [GateBoy](https://github.com/aappleby/MetroBoy)
### 表现测试
* [dmg-acid2](https://github.com/mattcurrie/dmg-acid2)
* [Mealybug Tearoom Tests](https://github.com/mattcurrie/mealybug-tearoom-tests)
