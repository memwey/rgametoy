# rgametoy Web Frontend — 任务表

> 状态: Draft
> 每个阶段必须跑通上一阶段才能进。子任务不锁死顺序,看实现方便。

## Phase 1 — 骨架

让 `trunk serve` 跑起来,wasm 能在浏览器里跑,helloworld 级别。

- [ ] **T1** workspace members 加 `crates/rgametoy-web`
- [ ] **T2** `crates/rgametoy-web/Cargo.toml`(空 crate,只挂 `wasm-bindgen` 跑通编译)
- [ ] **T3** `crates/rgametoy-web/Trunk.toml`
- [ ] **T4** `crates/rgametoy-web/index.html` + 空 `style/main.css`
- [ ] **T5** `lib.rs`:挂 `#[wasm_bindgen(start)]`,浏览器 console 打 "rgametoy web ok"
- [ ] **T6** 本地 `trunk serve`,浏览器看到 console log,canvas 区域是空白

**验收**:`trunk serve` 起来,DevTools console 有 log,无报错。

---

## Phase 2 — core 加序列化

web 端能存存档的硬性前置,先做。

- [ ] **T7** `Console::save_state_bytes() -> Vec<u8>` + `load_state_bytes()`
- [ ] **T8** `Cartridge::save_ram_bytes() / load_ram_bytes()`
- [ ] **T9** `SaveStateError` 类型 + magic `"RGSV"` + version 1 + CRC32
- [ ] **T10** 各子模块加 `to_bytes` / `from_bytes`(CPU、PPU、APU、Timer、Interrupts、Bus)
- [ ] **T11** roundtrip 测试:`save_state_bytes` → `load_state_bytes` 后 `framebuffer` / `audio_samples` / `cpu` 状态一致
- [ ] **T12** 跑现有 `rgametoy-core` 全部测试,确认无 regression

**验收**:`cargo test -p rgametoy-core` 全绿,新加 roundtrip 测试通过。

---

## Phase 3 — wasm-host 壳

让 JS 能调 Console 基础操作,canvas 能画一帧黑屏。

- [ ] **T13** `wasm_host.rs`:`WasmHost` newtype + `Rc<RefCell<Inner>>`
- [ ] **T14** 最小 API 子集:`new / step_frame / framebuffer / set_buttons / load_rom`(只 ROM 加载,不存档)
- [ ] **T15** `palette.rs` 调色板常量 + `to_rgba`
- [ ] **T16** `canvas.rs`:rAF 循环 + `putImageData`(用 shade 默认调色板)
- [ ] **T17** `rom.rs`:`<input type="file">` change → `arrayBuffer` → `host.load_rom`
- [ ] **T18** `ui.rs`:Load ROM 按钮 + status 行
- [ ] **T19** 跑一个不要求音频的 ROM(比如 Tetris),画面正常

**验收**:浏览器能上传 `.gb`,画面跑起来。

---

## Phase 4 — 输入

- [ ] **T20** `input.rs`:`KeyboardEvent.code` → 按钮位掩码
- [ ] **T21** D-pad + A/B/Start/Select 全映射
- [ ] **T22** 边沿检测(5/7/2/3 一次按一次触发)
- [ ] **T23** Tab turbo
- [ ] **T24** window blur 自动 `set_buttons(0xFF)`,focus 恢复
- [ ] **T25** 跑一个按键密集的 ROM(马里奥之类),手感正常

**验收**:玩一关马里奥能正常跳、打砖块、暂停。

---

## Phase 5 — 音频

- [ ] **T26** `audio.rs`:`AudioContext` + `ScriptProcessorNode` 接线
- [ ] **T27** 桥接 APU samples 到 `onaudioprocess` 回调(共享 `Rc<RefCell<Inner>>`)
- [ ] **T28** "Enable audio" 按钮(必须 user gesture)
- [ ] **T29** 失焦 `audio_ctx.suspend()`,focus `resume()`
- [ ] **T30** 跑 dmg_sound 音频测试 ROM,听输出正确

**验收**:能听到游戏的 BGM + SFX,iOS Safari 上需要点 Enable audio 才有声音。

---

## Phase 6 — 存档

