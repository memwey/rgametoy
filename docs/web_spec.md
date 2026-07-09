# rgametoy Web Frontend — 技术规格

> Status: Draft v1
> Target: 本地 `trunk serve` 跑通即可
> Stack: Rust + web-sys + js-sys + wasm-bindgen + Trunk(无 UI 框架)

## 1. 目标

把 `rgametoy-core` 编译成 wasm,在浏览器里跑出对齐桌面版核心子集的体验:加载用户上传的 ROM、渲染、输入、调色板、音频、电池存档、即时存档。v1 不做部署,本地 `trunk serve` 跑通即可。

## 2. 不在 v1 范围

- 触屏手柄(移动端 D-pad)
- PWA / 离线安装
- 多 ROM 切换 / ROM 库
- MBC2 / MMM01 / RTC(核心本身不支持,沿用)
- 联机 / Netplay
- AudioWorklet(v1 用 `ScriptProcessorNode`,v2 再升级)

## 3. 架构总览

```
rgametoy-web/
├── Cargo.toml          # workspace 成员,依赖 rgametoy-core + wasm-bindgen + ...
├── Trunk.toml
├── index.html
├── style/main.css
└── src/
    ├── lib.rs          # #[wasm_bindgen(start)],挂监听,启动主循环
    ├── app.rs          # AppState 状态机
    ├── wasm_host.rs    # #[wasm_bindgen] 包装 Console
    ├── canvas.rs       # canvas 初始化 + rAF 循环 + putImageData
    ├── input.rs        # KeyboardEvent → button 位掩码
    ├── audio.rs        # AudioContext + ScriptProcessorNode
    ├── storage.rs      # IndexedDB 包装
    ├── rom.rs          # File input → Cartridge
    ├── palette.rs      # 与 desktop 同步的调色板常量
    └── ui.rs           # DOM 状态(按钮 enable/disable、状态行)
```

`rgametoy-core` 仍然零依赖,不引入 `serde` / `wasm-bindgen` 等。`rgametoy-desktop` 完全不动。

## 4. wasm-host API(`wasm_host.rs`)

`Console` 不能直接 `#[wasm_bindgen]` 暴露(它含 `Vec` / `Arc` 等)。用 newtype 包装,通过 `Rc<RefCell<Inner>>` 共享内部状态(音频回调需要独立访问同一份 console):

```rust
#[wasm_bindgen]
pub struct WasmHost {
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    console: Console,
    audio: Option<AudioState>,
}

#[wasm_bindgen]
impl WasmHost {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmHost;

    // ROM
    pub fn load_rom(&self, bytes: &[u8]) -> Result<(), JsValue>;
    pub fn has_rom(&self) -> bool;
    pub fn cartridge_title(&self) -> String;
    pub fn rom_hash(&self) -> String;
    pub fn has_battery(&self) -> bool;

    // 帧
    pub fn step_frame(&self);
    pub fn framebuffer(&self) -> js_sys::Uint8Array;     // 23040 字节,copy

    // 音频
    pub fn audio_output_rate(&self) -> u32;
    pub fn take_audio_samples(&self) -> js_sys::Float32Array;
    pub fn enable_audio(&self) -> Result<(), JsValue>;
    pub fn disable_audio(&self);
    pub fn audio_enabled(&self) -> bool;

    // 输入
    pub fn set_buttons(&self, mask: u8);
    pub fn get_buttons(&self) -> u8;

    // 存档(序列化字节流)
    pub fn save_state_bytes(&self) -> js_sys::Uint8Array;
    pub fn load_state_bytes(&self, bytes: &[u8]) -> Result<(), JsValue>;
    pub fn save_ram_bytes(&self) -> Option<js_sys::Uint8Array>; // None if no battery
    pub fn load_ram_bytes(&self, bytes: &[u8]) -> Result<(), JsValue>;
    pub fn ram_dirty(&self) -> bool;
}
```

方法签名全部是 `&self`,内部走 `RefCell::borrow_mut`。这样音频回调能 clone `Rc<RefCell<Inner>>` 独立访问,而 `WasmHost` 本身也可以被多处持有。

