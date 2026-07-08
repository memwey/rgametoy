# rgametoy

[English](README.md) | **中文**

一个用 Rust 编写的 DMG (初代 Game Boy) 模拟器。仿真核心(`rgametoy-core`)**零第三方依赖**;
桌面前端(`rgametoy-desktop`)才引入 `minifb`(窗口)和可选的 `cpal`(音频)。

## 构建与运行

```sh
cargo run --release -- path/to/rom.gb              # 无声音,零额外依赖
cargo run --release --features audio -- rom.gb     # 开启声音(引入 cpal)
cargo run --release -- rom.gb 8                    # 第二个参数 = 快进倍率(默认 4)
```

按键映射:方向键 = 方向键,`Z` = A,`X` = B,`Enter` = Start,`Backspace` = Select,
**按住 `Tab` = 快进**,`F5` = 即时存档 / `F7` = 即时读档,`F2` = 截图,`F3` = 切换配色,
`Esc` = 退出。窗口标题会实时显示帧率、速度倍率和当前配色。

快进以 CPU 时钟为基准:每个模拟帧的真实时间预算 = `一帧时间 / 倍率`,呈现仍每帧一次;
松开 `Tab` 立即回原速。快进期间音频静音(避免过量采样)。

**数据目录**:存档和截图都放在一个 base 目录下的两个子文件夹,base 默认为当前目录、可用
环境变量 `RGAMETOY_DATA_DIR` 覆盖:

```text
<base>/
├── saves/        <卡带文件名>-<内容哈希8位>.sav   (电池 SRAM)
└── screenshots/  <卡带标题>-<毫秒时间戳>.bmp       (F2 截图)
```

**截图**:`F2` 把当前帧按原生 160×144 存成 24-bit BMP(像素精确,适合调试),存好后在终端
打印路径。编码器与无头的 `-p rgametoy-desktop --example screenshot` 共用一份(`crates/rgametoy-desktop/src/screenshot.rs`),
窗口与截图配色一致。

**配色**:核心只输出 0–3 四级灰度,把灰度映射成颜色是纯前端的选择
(`crates/rgametoy-desktop/src/palette.rs`)。`F3` 在几套内置配色间循环——经典 DMG 绿,以及给四级灰度上色
的变体(grayscale、amber、ocean、berry)。截图使用当前生效的配色。默认是 DMG 绿。

**显示**:窗口把**原生 160×144** 的缓冲交给 minifb 后端(macOS 上是 Metal),由 GPU 做最近邻
放大——每帧上传的数据比在 CPU 上预放大少 16 倍,前端因此很省。窗口标题实时显示 fps、速度倍率
和配色;加 `--features debug` 还会显示每帧 core / present 的耗时拆分。

**日志**:前端信息走一个零依赖的小日志器(`crates/rgametoy-desktop/src/log.rs`)——`info` 到 stdout,
`warn`/`error` 到 stderr,仅当输出是终端且未设 `NO_COLOR` 时才上 ANSI 颜色。测试 ROM 的
串口输出原样打印,不加标签。

支持的卡带:无 MBC (32KB)、MBC1、MBC3(不含 RTC)、MBC5,含外部 RAM 与 bank 切换。
带电池的卡带把外部 RAM 存档持久化到 `saves/<卡带文件名>-<内容哈希>.sav`——文件名可读、
哈希绑 ROM 内容(同名不同 ROM 不撞),`.sav` 本身是裸 SRAM(与其它模拟器通用)。启动时自动
读回,运行中防抖落盘,退出时兜底保存。

已实现:完整 SM83 指令集(含 CB 前缀)、中断(VBlank/STAT/Timer/Serial/Joypad)、Timer、
PPU 像素-FIFO 渲染(背景 / 窗口 / 精灵,mode 3 逐点)、OAM DMA、串口(截获输出)、键盘输入、
截图(F2)、电池存档 (`.sav`)、**APU 四声道声音**(方波 ×2 + 波形 + 噪声,含扫频 / 包络 /
长度计数器)。APU 仿真核心是纯 Rust、默认编译;真实音频输出通过 `audio` feature(cpal)开启,
默认关闭。

另外支持**快进/加速**(按住 Tab,倍率可配)和**即时存档 / 读档 (save state)**
(F5/F7,整机深拷贝到内存槽,零依赖)。PPU 是**像素 FIFO**(mode 3 逐点、行内改寄存器
生效)。尚未实现:MBC3 RTC、MBC2。

## 架构

项目是一个 Cargo **workspace**,两个 crate 按硬件 / 宿主分层。`rgametoy-core` 是被模拟的机器——
确定性、零依赖,能编到 `wasm32`。`rgametoy-desktop` 是驱动它的原生前端(并产出 `rgametoy` 二进制);
将来可以再加一个 `rgametoy-web` crate 作为第三个前端,复用同一份核心。

```text
  crates/rgametoy-desktop   原生前端 —— 窗口、输入、音频、文件(minifb / cpal)
  ───────────────────────   main → Emulator::run(): 读输入 → run_frame → 呈现 → 按帧节流
       modules:             display · input · audio · screenshot · palette · log · paths
                 │  run_frame() / framebuffer()          ▲  set_buttons()
                 ▼  依赖 ↓                                │
  crates/rgametoy-core      被模拟的 DMG —— 确定性、无宿主 I/O、可编 wasm
  ──────────────────────
     Cpu (SM83)  ── 总线主控 ──►  MemoryBus  (地址译码,拥有下面全部外设)
        每次访存 / 内部延迟都调 bus.tick(n)  ──┐
                                              ▼  把 n 个 T-cycle 分发给:
     受时钟:  Ppu (像素 FIFO)   Timer (DIV/TIMA)   Apu (4 声道)   Serial (串口)
     被动:    Cartridge (MBC1/3/5 + 电池)   P1 (手柄)   WRAM   HRAM
```

