# 测试方法、用例情况与遗留问题

本文记录 rgametoy 的验证方式、当前测试通过情况,以及尚未攻克的遗留问题。
目标始终是"尽量贴近真实硬件结构 + T-cycle 精度"(见 [AGENTS.md](../AGENTS.md)),
因此除了常规单元测试,主要靠社区标准**测试 ROM** 来度量精度。

---

## 1. 测试方法

测试分两层:

### 1.1 单元测试(`cargo test`)

`tests/` 下按子系统组织,直接对 core(`console/`)的各模块做白盒断言,不依赖外部 ROM:

| 文件 | 覆盖 |
|---|---|
| `cpu_instructions_test.rs` / `cpu_integration_test.rs` / `registers_test.rs` | 指令语义、标志位、寄存器 |
| `ppu_test.rs` | 时序里程碑、逐行/行内渲染(含 BGP 行内改色分割) |
| `timer_test.rs` | 16 位计数器、下降沿、重载延迟 |
| `dma_test.rs` | OAM DMA 启动延迟与总线阻塞窗口 |
| `apu_test.rs` | 四声道、包络/扫频/长度、DAC |
| `cartridge_test.rs` / `save_test.rs` / `savestate_test.rs` | MBC、电池存档、即时存档 |
| `serial_test.rs` / `rom_render_test.rs` | 串口截获、整帧渲染 |

```sh
cargo test --release
```

### 1.2 测试 ROM 验证

