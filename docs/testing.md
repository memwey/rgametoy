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

**调试器(`--features debug`)**:标定硬件测试用的项目内检查器(见 `src/console/debug.rs`),
`Console::snapshot()` 一次性拿到整机可观测状态(含寄存器看不到的 PPU 内部 mode/dot、STAT 线、
LY==LYC 锁存),`run_until` 打断点。`examples/inspect.rs` 是它的 CLI:

```sh
cargo run --release --features debug --example inspect -- rom.gb break 0x48   # 跑到 PC 后打全快照
cargo run --release --features debug --example inspect -- rom.gb watch 0x48 6 # 每次命中 PC 打快照
cargo run --release --features debug --example inspect -- rom.gb line 0       # 某扫描线的 mode/dot 变化
```
`stat_lyc_onoff` 就是靠它几步定位 patch 未生效 + STAT 线冻结丢失两个 bug 后修好的。

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

### 2.3 mooneye acceptance —— 59 / 75

| 分组 | 成绩 | 备注 |
|---|---|---|
| bits | 3/3 ✅ | |
| instr | 1/1 ✅ | |
| interrupts | 1/1 ✅ | `ie_push` 已修(压栈盖 IE 重算向量) |
| oam_dma | 3/3 ✅ | `reg_read`、`sources-GS` 已修 |
| timer | 12/13 | 仅剩 `rapid_toggle` |
| ppu | 9/12 | 已修 `hblank_ly_scx`、`intr_2_0/mode0/mode3/oam_ok`、`vblank_stat_intr`、`stat_lyc_onoff`;挂 `intr_2_mode0_sprites`、`lcdon_*` |
| serial | 0/1 | `boot_sclk_align`(需 boot 时序) |
| root | 30/41 | 只剩 boot 类(见下) |

root 组仅剩 11 个失败,**全是 Boot 状态**:`boot_regs-{dmg0,mgb,sgb,sgb2}`、`boot_div*`、
`boot_hwio*`,校验**特定机型**开机后的寄存器/IO 状态——我们只做 DMG、也不跑真实 boot ROM,
故不在目标范围(`boot_regs-dmgABC` 真正的 DMG 已过)。

本轮已修并转绿(root 组内):控制流**写**时序 `rst/push/call2/call_cc2`、`ei_sequence`,
以及控制流**读**时序整簇 `call/call_cc/jp/jp_cc/ret/ret_cc/reti/add_sp_e/ld_hl_sp_e _timing`
(见 §3.1)。

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

### 3.1 控制流读时序 —— 已修(源总线相关的 DMA 阻塞)
mooneye 这些 timing 测试用 **OAM DMA 当示波器**:把栈指进 OAM、或让指令从 ROM 取指,
再用 DMA 窗口卡边界。**写**方向早先修好(启动延迟 + 窗口内写丢弃)。

**读**方向的根因用 trace + 反汇编定位到了:`ret_timing` 的 RET 在 **ROM**(0x0192),DMA
源是 **$80(VRAM)**。真机上 **DMA 只占用与源冲突的那条总线**:VRAM 源占**视频总线**
(VRAM+OAM),CPU 仍可读**外部总线**(ROM)——所以 RET 取指成功,只有 OAM 的 pop 被挡。
我们之前**无脑阻塞 `addr < 0xFEA0`**(所有源一样),把 ROM 取指也挡了 → 读到 `$FF` = RST 38
→ 指令跑飞。改成按源分总线阻塞(`dma_conflicts`,仅 OAM 恒锁)后,**整簇 9 个读时序全绿**,
oam_dma 组不回归(见 `fix(bus): OAM DMA blocks only the bus that conflicts with its source`)。

### 3.2 `rapid_toggle`(timer 亚周期)
timer 内部本就逐 T-cycle。差的是 **CPU 的写在 M-cycle 内哪一拍提交**:紧凑循环里连写
TAC,毛刺增量取决于写落地那刻计数器选中位是 0 还是 1;压在位翻转边界上,差 1–2 个
T-cycle 结果就差一次。杠杆在"总线写的精确 T 位置",不在 timer 本身。

### 3.3 PPU 组(mooneye `ppu` 4 挂 + mealybug mode-3)
STAT 中断的**边沿检测**本就正确(`ppu.rs` 的 `update_stat_line`)。本轮修好五个:

- **`hblank_ly_scx_timing` + `intr_2_0_timing`**:此前 mode 3 长度只有 167 dot(裸 FIFO
  warmup),真机是 172。补上**固定 5-dot 取数启动 stall**后,mode 3 = 172 + (SCX&7) +
  精灵/窗口惩罚,HBlank 起点归位。像素不变(acid2 字节稳定),只是产出的 dot 时刻对齐。