## 5. core 改动:SaveState 序列化

`rgametoy-core` 当前没有把 `SaveState` / `Console` 序列化为字节的接口。Web 端要存进 IndexedDB,所以 core 必须加。

### 5.1 公共 API

```rust
// 在 rgametoy-core/src/lib.rs
impl Console {
    pub fn save_state_bytes(&self) -> Vec<u8>;
    pub fn load_state_bytes(&mut self, bytes: &[u8]) -> Result<(), SaveStateError>;
}

// 在 rgametoy-core/src/cartridge.rs
impl Cartridge {
    pub fn save_ram_bytes(&self) -> Vec<u8>;
    pub fn load_ram_bytes(&mut self, bytes: &[u8]) -> Result<(), SaveStateError>;
}

#[derive(Debug)]
pub enum SaveStateError {
    BadMagic,
    UnsupportedVersion(u8),
    Truncated,
    CrcMismatch,
    UnknownVariant,
}
```

### 5.2 二进制格式

```
[Magic: "RGSV" 4 bytes]
[Version: u8 = 1]
[CRC32 of payload: u32 LE]
[Payload: LEB128 长度前缀 + 数据]
```

Payload 内容(顺序固定):

| 字段 | 类型 | 备注 |
|---|---|---|
| total_cycles | u64 LE | |
| CPU 全部寄存器 | 原始字节 | 由 `Cpu::write_to_bytes` / `read_from_bytes` 暴露 |
| WRAM | `[u8; 8192]` | |
| HRAM | `[u8; 127]` | |
| I/O 寄存器 0xFF00..=0xFF4B | `[u8; 0x4C]` | |
| PPU 状态 | 内部 struct 序列化为字节 | |
| APU 状态 | 内部 struct | |
| Timer 状态 | 内部 struct | |
| Interrupt 状态 | 内部 struct | |
| Cartridge 状态(MBC 寄存器 + RAM) | 见下 | |

Cartridge 状态序列化:
- MBC kind tag(u8: 0=None, 1=MBC1, 3=MBC3, 5=MBC5)
- `rom_bank`, `ram_bank`, `banking_mode`, `ram_enabled`(按 kind 决定实际写入哪些)
- `ram` 长度 u32 LE + 字节

### 5.3 CRC32

core 自带 256 项表(无依赖),多项式 `0xEDB88320`,在 `lib.rs` 私有 helper。`#[cfg(test)]` 加一个 roundtrip 测试。

### 5.4 兼容性

- v1 写 magic `"RGSV"` + version=1
- 未来加新字段时,version 升到 2,旧 v1 仍能读(忽略末尾未知字节)

## 6. 数据流

### 6.1 帧循环

```text
rAF callback (主线程, ~60 Hz)
  ├── host.set_buttons(input_state.buttons)
  ├── host.step_frame()
  ├── fb = host.framebuffer()                  // Uint8Array(23040 bytes)
  ├── image_data = new ImageData(new Uint8ClampedArray(fb), 160, 144)
  ├── canvas_ctx2d.putImageData(image_data, 0, 0)
  └── 若音频开:AudioPlayer::feed(本帧 APU 样本) → 重采样 → postMessage 给 worklet(见 §6.2)
```

**计时基准(重要)**:我们适配的是**模拟出的 Game Boy 输出**,它才是计时的钟——DMG 逐点/逐行凑成一帧(154 行 × 456 点 = 70224 周期),落在 ~59.7 fps。因此:

- **速度以"模拟机输出速率 vs 墙钟"定义**,不看 host 显示。1× = ~59.7 模拟 fps;快进 N× = 让 DMG 产出 N × 59.7 fps(4× ≈ 240 fps)。
- **呈现速率 ≠ 模拟速率,由显示适配**。`requestAnimationFrame` 硬锁在显示器刷新率(60/144 Hz),所以每次刷新画最新的一帧、必要时丢帧;游戏速度仍是墙钟下的 N×。这就是快进用**墙钟累加器**(`dt × 倍速`)而非"每次重绘跑固定帧数"的原因——后者会让速度跟着刷新率变(60 Hz 上 4×、144 Hz 上 ~9.6×),反而和 desktop 不一致。
- **变速时音频静音**(见 §6.2):N× 下 APU 每真实秒产出的采样数对不上设备,与其硬重采样不如丢掉;加速后的音乐也没意义。