用业界公认的测试 ROM 度量精度。ROM **不随仓库分发**(体积 + 版权),从
[`c-sp/game-boy-test-roms`](https://github.com/c-sp/game-boy-test-roms) 的
release bundle(本项目用 v7.0)获取。每种 ROM 的"通过信号"不同,对应一个无头 runner:

| 套件 | 判定方式 | Runner |
|---|---|---|
| **Blargg** | 把结果("Passed"/"Failed")打到**串口**,截获比对 | `examples/run_serial.rs` |
| **dmg-acid2** | 渲染一张参考图,**肉眼/像素**比对 | `examples/screenshot.rs`(转 BMP) |
| **mooneye** | 成功时把斐波那契签名 `3,5,8,13,21,34` 载入 `B,C,D,E,H,L`,失败为其它值 | `examples/run_mooneye.rs` |
| **mealybug** | 在**某一特定帧**产出画面,与参考 PNG(`*_dmg_blob.png`)**逐像素**比对 | `examples/dump_fb.rs` + 比对脚本 |

```sh
cargo run --release --example run_serial  -- path/to/blargg.gb      # 打印串口输出
cargo run --release --example run_mooneye -- path/to/mooneye.gb     # 打印 PASS / FAIL
cargo run --release --example dump_fb     -- rom.gb out.raw [帧数]  # dump 160x144 灰度字节
cargo run --release --example screenshot  -- rom.gb out.bmp         # 无头渲染一帧
```

几个判定细节:
- **mooneye** 的斐波那契签名是所有硬件测试通用的"通过"约定;`run_mooneye` 跑
  240 帧后读寄存器判定。它**只给 PASS/FAIL**,调试具体差异要另 dump 全寄存器。
- **mealybug** 的效果只在**一帧**上出现:CPU 做完一串按拍对齐的行内写入后执行
  `ld b,b`(断点)就停,之后的帧会**不带效果**重绘。所以要扫描帧号找到"测试帧",
  再和参考图逐像素比;参考 PNG 是 4 级灰度,按 `round((255-gray)/85)` 映射到我们的
  0–3 shade。100% 命中才算 PASS。

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

### 2.3 mooneye acceptance —— 46 / 75

| 分组 | 成绩 | 备注 |
|---|---|---|
| bits | 3/3 ✅ | |
| instr | 1/1 ✅ | |
| interrupts | 1/1 ✅ | `ie_push` 已修(压栈盖 IE 重算向量) |
| oam_dma | 3/3 ✅ | `reg_read`(DMA 期间 I/O 仍可读)、`sources-GS`(源 E0-FF 读 WRAM 回声)已修 |
| timer | 12/13 | 仅剩 `rapid_toggle` |
| ppu | 5/12 | 已修 `hblank_ly_scx`、`intr_2_0`、`vblank_stat_intr`;挂 `intr_2_mode0/mode3/oam_ok`、`lcdon_*`、`stat_lyc_onoff` |
| serial | 0/1 | `boot_sclk_align`(需 boot 时序) |
| root | 21/41 | 拆解见下 |

root 组 20 个失败按性质分两类:

1. **Boot 状态(11 个,基本不在目标范围)**:`boot_regs-{dmg0,mgb,sgb,sgb2}`、
   `boot_div*`、`boot_hwio*`。校验的是**特定机型**开机后的寄存器/IO 状态,我们只做
   DMG、也不跑真实 boot ROM。注:`boot_regs-dmgABC`(真正的 DMG)**已通过**。
2. **控制流读/内部时序(9 个)**:`call_timing`、`call_cc_timing`、`jp_timing`、
   `jp_cc_timing`、`ret_timing`、`ret_cc_timing`、`reti_timing`、`add_sp_e_timing`、
   `ld_hl_sp_e_timing`。见 §3。

本轮已修并转绿(root 组内):控制流**写**时序 `rst_timing`、`push_timing`、
`call_timing2`、`call_cc_timing2`,以及 `ei_sequence`(EI 一指令延迟在连续 EI 下的正确性)。

### 2.4 mealybug tearoom(DMG)—— 0 / 24

全挂,但命中率说明问题:多数在 **90–99%**(如 `m3_lcdc_obj_en_change` 99%、
`m3_obp0_change` 98%、`m2_win_en_toggle` 99%)——像素 FIFO 把每个行内特效的**结构**
渲染对了,只是没精确到"具体哪个 dot";少数偏差大(`m3_wx_4_change` 34%、
`m3_scy_change` 43%),指向具体的窗口/滚动锁存边界。mealybug 是最严的 PPU 时序测试,
0/24 对不少成熟模拟器也是常态。

---

## 3. 遗留问题

一句话:凡是 **M-cycle 粒度**的都过了(Blargg、dmg-acid2、大部分 mooneye);凡是要求
**T-cycle / 逐点精确**落点的,还没到位。这些不是"改几行"能干净修的,普遍**标定密集、
有回归风险**,故单独列出。

### 3.1 控制流读时序(`call/ret/reti/jp/add_sp_e/ld_hl_sp_e`)
mooneye 这些 timing 测试用 **OAM DMA 当示波器**:把栈指进 OAM、或让操作数从外部总线
读取,再用 DMA 窗口卡边界。**写**方向已经修好(见
[`fix(bus): cycle-accurate OAM DMA start delay and bus blocking`](../src/console/bus.rs)):
启动延迟 = 1 个空转 M-cycle(M=1 仍可访问 OAM、M=2 才 block),窗口内对外部总线/OAM
的写被丢弃——`rst/push/call2/call_cc2` 因此转绿。

**读**方向仍没到位,但原因和先前的猜测**相反**:曾假设 DMA 期间读外部总线应返回"DMA
当前在搬运的字节"。实测把它建成**逐字节冲突读**后,`oam_dma_start/restart/timing` 立刻
回归——**证明 DMG 在 DMA 期间读到的就是 `$FF`(开路总线),不是源字节**,该改动已回退。
所以这簇失败是更细的**读边界/采样拍**问题(dump 寄存器可见整个跑飞),暂未定位到可干净修
的根因。

### 3.2 `rapid_toggle`(timer 亚周期)
timer 内部本就逐 T-cycle。差的是 **CPU 的写在 M-cycle 内哪一拍提交**:紧凑循环里连写
TAC,毛刺增量取决于写落地那刻计数器选中位是 0 还是 1;压在位翻转边界上,差 1–2 个
T-cycle 结果就差一次。杠杆在"总线写的精确 T 位置",不在 timer 本身。

### 3.3 PPU 组(mooneye `ppu` 7 挂 + mealybug mode-3)
STAT 中断的**边沿检测**本就正确(`ppu.rs` 的 `update_stat_line`)。本轮又修好三个:

- **`hblank_ly_scx_timing` + `intr_2_0_timing`**:此前 mode 3 长度只有 167 dot(裸 FIFO
  warmup),真机是 172。补上**固定 5-dot 取数启动 stall**后,mode 3 = 172 + (SCX&7) +
  精灵/窗口惩罚,HBlank 起点归位。像素不变(acid2 字节稳定),只是产出的 dot 时刻对齐。
- **`vblank_stat_intr`**:第 144 行进 VBlank 时同时触发 mode 2/OAM 的 STAT 中断——把
  144 行并入 STAT 线条件即可。

仍挂 7 个,都是**逐点(dot-precise)时序**:`intr_2_mode0/mode3/oam_ok_timing`、
`intr_2_mode0_timing_sprites`、`lcdon_timing`、`lcdon_write_timing`、`stat_lyc_onoff`。
- **intr_2 一簇**测"从 mode2 中断到 modeX 的精确 M-cycle 数"。我们的 mode2-int/mode3/
  mode0 落点已是教科书值(dot 0 / 80 / 252)。根因是 **HALT 唤醒是 M-cycle 粒度**:PPU 在
  dot 0 置 IF,而 halted 的 CPU 每步 tick(4)、在下一步边界才轮询,唤醒抖动 0–3 dot。这需要
  **整体的 T-cycle 中断路径**(唤醒 + 派发 + 采样一起),不是点修——见 §3.6 的实验教训。
- **`stat_lyc_onoff`**:已修**关屏时 LY=LYC 比较位冻结**(见 `fix(ppu): freeze LY==LYC`),
  还差开屏那一拍精确触发 STAT 中断(要给写入路径加中断管线)。
- **`lcdon_*`**:开屏首行 line 0 从 mode 0 直接进 mode 3(跳过 mode 2)、且 PPU 晚 2 T——
  需要建首帧特殊时序,精确偏移待标定。

### 3.4 实验教训(已回退)
几次朝 T-cycle 精度的尝试被棘轮挡回,记录以免重蹈:
- **DMA 逐字节冲突读**:曾以为 DMA 期间读外部总线返回"在飞字节";实测**回归了
  `oam_dma_start/restart/timing`**,证明 DMG 读到的是 `$FF` 开路总线,已回退。
- **T-cycle HALT 唤醒**(逐 T 轮询):既没修好 intr_2,又**回归了一个 halt 时序测试**
  (HALT 变成变长,破坏了别处的周期计数)。说明中断路径要整体改,不能只改唤醒。
- **lcdon 首行建模**(mode 3 落在 dot 82):dot 偏移猜错,且改动破坏了本地 `ppu_test` 对
  "开屏即 mode 2"的假设。首行特殊时序要连本地测试一起重做。

### 3.5 Boot 状态(不在目标范围)
`boot_regs`/`boot_div`/`boot_hwio` 的 `dmg0/mgb/sgb/sgb2` 变体校验特定机型开机态;我们
只做 DMG 且不跑真实 boot ROM。DMG 变体(`*-dmgABC`)已过。

### M-cycle 级 ↔ 亚周期级 光谱

```
M-cycle 级(已过 / 可干净修)        亚 M-cycle / T-cycle 级(标定密集、有回归风险)
──────────────────────────────────┼────────────────────────────────────────────
Blargg cpu_instrs/instr/mem_timing  控制流读时序(call/ret/reti/jp)——读边界/采样拍
dmg-acid2                           rapid_toggle(总线写的 T 位置)
mooneye: bits/instr/interrupts      PPU intr_2_mode* / lcdon / stat_lyc_onoff
timer(除 rapid_toggle)             mealybug mode-3(取数/寄存器锁存点)
oam_dma 全组(start/restart/timing
  /reg_read/sources)
rst/push/call2 写时序
ei_sequence;PPU mode-3 长度
PPU hblank_ly_scx / vblank_stat_intr
```

> **T-cycle 迁移状态**:CPU 时间推进已收敛到单一 T-cycle seam(`Cpu::tick_t`,见
> `refactor(cpu): T-cycle tick primitive`)。实测 CPU 的访存本就落在 M-cycle 内正确的
> 末拍(≈T4,故 Blargg `mem_timing` 过),所以 CPU 的**访存**已 T-accurate。

### 3.6 已量化的根因:中断观测的相位量化(intr_2 一簇)
用 `ppu_probe` 实测:PPU 的 mode2-int / mode3 / mode0 落点是教科书值 **dot 0 / 80 / 252**
(= mode2→mode0 恰好 63 M-cycle),**PPU 侧没有偏移**。用一次性 trace 跑真 ROM 实测
`intr_2_mode0_timing` 的 handler→mode0 跨度,两轮之间**抖动 4 T-cycle(整整 1 个 M-cycle)**,
而真机是确定值——这就是失败原因。

曾假设根因是"指令按 M-cycle 成块推进、观测相位量化",要靠**整颗 CPU 逐 T 步进**来修。
**已实测证伪。** 我完整实现了 cycle-stepped 核(`tick_t` 逐 `bus.tick(1)` 推进 + 把 timer
`just_reloaded` 守卫改成 `begin_m_cycle` 钩子 + T-cycle HALT 唤醒),跑全套 baseline diff:

- 对**运行态是零变化**(除 hblank 外其余 44 项完全不动),证明 M-cycle 成块推进 ≡ 逐 T 推进;
- `intr_2_*` **仍然全挂**——cycle-stepped 核**没修好它**;
- T-cycle HALT 唤醒**回归了 `hblank_ly_scx`**(净 46→45),说明现有的 **M-cycle HALT 唤醒
  其实是对的**(`hblank` / `halt_*` 都靠它过)。

**修正结论:M-cycle vs T-cycle 的 CPU 结构不是 `intr_2` 的瓶颈,大重写也救不了它。** PPU 侧的
mode 落点是教科书值,`hblank`(相对测量)又过,所以差的是 **STAT 中断 / mode 寄存器读数的某个
常量拍偏移**(常量差会被 `hblank` 的相对测量抵消,却会让 `intr_2` 的绝对测量挂)。要修需要
**精确的硬件参考拍值**来标定这个偏移(如 Gekkio gb-ctr 的 STAT 时序表),属定向标定、非架构改动。

---

## 4. 复现

```sh
# 1) 取测试 ROM(不随仓库分发)
#    https://github.com/c-sp/game-boy-test-roms/releases  (本项目用 v7.0)

# 2) 单元测试
cargo test --release

# 3) 各套件(示例)
cargo run --release --example run_serial  -- blargg/cpu_instrs/cpu_instrs.gb
cargo run --release --example run_mooneye -- mooneye-test-suite/acceptance/timer/tima_reload.gb
cargo run --release --example dump_fb     -- mealybug-tearoom-tests/ppu/m3_bgp_change.gb out.raw 5
```

> mooneye/mealybug 的成绩单可用脚本对整个 `acceptance/`、`ppu/` 目录批量跑;
> 判定规则见 §1.2。
