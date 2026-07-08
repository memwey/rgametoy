# 测试方法、用例情况与遗留问题

[English](testing.md) | **中文**

本文记录 rgametoy 的验证方式、当前测试通过情况,以及尚未攻克的遗留问题。
目标始终是"尽量贴近真实硬件结构 + T-cycle 精度"(见 [AGENTS.md](../AGENTS.md)),
因此除了常规单元测试,主要靠社区标准**测试 ROM** 来度量精度。

---

## 1. 测试方法

测试分两层——**灰盒**模块单测(快,钉内部时序)+ **黑盒** ROM 套件(ground truth)。

### 1.1 模块单测(`cargo test`)—— 黑盒;要偷看只走调试工具

原则两条:
1. **测试不自己伸手抠内部**——不读私有字段、不给测试专门开 `pub`。默认全走软件能观测的面:
   寄存器读写(`read_register`/`write_register`=MMIO)、`tick`、framebuffer、中断、内存可访问性,
   和真实程序、和每个测试 ROM 一样。
2. **确实要看软件读不到的内部量时,走"调试工具提供的接口"**——`--features debug` 后面的
   `Ppu::get_mode`/`debug_state`(同一套给 `inspect`/`ppu_probe` 用的检查器),而不是自己抠字段。
   这类测试也 `#[cfg(feature="debug")]` 门控,`cargo test --features debug` 才编译/跑;默认公共
   API 保持黑盒。

一个 Game Boy 的"外部可观测面"是封闭的,大多数内部时序其实能从这面直接测:例如 STAT mode
时间线(mode 2 到 dot 83、mode 3 从 dot 84,含 4-dot 滞后)、开屏首行(LY 到 dot 452、无 scan)——
这些是默认黑盒测试。只有 mode-3 **内部长度**(172 + 惩罚)这种软件读不到绝对值的,才放
`ppu_test.rs` 的 `#[cfg(feature="debug")] mod debug_timing`,用 `get_mode` 观测;其可观测后果
另由 rom_suite 的 mooneye `intr_2`/`lcdon` 黑盒兜底(见 §1.2)。

| 位置 | 覆盖 |
|---|---|
| `tests/ppu_test.rs`(默认黑盒) | STAT mode 时间线、开屏首行(LY 452 + 无 scan)、LY==LYC、OAM/VRAM 锁、精灵优先级/OBP、WX<7 裁剪、行内改色、整帧渲染 |
| `tests/ppu_test.rs` `#[cfg(debug)] mod debug_timing` | mode-3 内部长度 / SCX / 精灵惩罚聚合(经调试工具 `get_mode`,`--features debug` 才跑) |
| `tests/cpu_instructions_test.rs` / `registers_test.rs` | 指令语义、标志位、寄存器 |
| `tests/cpu_integration_test.rs` | 整程序跑通、`ADD HL` 进位、`ie_push` 向量重算、非法码锁死 |
| `tests/timer_test.rs` | 16 位计数器、四频率、下降沿毛刺(TAC/DIV)、重载延迟三态 |
| `tests/dma_test.rs` | OAM DMA 启动延迟、源总线阻塞(VRAM/外部)、echo 源、I/O 可读 |
| `tests/joypad_test.rs` | P1 选择线映射、中断按选择线门控、松开/切组暴露已按键的边沿 |
| `tests/apu_test.rs` | 四声道、包络/扫频/长度、DAC |
| `tests/cartridge_test.rs` / `save_test.rs` / `savestate_test.rs` | MBC、电池存档、即时存档 |
| `tests/serial_test.rs` / `rom_render_test.rs` | 串口截获、整帧渲染 |
| `tests/common/mod.rs` | 共享脚手架(黑盒 PPU helper + ROM runner),被 `tests/*` 按需 `mod common;` 引入 |

```sh
cargo test --release
```

### 1.2 测试 ROM 验证 —— 黑盒