这条原则对 desktop 同样成立(desktop 用 `frame_budget` sleep 按墙钟计,`present` 不锁 vsync、上限 ~250 fps),两端一致。

### 6.2 音频(AudioWorklet 输出汇)

```text
主线程 rAF 每帧:
  console.take_audio_samples()            // 本帧的 APU 样本(source_rate 交织立体声)
  └── AudioPlayer::feed(samples)
       ├── resampler: source_rate → device_rate
       └── port.postMessage(Float32Array) ──► 音频渲染线程

音频渲染线程(AudioWorkletProcessor.process,每次 128 帧):
  从环形缓冲(约 170ms)pop 出 128 帧;欠载补静音,溢出丢最旧(延迟有界)
```

`take_audio_samples` 每帧清空 console 内部 sample buffer,所以不会积压。

**驱动权(统一)**:**rAF 循环始终驱动模拟**,按墙钟计(见 §6.1);音频只是**下游汇**,不驱动模拟。音频开时,每帧把 APU 输出喂给 worklet;音频关时不喂。**快进**(按住 `Space`)时 rAF 以固定倍速(4×,对齐 desktop)驱动、且**不喂音频**——即"加速即静音"。

这是从旧版(主线程 `ScriptProcessorNode` 的 `onaudioprocess` 驱动模拟)迁过来的:输出移到音频渲染线程(`AudioWorklet`),更抗主线程卡顿;驱动权统一到 rAF,消除了"音频开时驱动方式不一致"和音频回调重入借用 `Inner` 的隐患。worklet 用 blob URL 内联加载(无需单独静态资源)。**权衡**:改成 sync-to-video 后,系统钟与声卡钟会缓慢漂移,由 worklet 的环形缓冲吸收(溢出丢最旧、欠载补静音)。

### 6.3 ROM 加载时序

```text
file input change
  └── bytes = await file.arrayBuffer()
       └── host.load_rom(bytes)
            ├── Cartridge::from_bytes + 类型校验
            ├── console.load_cartridge(...)
            ├── idb.get(rom_hash) → {ram, quick_state}
            ├── if ram: host.load_ram_bytes(ram)
            ├── if quick_state: host.load_state_bytes(quick_state)
            ├── UI: 显示 title, enable Pause/Reset/Save/Load
            └── rAF 循环已在跑(只是没 ROM 时画黑屏)
```

### 6.4 存档自动写回

```text
rAF 循环每帧
  └── if (frame_idx % 120 == 0) && host.ram_dirty()
       └── idb.put(rom_hash, {ram: host.save_ram_bytes(), quick_state, ...})
       └── host.clear_ram_dirty() (若 API 暴露;否则 bus 直清)
```

debounce 120 帧 ≈ 2 秒。

## 7. 输入映射

与 desktop `input.rs` **完全一致**(整套键位刻意统一,muscle memory 跨端通用):

| Host `KeyboardEvent.code` | Game Boy 位 |
|---|---|
| `ArrowRight` / `ArrowLeft` / `ArrowUp` / `ArrowDown` | D-pad |
| `KeyZ` / `KeyX` | A / B |
| `Enter` / `ShiftRight` | Start / Select |
| `Space` | 快进(按住) |
| `Digit5` / `Digit7` | 即时存 / 读 |
| `Digit2` | 截图下载 |
| `Digit3` | 切调色板 |

热键用数字而非 F 键:`F5`/`F11`/`F12` 等被浏览器占用;快进用 `Space` 而非 `Tab`(`Tab` 是浏览器焦点切换键,即使 `preventDefault` 也别扭)。凡是模拟器要用的键,`on_keydown` 每次(含自动重复)都 `preventDefault`,防止方向键/空格滚动页面;未映射的键(`Tab`、`F5`…)照常交给浏览器。

