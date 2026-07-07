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

### 2.3 mooneye acceptance —— 40 / 75

| 分组 | 成绩 | 备注 |
|---|---|---|
| bits | 3/3 ✅ | |
| instr | 1/1 ✅ | |
| interrupts | 1/1 ✅ | `ie_push` 已修(压栈盖 IE 重算向量) |
| timer | 12/13 | 仅剩 `rapid_toggle` |
| oam_dma | 1/3 | 挂 `reg_read`、`sources-GS` |
| ppu | 2/12 | 挂全部 `intr_2_mode*`、`stat_lyc_onoff`、`lcdon_*` 等 |
| serial | 0/1 | `boot_sclk_align`(需 boot 时序) |
| root | 20/41 | 拆解见下 |

root 组 21 个失败按性质分三类:

1. **Boot 状态(11 个,基本不在目标范围)**:`boot_regs-{dmg0,mgb,sgb,sgb2}`、
   `boot_div*`、`boot_hwio*`。校验的是**特定机型**开机后的寄存器/IO 状态,我们只做
   DMG、也不跑真实 boot ROM。注:`boot_regs-dmgABC`(真正的 DMG)**已通过**。
2. **控制流读/内部时序(9 个)**:`call_timing`、`call_cc_timing`、`jp_timing`、
   `jp_cc_timing`、`ret_timing`、`ret_cc_timing`、`reti_timing`、`add_sp_e_timing`、
   `ld_hl_sp_e_timing`。见 §3。
3. **`ei_sequence`(1 个)**:EI 生效时机的边界。

本轮已修并转绿的控制流**写**时序测试(在 root 组内):`rst_timing`、`push_timing`、
`call_timing2`、`call_cc_timing2`。

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

### 3.1 逐字节 OAM DMA 总线冲突读(阻挡 `call/ret/reti/jp` 等 timing)
mooneye 这些 timing 测试用 **OAM DMA 当示波器**:把栈指进 OAM、或让操作数从外部总线
读取,再用 DMA 窗口卡边界。**写**方向已经修好(见
[`fix(bus): cycle-accurate OAM DMA start delay and bus blocking`](../src/console/bus.rs)):
启动延迟 = 1 个空转 M-cycle(M=1 仍可访问 OAM、M=2 才 block),窗口内对外部总线/OAM
的写被丢弃——`rst/push/call2/call_cc2` 因此转绿。

**读**方向没到位。真机在 DMA 期间从外部总线读到的是"**DMA 当前正在搬运的那个字节**",
而我们是**一次性原子拷贝**、阻挡读一律返回 `$FF`,没有"当前在飞字节"的概念。dump 寄存器
可见这几个测试不是"边界差一拍",而是喂错数据后**整个跑飞**(`jp_timing` 与 `call_timing`
结束态完全一致地卡死)。要修得建**逐字节、按 T-cycle 推进的 DMA 总线模型**,且哪个字节、
差几拍都要拿 ROM 反复标定——已滑出"小而可控"范围。
(`oam_dma_timing` 能过,只是因为它的源恰好是 `$FF`。)

### 3.2 `rapid_toggle`(timer 亚周期)
timer 内部本就逐 T-cycle。差的是 **CPU 的写在 M-cycle 内哪一拍提交**:紧凑循环里连写
TAC,毛刺增量取决于写落地那刻计数器选中位是 0 还是 1;压在位翻转边界上,差 1–2 个
T-cycle 结果就差一次。杠杆在"总线写的精确 T 位置",不在 timer 本身。

### 3.3 PPU 逐点时序(mooneye `ppu` 组 + mealybug mode-3)
`intr_2_mode*`、`stat_lyc_onoff`、`lcdon_*`、mealybug 的 mode-3 类,测的是**模式在哪个
dot 翻转、寄存器在哪个 dot 被锁存**。PPU 已逐点渲染,但这些锁存/翻转点的精确 dot 还没
标定到位,属 PPU 侧的 T-cycle 前沿。

### 3.4 `ei_sequence`
EI 使能的延迟边界(EI 后一条指令才置 IME,与中断轮询点的精确对齐)。可 M-cycle 级
表示,但需要细扣轮询时机,尚未验证。

### 3.5 `oam_dma/reg_read`、`oam_dma/sources-GS`
DMA 寄存器回读值、以及从不同源地址区(含冲突区)启动 DMA 的细节;与 §3.1 的总线冲突
模型相关。

### 3.6 Boot 状态(不在目标范围)
`boot_regs`/`boot_div`/`boot_hwio` 的 `dmg0/mgb/sgb/sgb2` 变体校验特定机型开机态;我们
只做 DMG 且不跑真实 boot ROM。DMG 变体(`*-dmgABC`)已过。

### M-cycle 级 ↔ 亚周期级 光谱

```
M-cycle 级(已过 / 可干净修)        亚 M-cycle / T-cycle 级(标定密集、有回归风险)
──────────────────────────────────┼────────────────────────────────────────────
Blargg cpu_instrs/instr/mem_timing  逐字节 DMA 总线冲突读(call/ret/reti/jp)
dmg-acid2                           rapid_toggle(总线写的 T 位置)
mooneye: bits/instr/interrupts      PPU intr_2_mode* / stat / lcdon
timer(除 rapid_toggle)             mealybug mode-3(取数/寄存器锁存点)
oam_dma_start/restart/timing        ei_sequence(轮询点对齐)
rst/push/call2 等写时序
```

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