- **`rgametoy-core`** 是被模拟的机器——无宿主 I/O、完全确定性,所以存档就是一次深拷贝
  (只读的 ROM 用 `Arc` 共享、不复制)。
- **`Cpu` 是唯一的总线主控**:每次访存和内部延迟都调 `bus.tick(n)`,把**受时钟**的外设
  (PPU/Timer/APU/Serial)推进 `n` 个 T-cycle。**这一条 seam** 正是让读写时序可观测的关键。
  **被动**外设(卡带、手柄、RAM)只在被访问时响应。
- **`rgametoy-desktop`** 每个宿主帧跑一次 `run_frame()`,再呈现 framebuffer 并节流到真实的 ~59.7 Hz。

## 时序

一个 M-cycle = 4 个 T-cycle = 4 个 PPU dot;时间**只通过 CPU 的 tick 推进**:

```text
  CPU 步:     取指      读       内部延迟
              [ 4T ]    [ 4T ]    [ 4T ]
  bus.tick:    ►►►►      ►►►►      ►►►►     每次把 PPU/Timer/APU/Serial 各推进 4 dot
                                           (寄存器写落在 T4,即 M-cycle 末拍)
```

PPU 是逐 dot 的状态机。一条扫描线 = 456 dot(开屏后的特殊首行是 452);LY 在行尾 +1:

```text
  dot        0           80                    252                        456
  内部       │─ mode 2 ──│──── mode 3 ─────────│──────── mode 0 ──────────│
              OAM 扫描     绘制                  HBlank
              80 dot       172 + SCX&7 + 精灵/窗口惩罚

  软件从 STAT 读到的 mode 比内部沿**滞后**:出 scan 晚 4 dot、出 Drawing 晚 1 dot
  (好几个 mooneye 测试就卡这点)——
  STAT&3     │0─│──── mode 2 ────│──── mode 3 ─────│──────── mode 0 ──────────│
  dot        0  4                84               253
              └ 行首 4 dot 沿用上一行 HBlank 的 mode 0

  一帧 = 144 可见行 + 10 VBlank 行 = 154 × 456 = 70224 dot ≈ 59.7 Hz。
```

mode 3 长度、STAT 滞后、开屏首行背后的逐 dot 细节见 [docs/testing_cn.md](docs/testing_cn.md) §3。

### 测试 ROM 验证

CPU 是**逐 M-cycle 精确**的(每次访存/内部周期都推进外设),PPU mode-3 逐点。通过
Blargg(`cpu_instrs` 全 11 项、`instr_timing`、`mem_timing` 均 **Passed**)、**dmg-acid2**
(渲染出完整参考笑脸)、mooneye acceptance **63/75(非 boot 全过**,剩 12 个全是 boot 类,
不在 DMG 范围);mealybug tearoom 1/24(最严一档,详见文档)。

测试方法(灰盒模块单测 + 黑盒 ROM 套件)、各套件通过情况与遗留问题(亚周期/T-cycle 前沿)
详见 [docs/testing_cn.md](docs/testing_cn.md)。

```sh
cargo test --release                                              # 灰盒模块单测
GB_TEST_ROMS=/path/to/game-boy-test-roms \
    cargo test --release -p rgametoy-core --test rom_suite                         # mooneye 非 boot + Blargg
cargo run --release -p rgametoy-core --example run_serial  -- path/to/test.gb      # 打印串口输出(Blargg)
cargo run --release -p rgametoy-core --example run_mooneye -- path/to/test.gb      # 打印 PASS / FAIL(mooneye)
cargo run --release -p rgametoy-desktop --example screenshot  -- rom.gb out.bmp       # 无头渲染一帧到 BMP
cargo run --release -p rgametoy-core --example benchmark   -- rom.gb [帧数]        # 无头跑吞吐:fps + 倍速
```

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
* [GameRoy](https://github.com/Rodrigodd/gameroy) —— 参考其开屏首行(line 0 mode 3 落 cycle 84)与 OBJ 惩罚实现
* [jgb](https://github.com/jsgroth/jgb)
* [Mooneye GB](https://github.com/Gekkio/mooneye-gb)
* [GateBoy](https://github.com/aappleby/MetroBoy)
* [SameBoy](https://github.com/LIJI32/SameBoy) —— 高精度参考,标定开屏首行(lcdon)时序时对照
### 测试 ROM 套件
* [c-sp/game-boy-test-roms](https://github.com/c-sp/game-boy-test-roms) —— 下列各套件的打包发布(本项目取 v7.0,经 `GB_TEST_ROMS` 喂给 `rom_suite`)
* [Blargg's gb-test-roms](https://github.com/retrio/gb-test-roms) —— `cpu_instrs` / `instr_timing` / `mem_timing`(串口判定)
* [mooneye-test-suite](https://github.com/Gekkio/mooneye-test-suite) —— CPU/PPU/timer 逐周期精度验收;解码其 `.s` 期望表标定了 `lcdon` / `intr_2` / `rapid_toggle`
* [dmg-acid2](https://github.com/mattcurrie/dmg-acid2) —— PPU 渲染一张参考笑脸,逐像素比对
* [Mealybug Tearoom Tests](https://github.com/mattcurrie/mealybug-tearoom-tests) —— mode-3 行内改寄存器,逐像素比对(见 `tools/mealybug_compare.py`)