- **`vblank_stat_intr`**:第 144 行进 VBlank 时同时触发 mode 2/OAM 的 STAT 中断——把
  144 行并入 STAT 线条件即可。
- **`intr_2_mode0/mode3/oam_ok_timing`**:见 §3.6——找到并修好了那个常量拍偏移。
- **`stat_lyc_onoff`**:用**项目内的 `inspect` 调试器**(见 `feat(debug)`)一步步定位后修好——
  三件事一起:①**开屏首行 line 0 走 mode 0**(不扫 OAM),故开屏首拍 STAT 读 mode 0;②关屏时
  scan 停,只有**冻结的 LYC 一致位**能撑起 STAT 线,所以关屏保留 `stat_line = 一致位 & bit6`,
  再开屏才是"真上升沿"才触发;③开屏/写 LYC 造成上升沿时**立即**触发 STAT 中断(赶在下一条
  `DI` 之前,tick 路径会晚一两拍)。inspect 直接看到 `lyc_m/stat_l`,几步就锁定了 patch 未生效 +
  冻结丢失两个 bug。

仍挂 2 个(已精确定位机制,缺参考拍/算法):

- **`lcdon_*`**:trace 证实真机 line 0 的 STAT 先读 **mode 0** 再进 mode 3(`enable→mode 0` 已就位);
  但**扫 delay=0..6 都不过**,说明不止 mode 3 起点——还牵涉 mode 3 长度 / OAM·VRAM 锁的逐点时序,
  缺该测试的精确参考拍值。
- **`intr_2_mode0_timing_sprites`**:非精灵版已过,只差**精灵的 OBJ mode-3 惩罚**精确(现为固定
  6 dot 近似)。需要 Pan Docs 的 OBJ penalty 算法(按 (x+SCX)%8 + 取数状态算),且改动碰渲染、
  有 acid2/mealybug 回归风险。

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
已过(含定向标定拿下的)              仍挂(标定密集、有回归风险)
──────────────────────────────────┼────────────────────────────────────────────
Blargg cpu_instrs/instr/mem_timing  rapid_toggle(总线写的 T 位置)
dmg-acid2                           intr_2_mode0_sprites(OBJ mode-3 惩罚)
mooneye: bits/instr/interrupts      lcdon_*(开屏首行逐点时序)
oam_dma 全组;控制流读/写时序          mealybug mode-3(取数/寄存器锁存点)
timer(除 rapid_toggle);ei_sequence
PPU mode-3 长度 / hblank_ly_scx
PPU vblank_stat_intr / stat_lyc_onoff
PPU intr_2_0/mode0/mode3/oam_ok  ← STAT mode 滞后 4 dot 标定
```

> **T-cycle 迁移状态**:CPU 时间推进已收敛到单一 T-cycle seam(`Cpu::tick_t`,见
> `refactor(cpu): T-cycle tick primitive`)。实测 CPU 的访存本就落在 M-cycle 内正确的
> 末拍(≈T4,故 Blargg `mem_timing` 过),所以 CPU 的**访存**已 T-accurate。

### 3.6 已定位并修复:STAT mode 读数的常量拍偏移(intr_2 一簇)
一段"证伪 → 定位 → 修复"的完整过程,值得记下方法:

1. `ppu_probe` 实测 PPU 的 mode2-int / mode3 / mode0 落点是教科书值 **dot 0 / 80 / 252**,
   **PPU 内部时序没偏**。
2. 曾假设要靠**整颗 CPU 逐 T 步进**修。**实测证伪**:完整实现了 cycle-stepped 核(`tick_t`
   逐 `bus.tick(1)` + timer 守卫改 `begin_m_cycle` 钩子 + T-cycle HALT 唤醒),结果对运行态
   零变化、`intr_2` 仍全挂、且 T-cycle HALT 唤醒还回归了 `hblank`。**结论:CPU 结构不是瓶颈。**
3. 关键观察:`intr_2_0_timing`(测 mode2 中断本身)**过**,而 `intr_2_mode0/mode3/oam_ok`
   (测从 mode2 中断到"STAT 读到 modeX / OAM 可访问")**挂**。所以偏移在**软件观测到 mode 的
   时刻**,不在中断、不在内部转换。`hblank`(相对测量)对常量偏移免疫,故一直过。
4. **修复**:真机上 STAT 寄存器的 mode 位、以及 OAM/VRAM 锁,都比内部 mode 转换**晚约 4 dot**。
   加 `prev_mode` / `transition_age`,用 `visible_mode()`(滞后 4 dot)供 STAT 读和 OAM 访问判定
   ——`intr_2_mode0/mode3/oam_ok` 三个全绿,acid2 字节不变,无回归(见 `fix(ppu): STAT mode
   bits ... lag ... 4 dots`)。

教训:别急着上大重写;先用 probe/trace 把偏移**量化**,常常是外设的一个常量拍。

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