按钮位掩码(`RIGHT`…`START`)由 core 定义(`rgametoy_core::joypad`,即 `set_buttons` 消费的那套契约),desktop / web 都 `pub use` 过来,单一真相源。(按键**映射表**因键类型不同 —— minifb `Key` vs `event.code` 字符串 —— 各前端一份,删不掉。)

边沿检测:host 这边不存"上一次按键",`AppState` 里维护 `prev_save / prev_load / prev_screenshot / prev_palette` 四个 bool,每帧更新。

## 8. 调色板

`palette.rs` 直接照搬 `desktop/src/palette.rs` 的 6 个调色板,文件头加注释:

```rust
// KEEP IN SYNC with crates/rgametoy-desktop/src/palette.rs
// 任何修改都要同步两边,除非把 palette 移到 core 共用模块。
```

Web 端需要 RGBA(不是 desktop 的 ARGB),加一个:

```rust
pub fn to_rgba(p: &Palette) -> [u8; 16] { /* 4 色 × 4 字节 R,G,B,A */ }
```

## 9. IndexedDB Schema

```text
DB:    rgametoy
Store: saves
       keyPath: romHash (string, 8 hex chars)

Record {
    romHash:     string,        // FNV-1a 32-bit hex
    romTitle:    string,
    ram:         Uint8Array | null,
    quickState:  Uint8Array | null,
    updatedAt:   number,        // Date.now()
}
```

不存 ROM 本身(版权 + 体积)。

## 10. ROM 标识

复用 desktop 的 FNV-1a 32-bit 8 hex chars。web 端在 `wasm_host.rs` 重新实现一遍(避免 web crate 依赖 desktop 的 host-only 代码)。FNV 算法就 5 行,加测试跟 desktop 算同一份字节的 hash 对齐。

## 11. 失焦 / 暂停

```text
window "blur" 事件
  ├── rAF 循环不退出,只是 step_frame 跳过(画上一帧)
  ├── audio_ctx.suspend()
  ├── set_buttons(0xFF)  // 全部释放
  └── UI 状态条显示 "paused"

window "focus" 事件
  ├── 恢复 step_frame
  ├── audio_ctx.resume() (若 enable 过)
  └── UI 状态条恢复
```

rAF 在后台 tab 会被节流到 1Hz,所以不主动 stop 循环也无所谓(代价是后台继续跑 console 逻辑)。v2 可以加 visibility 监听跳过 step。

## 12. UI 布局

```text
+---------------------------------------------------+
| rgametoy — <rom title>    audio: off  [Enable]    |
+---------------------------------------------------+
|                                                   |
|              [ 160x144 canvas,                    |
|                CSS scaled ×4 → 640x576 ]          |
|                                                   |
+---------------------------------------------------+
| [Load ROM] [Pause] [Reset] [Cycle palette]        |
| [Screenshot]    FPS: 60.0   Palette: DMG green     |
+---------------------------------------------------+
```

`index.html` 静态结构,所有动态状态由 `ui.rs` 操作。

## 13. 错误处理

- ROM 类型不支持(非 0x00/0x01-0x03/0x0F-0x13/0x19-0x1E):`load_rom` 返回 `Err(JsValue)`,`ui.rs` 弹 status line 提示
- IDB 打开失败:走内存模式(存档不持久),status line 提示
- Web Audio 启用失败(老浏览器):`enable_audio` 返回 `Err`,UI 灰掉按钮
- panic:`console_error_panic_hook` 输出到 `console.error`,页面显示红色 banner(可选)
- 不上抛到 status line 的失败(IDB 读写各阶段、存档记录损坏、快照恢复、ROM 文件读取、AudioContext resume、截图下载)统一经 `weblog::error`/`error_val` 打到 `console.error`;否则在隐私窗口 / 配额超限 / 坏记录时存档会静默失效且 DevTools 里无痕。逐帧 / 逐音频回调路径(present、声道拷贝)故意不记,避免刷屏

## 14. 构建

### 14.1 Trunk.toml

```toml
[build]
target = "index.html"
dist = "dist"

[serve]
port = 8080
```

### 14.2 index.html

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>rgametoy</title>
  <link data-trunk rel="css" href="style/main.css" />