- [ ] **T31** `storage.rs`:IndexedDB 包装(`open_db` / `get` / `put` / `delete`)
- [ ] **T32** ROM 加载时查 IDB,自动加载 RAM + quick state
- [ ] **T33** RAM dirty 检测 + 每 120 帧 debounce 写回
- [ ] **T34** 5 → `host.save_state_bytes()` → 写 IDB
- [ ] **T35** 7 → 从 IDB 读 quick state → `host.load_state_bytes`
- [ ] **T36** 刷新页面后状态恢复(电池 RAM 必须,quick state 期望)

**验收**:玩到一半刷新,RAM 状态不丢;5 存档后退出再回来,7 能继续。

---

## Phase 7 — 截图 + 打磨

- [ ] **T37** 2:把当前 framebuffer 画到内存 canvas,`toBlob('image/png')` → `<a download>` 触发下载
- [ ] **T38** FPS 计数器(rAF timestamp 滚动 60 帧平均)
- [ ] **T39** Pause / Reset 按钮
- [ ] **T40** 错误 toast(ROM 不支持、IDB 失败、Audio 失败)
- [ ] **T41** CSS 美化(canvas 居中、控制条统一间距)

**验收**:完整桌面版核心子集都能用。

---

## Phase 8 — 本地验证

- [ ] **T42** 装 trunk + wasm-bindgen-cli,加 `wasm32-unknown-unknown` target
- [ ] **T43** `.gitignore` 加 `crates/rgametoy-web/dist/`
- [ ] **T44** `trunk serve` 起来,跑通一个完整 ROM 全流程
- [ ] **T45** 桌面 Chrome / Firefox / Safari 实测
- [ ] **T46** wasm 体积报告,记下基线(给未来 v2 优化用)

**验收**:本地浏览器能跑完一局游戏,工具链装好后增量编译秒级响应。

---

## Phase 9 — 加固(错误可见性 + 回归测试)

Phase 1–8 跑通后的收敛,降低后续维护风险。全部本地已提交(未 push)。

- [x] **T47** 错误探照灯:所有不上抛 status line 的静默失败(IDB 读写各阶段 / 坏存档记录 / 快照恢复 / ROM 文件读取 / AudioContext resume / 截图)经 `weblog::error(_val)` 打到 `console.error`;逐帧 / 逐音频回调路径(present、声道拷贝)故意不记,避免刷屏
- [x] **T48** host 单测:把 rAF 节拍数学抽成纯函数 `frames_to_run` 并测(含 60/120/144 Hz 都收敛到 ~59.7 fps 的回归);补音频 `Resampler` 等/降/升采样。`cargo test -p rgametoy-web` 17 绿
- [x] **T49** 收敛闭包:11 个仅为续命而存在的 `Inner::*_closure` 字段 → `on_click` / `on_event` 两个 helper(建 + 注册 + `forget()`);仅保留 `raf_slot`(自重排)和 `rom_reader_closure`(每次选文件重建)
- [x] **T50** 浏览器集成测试(`tests/web.rs`,`wasm-bindgen-test`):SaveRecord ⇄ JS 对象往返 + `canvas::present` 落像素。跑法 `wasm-pack test --headless --firefox --test web`;当前仅 wasm32 编译通过,断言需真浏览器执行

**验收**:三条 clippy ratchet + 17 host 测试全绿;浏览器测试 wasm32 编译通过(实跑待 `wasm-pack`)。

---

## 总依赖图

```
Phase 1 ─→ Phase 2 ─┐
                    ├─→ Phase 3 ─→ Phase 4 ─→ Phase 5 ─→ Phase 6 ─→ Phase 7 ─→ Phase 8
       (core 改动) ──┘
```

Phase 2 跟 Phase 1 完全独立,可以并行做(core 改动不依赖 web crate 的存在)。但 web 的存档功能(Phase 6)强依赖 Phase 2。

## 风险登记

- [R1] Phase 2 改动 core 内部接口可能动到现有 desktop 代码 → 提前同步 desktop 端的 `SaveState` 用法
- [R2] `web-sys` features 漏配导致 `JsValue` 编译失败 → 第一次配齐后写脚本验证
- [R3] Trunk 工具链首次 install 慢 → 用 `--locked` 锁版本,后续增量重装快
- [R4] iOS Safari IndexedDB 偶有 bug → 降级到内存模式 + status line 提示
- [R5] wasm-bindgen `#[wasm_bindgen(start)]` 跟 `mount` 顺序不直观 → 先写 lib.rs 跑通,再分模块