用业界公认的测试 ROM 度量精度。ROM **不随仓库分发**(体积 + 版权),从
[`c-sp/game-boy-test-roms`](https://github.com/c-sp/game-boy-test-roms) 的
release bundle(本项目用 v7.0)获取。每种 ROM 的"通过信号"不同:

| 套件 | 判定方式 | 入口 |
|---|---|---|
| **Blargg** | 把结果("Passed"/"Failed")打到**串口**,截获比对 | `rom_suite` / `examples/run_serial.rs` |
| **mooneye** | 成功时把斐波那契签名 `3,5,8,13,21,34` 载入 `B,C,D,E,H,L`,失败为其它值 | `rom_suite` / `examples/run_mooneye.rs` |
| **dmg-acid2** | 渲染一张参考图,**逐像素**比对 | `examples/screenshot.rs`(转 BMP)/ `dump_fb.rs` |
| **mealybug** | 某一特定帧的画面与参考 PNG(`*_dmg_blob.png`)**逐像素**比对 | `tools/mealybug_compare.py` |

**入库的 ROM 集成测试(`tests/rom_suite.rs`)**:把**自带信号**的黑盒套件做成 `cargo test`
——设 `GB_TEST_ROMS` 指向 bundle 根就跑,没设就整体 skip(默认 `cargo test` 不受影响):

```sh
GB_TEST_ROMS=/path/to/game-boy-test-roms cargo test --release --test rom_suite
```

两个断言:mooneye acceptance **非 boot 全过**(遍历,跳过 `boot*`)、Blargg cpu/timing 串口
"Passed"。这两类靠寄存器签名/串口自证,**不需要参考数据**,所以适合入库自动跑。

**mealybug 用本地脚手架而非自动测试**:它是**逐像素相似度**(非 pass/fail),每个测试要一张
参考图——而参考图(`<name>_dmg_blob.png`)本就和 ROM 同目录躺在 bundle 里,再入库一份纯属
冗余。故用 `tools/mealybug_compare.py`(见 §2.4):有 ROM 就读旁边的 PNG 比对,**零入库数据**。
用 Python 是因为 Rust std 没 inflate、解 PNG 得引三方库(违背核心不依赖三方库),而 Python
stdlib 的 zlib 直接能解。

**调试器(`--features debug`)**:项目内检查器(见 `src/console/debug.rs`),`Console::snapshot()`
一次拿到整机可观测状态(含寄存器看不到的 PPU 内部 mode/dot、STAT 线、LY==LYC 锁存),
`run_until` 打断点。`examples/inspect.rs` 是它的 CLI:

```sh
cargo run --release --features debug --example inspect -- rom.gb break 0x48    # 跑到 PC 后打全快照
cargo run --release --features debug --example inspect -- rom.gb watch 0x48 6  # 每次命中 PC 打快照
cargo run --release --features debug --example inspect -- rom.gb line 0        # 某扫描线的 mode/dot 变化
cargo run --release --features debug --example inspect -- rom.gb dumpat PC A L  # 命中 PC 时 dump 一段内存
```
`stat_lyc_onoff`、`lcdon_*` 就是靠它逐步定位后修好的。

几个判定细节:
- **mooneye** 的斐波那契签名是所有硬件测试通用的"通过"约定。`rom_suite`/`run_mooneye` 跑
  若干帧后读寄存器判定:240 帧够绝大多数;`intr_2_mode0_timing_sprites` 要 ~2900 帧
  (100+ 个 testcase 逐个等帧),故 240 不过再兜到 3000。
- **mealybug** 的效果只在**一帧**上出现:CPU 做完一串按拍对齐的行内写入后执行 `ld b,b`(断点)
  就停,之后的帧不带效果重绘;但这些测试每帧循环重画同一效果,取第 ~30 帧即稳定。参考 PNG 是
  grayscale(bit-depth 1/2),映射到我们的 0–3 shade 的极性是 `shade = 3 - gray`(gray 归一到
  0–3)。**100% 命中才算 PASS**。

---

## 2. 用例情况(scoreboard)

> 数据对应当前 `dev` 分支。核心 CPU 为**逐 M-cycle 精确**(每次访存/内部周期都推进外设)。

### 2.1 Blargg —— 全过 ✅

| ROM | 结果 |
|---|---|
| `cpu_instrs`(全 11 项) | Passed |
| `instr_timing` | Passed |
| `mem_timing` | Passed |

### 2.2 dmg-acid2 —— 通过 ✅

渲染出完整参考笑脸(FIFO 重写前后字节一致,佐证渲染正确)。

### 2.3 mooneye acceptance —— 63 / 75(非 boot 全过 ✅)

| 分组 | 成绩 | 备注 |
|---|---|---|
| bits | 3/3 ✅ | |
| instr | 1/1 ✅ | |
| interrupts | 1/1 ✅ | `ie_push`(压栈盖 IE 重算向量,见 §3.1) |
| oam_dma | 3/3 ✅ | 源总线阻塞(§3.1) |
| timer | 13/13 ✅ | `rapid_toggle`(TAC 写落 T3,timer 内重放,§3.2) |
| ppu | 12/12 ✅ | STAT 滞后(§3.3)、`lcdon_*`(§3.4)、`intr_2_..._sprites`(§3.5) |
| serial | 0/1 | `boot_sclk_align`(需 boot 时序,不在范围) |
| root | 30/41 | 只剩 boot 类(§3.8);控制流读/写时序整簇已过(§3.1) |

剩下 12 个失败**全是 Boot 状态**(11 个 boot 类 + `boot_sclk_align`):校验**特定机型**开机后的
寄存器/IO/DIV 状态——我们只做 DMG、也不跑真实 boot ROM,故不在目标范围(真正的 DMG 变体
`*-dmgABC` 已过)。见 §3.8。

### 2.4 mealybug tearoom(DMG)—— 1 / 24 通过,逐像素相似度已量化

入库的比对脚手架 `tools/mealybug_compare.py`(纯 stdlib zlib 解 PNG,极性 `3 - gray`;读 bundle
里和 ROM 同目录的 `*_dmg_blob.png`,零入库数据):

```sh
GB_TEST_ROMS=/path/to/game-boy-test-roms tools/mealybug_compare.py [name-substr]
# 打印逐测试相似度 + pixel-perfect 计数(可选 name-substr 只跑一撮)
```

**PASS 需 100%**。当前:

| 相似度 | 测试 |
|---|---|
| **100%** ✅ | `m2_win_en_toggle` |
| 99%+ | `m3_wx_4_change_sprites`(99.96)、`m3_scx_high_5_bits`(99.64)、`m3_lcdc_obj_en_change`(99.37)、`m3_lcdc_obj_size_change_scx`(99.18)、`m3_wx_4_change`(99.01) |
| 95–99% | `m3_window_timing_wx_0`、`m3_obp0_change`、`m3_lcdc_obj_size_change`、`m3_scx_low_3_bits`、`m3_wx_5_change`、`m3_lcdc_bg_map_change`、`m3_lcdc_obj_en_change_variant` |
| 88–94% | `m3_lcdc_win_map_change`、`m3_window_timing`、`m3_lcdc_tile_sel_win_change`、`m3_lcdc_tile_sel_change`、`m3_lcdc_bg_en_change` |
| < 80% | `m3_bgp_change`(78)、`m3_bgp_change_sprites`(75)、`m3_lcdc_win_en_change_multiple_wx`(74)、`m3_lcdc_win_en_change_multiple`(64)、`m3_scy_change`(58)、`m3_wx_6_change`(40) |

已修 **WX<7 窗口左裁**(§3.6),把 `m3_wx_4_change` 56→99、`m3_wx_5_change` 59→97、
`m3_window_timing_wx_0` 96→99 顶上去。其余每个都是**独立的逐 dot 时序谜题**(逐一定性见 §3.6),
mealybug 是最严一档,多数成熟模拟器也长期停在个位数 PASS。

---

## 3. 遗留问题与关键修复

一句话:**mooneye 非 boot 全过**——M-cycle 粒度与逐点(dot/T)粒度的验收测试都已到位。
剩下 boot 类(不在范围)与 mealybug(mode-3 行内特效的**具体锁存 dot**,最严一档)。
下面按子系统记录"怎么修的/学到什么",供后续 mealybug 攻坚复用方法。

### 3.1 控制流读/写时序 + `ie_push`(总线/中断)
mooneye 这些 timing 测试用 **OAM DMA 当示波器**:把栈指进 OAM、或让指令从 ROM 取指,
再用 DMA 窗口卡边界。**写**方向修法:启动延迟 + 窗口内写丢弃。

**读**方向的根因用 trace + 反汇编定位:`ret_timing` 的 RET 在 **ROM**(0x0192),DMA 源是
**$80(VRAM)**。真机上 **DMA 只占用与源冲突的那条总线**:VRAM 源占**视频总线**(VRAM+OAM),
CPU 仍可读**外部总线**(ROM)——RET 取指成功,只有 OAM 的 pop 被挡。我们之前无脑阻塞
`addr < 0xFEA0`(所有源一样),把 ROM 取指也挡了 → 读到 `$FF` = RST 38 → 跑飞。改成按源分总线
阻塞(`dma_conflicts`,仅 OAM 恒锁)后,**整簇 9 个读时序全绿**,oam_dma 组不回归。

`ie_push`:中断派发压返回地址高字节时,若 `SP=0` 则该字节落在 **IE(0xFFFF)**、改写使能位,
向量按**压栈后**的 IE 重选——为 Timer 启动的派发会落到 VBlank 向量。已在 `service_interrupt`
里"压高字节 → 重采样 IE&IF → 选向量"实现(单测 `test_ie_push_...`)。

### 3.2 `rapid_toggle` —— 已修:TAC 写落 T3,在 timer 内部重放
用 `inspect` 量化:真机 timer 中断在 **BC=FFD9** 服务,我们晚一圈(FFD8)。逐圈手推整个 ROM
的毛刺增量表后锁定缺的那次:第 29 圈的 **enable 写**落在计数器 2047(bit9 即将下降)——真机的
写落在 **T3**(该 M-cycle 最后一拍**之前**),使能后的输入亲历 2047→2048 的下降沿 → TIMA 增量;
我们写落 T4(计数器已 2048),错过。补上后第 16 次增量提前到第 37 圈的 disable → 恰好 FFD9。

修法:**不动全局写落点**(T3 全局写实测回归 `call/push/rst/call_cc2` 四个,反证总线写就在 T4),
在 `Timer::write_register(FF07)` 里**重放硬件顺序**——记录每拍前的 `prev_counter`,把写按"落在
上一拍计数器、再以新 TAC 重算最后一拍边沿"计算,与已跑过的"T4-老 TAC"边沿对账补差。相位扫描
实验(0/±1/… 都无法同时满足 rapid_toggle 与 tim*)正是佐证:普通增量相位本来就对,只有
**TAC 写的观测点**要提前一拍。timer 组 13/13。

坑:重放最初用 `counter-1` 反推"上一拍",冷调用(单测直接 `write_register`、无先行 tick)时
回绕出 0xFFFF 幻影高位 → 两个 timer 单测挂;改为**记录真实 prev_counter** 后干净。

### 3.3 STAT mode 读数的常量拍偏移(intr_2 一簇)—— 已修
一段"证伪 → 定位 → 修复"的完整过程,值得记下方法:

1. `ppu_probe` 实测 PPU 的 mode2-int / mode3 / mode0 落点是教科书值 **dot 0 / 80 / 252**,
   **PPU 内部时序没偏**。
2. 曾假设要靠**整颗 CPU 逐 T 步进**修。**实测证伪**:完整实现了 cycle-stepped 核(`tick_t` 逐
   `bus.tick(1)` + timer 守卫钩子 + T-cycle HALT 唤醒),结果对运行态零变化、`intr_2` 仍全挂、
   且 T-cycle HALT 唤醒还回归了 `hblank`。**结论:CPU 结构不是瓶颈。**
3. 关键观察:`intr_2_0_timing`(测 mode2 中断本身)**过**,而 `intr_2_mode0/mode3/oam_ok`
   (测从 mode2 中断到"STAT 读到 modeX / OAM 可访问")**挂**。所以偏移在**软件观测到 mode 的
   时刻**,不在中断、不在内部转换。`hblank`(相对测量)对常量偏移免疫,故一直过。
4. **修复**:真机上 STAT 寄存器的 mode 位、以及 OAM/VRAM 锁,都比内部 mode 转换**晚约 4 dot**
   (出 Drawing 只晚 1 dot,见 §3.5)。加 `prev_mode` / `transition_age`,用 `visible_mode()`
   供 STAT 读和 OAM 访问判定——`intr_2_mode0/mode3/oam_ok` 全绿,acid2 字节不变,无回归。

同簇一并修好的还有:**`hblank_ly_scx` + `intr_2_0`**——此前 mode 3 只有 167 dot(裸 FIFO warmup),
真机 172;补上**固定取数启动 stall** 后 mode 3 = 172 + (SCX&7) + 精灵/窗口惩罚,HBlank 起点归位
(像素不变)。**`vblank_stat_intr`**——第 144 行进 VBlank 时同时触发 mode 2 的 STAT 中断,把 144 行
并入 STAT 线条件。**`stat_lyc_onoff`**——三件事:①开屏首行走 mode 0(不扫 OAM),故开屏首拍 STAT
读 mode 0;②关屏时 scan 停,只有**冻结的 LYC 一致位**能撑起 STAT 线,故关屏保留
`stat_line = 一致位 & bit6`,再开屏才是真上升沿才触发;③开屏/写 LYC 造成上升沿时**立即**触发
STAT 中断(赶在下一条 `DI` 之前)。

教训:别急着上大重写;先用 probe/trace 把偏移**量化**,常常是外设的一个常量拍。

### 3.4 `lcdon_timing-GS` + `lcdon_write_timing-GS` —— 已修:开屏首行逐点时序
本项目目前最完整的一次"从测试源码解码期望"标定:两测试各 3 个 pass 错开 1 nop(=4T),给出
**4-dot 分辨率**的 157 个期望值;把 `.s` 期望表逐值解码后反推出自洽模型,再用 `inspect dumpat`
直接 dump 测试自己的结果缓冲对表,一次全中:

- **开屏首行(line 0)**:行长 **452**(LY=1 落在 dot 452);无 OAM scan(假 mode 0),画点从
  dot 80 起;且该行的 mode 转换**无可见滞后**(internal==visible,仅此行)。
- **OAM/VRAM 锁不对称**:**读锁在内部沿上锁**(scan/fetch 一开始就占总线)、**在可见沿解锁**;
  **写只看可见 mode**——由此自然涌现行首 dot 0..3 与 mode2→3 交接 dot 80..83 的**写穿窗**
  (硬件实测行为,期望表钉死)。
- **LYC match 行首消隐**:LY 变化后头 4 dot 读 0,dot 4 重新锁存比较结果。

### 3.5 `intr_2_mode0_timing_sprites` —— 已修:惩罚聚合 + fetcher 气泡 + 可见滞后
一条测试钉死三处缺陷。它的 ~100 个 testcase 各编码 `floor(总惩罚/4)`,逐条验算后反推出:

1. **惩罚聚合**:同一 **OAM X** 只有第一个精灵付 BG-fetch 中止费(`11 - min(5,(x+SCX)%8)`,
   X=0 恒 11),后续同 X 精灵只付 6-dot 取数(10 个叠 X=0 = 5+6×10=65,不是 110)。键控必须用
   OAM X 而非触发像素:X=0 与 X=8 都在像素 0 触发,却各付全额。
2. **停摆确定性**:精灵停摆期间 BG fetcher **继续运行**(惩罚公式已包含其损失),mode 3 恰好
   延长惩罚本身;旧的"冻结 fetcher"模型会漏 1–2 dot 的涌现气泡。
3. **Push 逐拍重试**:fetcher 末段 push 无需访存、每 dot 重试(奇数惩罚 11/7 曾留下 1-dot 奇偶
   气泡);基线 warmup 相应 5→6,无精灵行保持 172。
4. **Drawing→HBlank 的 STAT 可见滞后是 1 dot 而非 4**:此测试的奇数惩罚打破了其它所有测试留下
   的模 4 采样简并(其余测试对该滞后模 4 不敏感,4 是巧合解)。

之前"测量行 LY=68 无精灵"的结论是**误读**:精灵 Y=$52 覆盖屏幕行 66..73,line 68 在内。
`ppu_probe` 12 种配置(单个/叠加/散开)全部逐 dot 命中硬件表。

### 3.6 mealybug tearoom —— 已修 WX<7 裁剪;其余逐一定性
`tools/mealybug_compare.py` 把"0/24 凭感觉"换成**逐测试相似度 + 差异结构**(见 §2.4)。

**已修**(`fix(ppu): clip the window's left edge when WX < 7`):`m3_wx_5_change` 的行样显示我们的
输出正是参考**右移 (7−WX) 像素**。真机 WX<7 时窗口左边 (7−WX) 像素落屏外被裁,我们没裁 → 整窗
右移。复用 SCX fine-scroll 的 discard 机制,窗口激活时置 `discard = 7 - WX`。`m3_wx_4` 56→99、
`m3_wx_5` 59→97(逐像素对齐)、`m3_window_timing_wx_0` 96→99;acid2 字节不变、mooneye ppu 全绿、
`m2_win_en_toggle` 保持 100%。

**剩余的定性**(每个都是独立逐-dot 时序,非"改几行",且碰渲染有回归风险):
- **调色板写延迟 + 瞬态**(`m3_bgp_change` 78 / `_sprites` 75):用 mode-2 STAT 中断触发,按 nop
  延迟连写 BGP 扫描整行。我们的转变比参考**晚 ~7px**,且参考在写落地那拍有**孤立的中间色像素**
  (DMG 著名的 BGP 写-推同拍毛刺)我们不建模。OBJ 版 `m3_obp0_change` 已 98%,BG/OBJ 路径这点
  差异是硬件特性。该 78% **非回归**(相关改动前后同分)。
- **窗口触发的精确 dot**(`m3_wx_6_change` 40 / `win_en_multiple` 64):某些行参考显示背景、我们
  显示窗口——WX 在触发比较那一拍被改写时应**抑制**触发,缺该逐-dot 比较时机。
- **coarse-scroll 采样点**(`m3_scx_high_5_bits` 99.64,单个 tile 列 x16-23):SCX 高位中途改,差
  一个 tile 的取数时机。
- **精灵-窗口边缘 1 像素**(`m3_wx_4_change_sprites` 99.96,仅 10 px):WX=4 窗口边缘透出的单个
  精灵像素被我们丢弃。
- **SCY 中途改**(`m3_scy_change` 58):行内改 SCY 影响取哪一 tile 行,大面积偏。

### 3.7 实验教训(已回退)
几次朝 T-cycle 精度的尝试被棘轮挡回,记录以免重蹈:
- **DMA 逐字节冲突读**:曾以为 DMA 期间读外部总线返回"在飞字节";实测**回归了
  `oam_dma_start/restart/timing`**,证明 DMG 读到的是 `$FF` 开路总线,已回退。
- **T-cycle HALT 唤醒**(逐 T 轮询):既没修好 intr_2,又**回归了一个 halt 时序测试**(HALT
  变长,破坏了别处周期计数)。说明中断路径要整体改,不能只改唤醒。
- **lcdon 首行早期建模**(mode 3 落在 dot 82):dot 偏移猜错,且改动破坏了本地 `ppu_test` 对
  "开屏即 mode 2"的假设——后来 §3.4 用期望表重做才对。
- **写落点改 T3**(tick3-写-tick1):想同时啃 rapid_toggle / lcdon_write,结果**回归
  `call/push/rst/call_cc2` 四个**、rapid_toggle 也没修。反证 SM83 的写就落在 **T4**,现有
  "tick(4) 后写"是对的,别再动(rapid_toggle 改在 timer 内部重放,见 §3.2)。

### 3.8 Boot 状态(不在目标范围)
`boot_regs`/`boot_div`/`boot_hwio` 的 `dmg0/mgb/sgb/sgb2` 变体、`boot_sclk_align` 校验特定机型
开机态;我们只做 DMG 且不跑真实 boot ROM。真正的 DMG 变体(`*-dmgABC`)已过。

### M-cycle 级 ↔ 亚周期级 光谱

```
已过(含定向标定拿下的)              仍挂
──────────────────────────────────┼────────────────────────────────────────────
Blargg cpu_instrs/instr/mem_timing  boot_*(需真实 boot ROM / 其它机型,不在范围)
dmg-acid2                           boot_sclk_align(同上)
mooneye: bits/instr/interrupts      mealybug(23/24;mode-3 行内特效的具体锁存 dot)
oam_dma 全组;控制流读/写时序;ie_push
timer 全组(含 rapid_toggle:TAC 写 T3 重放)
PPU 全组:mode-3 长度(172+惩罚聚合)
  hblank_ly_scx / vblank_stat_intr
  stat_lyc_onoff / intr_2 全簇
  lcdon_*(开屏首行 452 dot + 锁不对称)
  OBJ 惩罚聚合(首个付全额、同 X 后续付 6)
  mealybug m2_win_en_toggle(1/24)
STAT mode 滞后:出 scan/blank 4 dot、出 Drawing 1 dot
```

> **T-cycle 迁移状态**:CPU 时间推进已收敛到单一 T-cycle seam(`Cpu::tick_t`)。实测 CPU 的访存
> 本就落在 M-cycle 内正确的末拍(T4,故 Blargg `mem_timing` 过、T3 全局写会回归),所以 CPU 的
> **访存**已 T-accurate;剩余亚周期落点(rapid_toggle 的 TAC 写、mealybug 的锁存 dot)按需在
> 对应外设内局部建模,而非整颗 CPU 逐 T 步进。

---

## 4. 复现

```sh
# 1) 取测试 ROM(不随仓库分发)
#    https://github.com/c-sp/game-boy-test-roms/releases  (本项目用 v7.0)
export GB_TEST_ROMS=/path/to/game-boy-test-roms

# 2) 模块单测(灰盒,无需 ROM)
cargo test --release

# 3) ROM 集成套件(mooneye 非 boot 全过 + Blargg)
cargo test --release --test rom_suite

# 4) mealybug 相似度记分板
tools/mealybug_compare.py            # 全 24 个;可加 name-substr 只跑一撮

# 5) 单个 ROM 手动看
cargo run --release --example run_serial  -- "$GB_TEST_ROMS/blargg/cpu_instrs/cpu_instrs.gb"
cargo run --release --example run_mooneye -- "$GB_TEST_ROMS/mooneye-test-suite/acceptance/timer/tima_reload.gb"
```

> `MEALYBUG_DUMP_FB` 可覆盖脚手架里 dump 帧缓冲的命令(适配需直连工具链二进制的环境)。