</head>
<body>
  <div id="app"></div>
</body>
</html>
```

Trunk 在 build 时把 `data-trunk` 标签替换成产物引用。

### 14.3 依赖

`rgametoy-web/Cargo.toml`:

```toml
[dependencies]
rgametoy-core        = { workspace = true, features = ["serialize"] }
wasm-bindgen         = "0.2"
wasm-bindgen-futures = "0.4"
js-sys               = "0.3"
web-sys = { version = "0.3", features = [
    "Window", "Document", "Element", "HtmlElement",
    "HtmlCanvasElement", "CanvasRenderingContext2d", "ImageData",
    "HtmlInputElement", "HtmlButtonElement", "HtmlAnchorElement",
    "File", "FileReader", "Blob", "BlobPropertyBag", "Url", "Event", "KeyboardEvent",
    "AudioContext", "AudioDestinationNode", "AudioNode",
    "AudioWorklet", "Worklet", "AudioWorkletNode", "AudioWorkletNodeOptions", "MessagePort",
    "EventTarget", "console",
    "IdbFactory", "IdbOpenDbRequest", "IdbDatabase",
    "IdbRequest", "IdbObjectStore", "IdbTransaction",
    "DomException",
] }
console_error_panic_hook = "0.1"
```

`wasm-bindgen-futures` 用来 `await` `AudioWorklet.addModule` 返回的 promise(启用音频时)。

## 15. 本地开发

```bash
# 一次性
cargo install --locked trunk wasm-bindgen-cli
rustup target add wasm32-unknown-unknown

# 每天
cd crates/rgametoy-web
trunk serve              # 开发,HMR,默认 http://localhost:8080
# 或
trunk build --release    # 产物在 dist/,可拷去任意静态服务器
```

`trunk serve` 监听 8080,改 Rust 代码自动 rebuild + 浏览器 HMR,改 HTML/CSS 同样 HMR。

不部署,v1 工具链到此为止。

## 16. 风险与缓解

| 风险 | 缓解 |
|---|---|
| iOS Safari Web Audio 严格 | "Enable audio" 按钮强制 user gesture |
| iOS Safari IndexedDB 偶发 bug | 失败时降级到内存,status line 提示 |
| 后台 tab rAF 被节流到 1Hz | 失焦暂停,回到前台恢复 |
| 23 KB / 帧 Uint8Array 复制 | 实测 < 0.1 ms / 帧,可接受;v2 改 zero-copy |
| 第三方 ROM 兼容性 | 跟 desktop 一样只支持 0x00/0x01-03/0x0F-13/0x19-1E |
| `ScriptProcessorNode` deprecated | v1 可用,浏览器短期不会移除;v2 换 AudioWorklet |

## 17. 文件清单(实际要新增/修改)

**新增:**
- `crates/rgametoy-web/Cargo.toml`
- `crates/rgametoy-web/Trunk.toml`
- `crates/rgametoy-web/index.html`
- `crates/rgametoy-web/style/main.css`
- `crates/rgametoy-web/src/lib.rs`
- `crates/rgametoy-web/src/app.rs`
- `crates/rgametoy-web/src/wasm_host.rs`
- `crates/rgametoy-web/src/canvas.rs`
- `crates/rgametoy-web/src/input.rs`
- `crates/rgametoy-web/src/audio.rs`
- `crates/rgametoy-web/src/storage.rs`
- `crates/rgametoy-web/src/rom.rs`
- `crates/rgametoy-web/src/palette.rs`
- `crates/rgametoy-web/src/ui.rs`
- `docs/web_spec.md`(本文)
- `docs/web_tasks.md`

**修改:**
- `Cargo.toml`(workspace members)
- `crates/rgametoy-core/src/lib.rs`(加 save_state_bytes / load_state_bytes + SaveStateError)
- `crates/rgametoy-core/src/cartridge.rs`(加 save_ram_bytes / load_ram_bytes)
- 各 core 子模块(`cpu` / `ppu` / `apu` / `timer` / `interrupts` / `bus`)按需加 `to_bytes` / `from_bytes`
- `.gitignore`(加 `crates/rgametoy-web/dist/`)
