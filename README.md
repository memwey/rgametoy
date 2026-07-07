# rgametoy

一个用 Rust 编写的 DMG (初代 Game Boy) 模拟器,核心逻辑不依赖第三方库(仅用 `minifb` 做窗口显示)。

## 构建与运行

```sh
cargo run --release -- path/to/rom.gb              # 无声音,零额外依赖
cargo run --release --features audio -- rom.gb     # 开启声音(引入 cpal)
cargo run --release -- rom.gb 8                    # 第二个参数 = 快进倍率(默认 4)
```

按键映射:方向键 = 方向键,`Z` = A,`X` = B,`Enter` = Start,`Backspace` = Select,
**按住 `Tab` = 快进**,`Esc` = 退出。

快进以 CPU 时钟为基准:每个模拟帧的真实时间预算 = `一帧时间 / 倍率`,呈现仍每帧一次;
松开 `Tab` 立即回原速。快进期间音频静音(避免过量采样)。

支持的卡带:无 MBC (32KB)、MBC1、MBC3(不含 RTC)、MBC5,含外部 RAM 与 bank 切换。
带电池的卡带会把外部 RAM 存档持久化到与 ROM 同目录的 `<rom>.sav` 文件
(启动时自动读回,运行中防抖落盘,退出时兜底保存)。

已实现:完整 SM83 指令集(含 CB 前缀)、中断(VBlank/STAT/Timer/Joypad)、Timer、
PPU 扫描线渲染(背景 / 窗口 / 精灵)、OAM DMA、键盘输入、电池存档 (`.sav`)、
**APU 四声道声音**(方波 ×2 + 波形 + 噪声,含扫频 / 包络 / 长度计数器)。APU 仿真核心
是纯 Rust、默认编译;真实音频输出通过 `audio` feature(cpal)开启,默认关闭。

另外支持**快进/加速**(按住 Tab,倍率可配)。尚未实现:串口、存档即时快照 (save state)、
亚扫描线级 (FIFO) 时序精度、MBC3 RTC。

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
