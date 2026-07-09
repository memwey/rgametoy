# rgametoy

**English** | [中文](README_cn.md)

A DMG (original Game Boy) emulator written in Rust. The emulation core
(`rgametoy-core`) has **zero third-party dependencies**; the desktop frontend
(`rgametoy-desktop`) adds only `minifb` for the window and optional `cpal` for
audio.

## Build & run

```sh
cargo run --release -- path/to/rom.gb              # no sound, zero extra deps
cargo run --release --features audio -- rom.gb     # enable sound (pulls in cpal)
cargo run --release -- rom.gb 8                    # 2nd arg = fast-forward multiplier (default 4)
```

Key mapping (identical in the desktop and web frontends): arrow keys = D-pad,
`Z` = A, `X` = B, `Enter` = Start, `Right Shift` = Select,
**hold `Space` = fast-forward**, `5` = save state / `7` = load state,
`2` = screenshot, `3` = cycle palette, `Esc` = quit (desktop).
The window title shows the live frame rate, speed multiplier, and current palette.

The clock for all pacing is the **emulated Game Boy's output** (~59.7 fps), not
the host display: speed is the emulated output rate against the wall clock, and
the display just adapts (presents whatever it can, frame-skipping if needed).
Fast-forward is therefore paced against that clock — each emulated frame's
real-time budget is `frame_time / multiplier`, so N× means the DMG produces
N × 59.7 fps regardless of monitor refresh; releasing `Space` returns to full
speed immediately. Audio is muted while fast-forwarding (at N× the APU produces
the wrong number of samples per real second, and sped-up audio is meaningless).
The web frontend follows the same principle (paced against the wall clock, with
the browser presenting at whatever rate it can).

**Data directory**: saves and screenshots go into two subfolders under one base
directory. The base defaults to the working directory and can be overridden with
the `RGAMETOY_DATA_DIR` environment variable:

```text
<base>/
├── saves/        <rom-file-name>-<8-hex-content-hash>.sav   (battery SRAM)
└── screenshots/  <rom-title>-<epoch-millis>.bmp             ("2" key screenshots)
```

**Screenshots**: the `2` key saves the current frame at native 160×144 as a 24-bit BMP
(pixel-exact, good for debugging) and prints the path. The encoder is shared with
the headless `-p rgametoy-desktop --example screenshot` (`crates/rgametoy-desktop/src/screenshot.rs`), so the window
and the screenshots use the same colours.

**Palettes**: the core only emits shade values 0–3, so mapping them to colours is
a pure frontend choice (`crates/rgametoy-desktop/src/palette.rs`). The `3` key cycles through a few
built-in palettes — the classic DMG green plus tinted variants (grayscale, amber,
ocean, berry) that colourise the four shades. Screenshots use whichever palette is
active. The default is DMG green.

**Display**: the window presents the *native* 160×144 buffer and lets minifb's
backend (Metal on macOS) upscale it on the GPU with nearest-neighbour sampling —
16× less data to upload each frame than pre-scaling on the CPU, so the frontend
stays cheap. The window title shows the live fps, speed multiplier, and palette;
with `--features debug` it also shows the per-frame core-vs-present time split.

**Logging**: frontend messages go through a tiny dependency-free logger
(`crates/rgametoy-desktop/src/log.rs`) — `info` to stdout, `warn`/`error` to stderr, with ANSI
colour only when the stream is a terminal and `NO_COLOR` is unset. A test ROM's
serial output is printed raw, never tagged.

Supported cartridges: no-MBC (32 KB), MBC1, MBC3 (no RTC) and MBC5, with external
RAM and bank switching. A battery-backed cartridge persists its external RAM to
`saves/<rom-file-name>-<content-hash>.sav` — the name is human-readable and the
hash ties it to the ROM content (two different ROMs sharing a filename never
clash); the `.sav` itself is raw SRAM (portable across emulators). It is read
back on load, flushed with debounce while running, and saved on exit.

Implemented: the full SM83 instruction set (including the CB prefix), interrupts
(VBlank/STAT/Timer/Serial/Joypad), the timer, pixel-FIFO PPU rendering
(background / window / sprites, dot-by-dot in mode 3), OAM DMA, serial (captured
output), keyboard input, screenshots (`2` key), battery saves (`.sav`), and
**4-channel APU sound** (2× square + wave + noise, with sweep / envelope / length
counters). The APU emulation core is pure Rust and always compiled; real audio
output goes through the `audio` feature (cpal), off by default.

Also supports **fast-forward** (hold Space, configurable multiplier) and **instant
save / load state** (`5`/`7`, a serialized snapshot of the whole machine held in an
in-memory slot). The PPU is a **pixel FIFO** (dot-by-dot in mode 3, so
mid-scanline register changes take effect). Not yet implemented: MBC3 RTC, MBC2.

## Architecture

The project is a Cargo **workspace** of three crates. `rgametoy-core` is the
emulated machine — deterministic, dependency-free, and it compiles to `wasm32`.
Two frontends drive it on the same core: `rgametoy-desktop` (native — minifb
window, optional cpal audio; builds the `rgametoy` binary) and `rgametoy-web`
(browser — bare wasm-bindgen / web-sys, AudioWorklet audio, IndexedDB saves,
Trunk build).

```text
  crates/rgametoy-desktop               crates/rgametoy-web
  native: minifb window, cpal audio     browser: wasm-bindgen/web-sys, AudioWorklet, IDB
       └────────────────────┬────────────────────┘
                            │  set_buttons() / run_frame() / step() / framebuffer()
                            ▼  both depend on ↓
  crates/rgametoy-core      the emulated DMG — deterministic, no host I/O, wasm-ready
  ──────────────────────
     Cpu (SM83) ─ bus master ─► BusView = System + inserted Cartridge (decodes addrs)
        every access / internal delay calls bus.tick(n)  ──┐
                                                            ▼  fans n T-cycles out to:
     System, timed:    Ppu (pixel FIFO)   Timer (DIV/TIMA)   Apu (4 ch)   Serial (link)
     System, passive:  P1 (joypad)   WRAM   HRAM
     Cartridge:        MBC1/3/5 + battery — a separate unit inserted at power_on,
                       borrowed by the bus per step (not owned by System)
```

- **`rgametoy-core`** is the emulated machine — no host I/O, fully deterministic.
  Save-states (the opt-in `serialize` feature — a real Game Boy can't snapshot
  itself, so the featureless core is *just* the machine) serialize the whole
  machine to a portable byte blob; the read-only ROM is shared via `Arc`, never
  copied into it.
- The **`Cpu` is the only bus master**: every memory access and internal delay calls
  `bus.tick(n)`, which advances the *timed* peripherals (PPU/Timer/APU/Serial) by `n`
  T-cycles. That single seam is what makes read/write timing observable. The *passive*
  peripherals (cartridge, joypad, RAM) only respond to accesses.
- **`rgametoy-desktop`** runs one `run_frame()` per host frame, then presents the
  framebuffer and paces to the real ~59.7 Hz.

## Timing

An M-cycle = 4 T-cycles = 4 PPU dots; time advances only through the CPU's ticks:

```text
  CPU step:   fetch     read      internal
              [ 4T ]    [ 4T ]    [ 4T ]
  bus.tick:    ►►►►      ►►►►      ►►►►     each ticks PPU/Timer/APU/Serial 4 dots
                                           (a register store lands on T4, the M-cycle end)
```

The PPU is a per-dot state machine. One scanline = 456 dots (452 for the special
first line after LCD enable); LY steps at the end:

```text
  dot        0           80                    252                        456
  internal   │─ mode 2 ──│──── mode 3 ─────────│──────── mode 0 ──────────│
              OAM scan     Drawing               HBlank
              80 dots      172 + SCX&7 + sprite/window penalties

  the STAT mode software reads LAGS the internal edge: +4 dots out of the scan,
  +1 out of Drawing (a subtlety several mooneye tests pin down) —
  STAT&3     │0─│──── mode 2 ────│──── mode 3 ─────│──────── mode 0 ──────────│
  dot        0  4                84               253
              └ mode 0 carried over from the previous line's HBlank for 4 dots

  Frame = 144 visible + 10 VBlank lines = 154 × 456 = 70224 dots ≈ 59.7 Hz.
```

The dot-precise details behind mode 3's length, the STAT lag and the first line are
in [docs/testing.md](docs/testing.md) §3.

### Test-ROM validation

The CPU is **cycle-accurate to the M-cycle** (every memory access / internal
cycle advances the peripherals), and the PPU is dot-by-dot in mode 3. It passes
Blargg (`cpu_instrs` all 11, `instr_timing`, `mem_timing` all **Passed**),
**dmg-acid2** (renders the full reference smiley), and mooneye acceptance
**63/75 (every non-boot test passes** — the remaining 12 are all boot-ROM tests,
outside the DMG scope); mealybug tearoom is 1/24 (the strictest suite — see the
doc).

The test methodology (gray-box module unit tests + black-box ROM suites), the
per-suite scoreboard, and the leftover issues (the sub-cycle / T-cycle frontier)
are detailed in [docs/testing.md](docs/testing.md).

```sh
cargo test --release                                              # gray-box module unit tests
GB_TEST_ROMS=/path/to/game-boy-test-roms \
    cargo test --release -p rgametoy-core --test rom_suite                         # mooneye non-boot + Blargg
cargo run --release -p rgametoy-core --example run_serial  -- path/to/test.gb      # print serial output (Blargg)
cargo run --release -p rgametoy-core --example run_mooneye -- path/to/test.gb      # print PASS / FAIL (mooneye)
cargo run --release -p rgametoy-desktop --example screenshot  -- rom.gb out.bmp       # headless-render one frame to BMP
cargo run --release -p rgametoy-core --example benchmark   -- rom.gb [frames]      # headless throughput: fps + xrealtime
```

## References
### Technical manuals
* [Pan Docs](https://gbdev.io/pandocs/)
* [Game Boy / Color Architecture](https://www.copetti.org/writings/consoles/game-boy/)
* [Gameboy Emulator Development Guide](https://github.com/Hacktix/GBEDG)
* [Game Boy: Complete Technical Reference](https://github.com/Gekkio/gb-ctr)
### Tutorials
* [Building a Gameboy From Scratch](https://raphaelstaebler.medium.com/building-a-gameboy-from-scratch-part-1-51d05496783e)
* [从零开始实现GameBoy模拟器](https://zhuanlan.zhihu.com/p/676908347)
* [Rewriting My Game Boy Emulator: The Pixel FIFO](https://jsgroth.dev/blog/posts/gb-rewrite-pixel-fifo/)
### Emulator projects
* [GB Studio](https://github.com/chrismaltby/gb-studio)
* [GoBoy](https://github.com/Humpheh/goboy/)
* [PyBoy](https://github.com/Baekalfen/PyBoy/)
* [DawnGB](https://github.com/akatsuki105/dawngb)
* [Azayaka](https://github.com/7thSamurai/Azayaka)
* [Rugby](https://github.com/kaplanz/rugby)
* [GameRoy](https://github.com/Rodrigodd/gameroy) — referenced for its first-line-after-enable model (line 0 mode 3 at cycle 84) and OBJ penalty
* [jgb](https://github.com/jsgroth/jgb)
* [Mooneye GB](https://github.com/Gekkio/mooneye-gb)
* [GateBoy](https://github.com/aappleby/MetroBoy)
* [SameBoy](https://github.com/LIJI32/SameBoy) — a high-accuracy reference, cross-checked while calibrating the first-line (lcdon) timing
### Test-ROM suites
* [c-sp/game-boy-test-roms](https://github.com/c-sp/game-boy-test-roms) — a packaged release of the suites below (this project uses v7.0, fed to `rom_suite` via `GB_TEST_ROMS`)
* [Blargg's gb-test-roms](https://github.com/retrio/gb-test-roms) — `cpu_instrs` / `instr_timing` / `mem_timing` (judged over serial)
* [mooneye-test-suite](https://github.com/Gekkio/mooneye-test-suite) — cycle-accurate CPU/PPU/timer acceptance tests; decoding its `.s` expectation tables calibrated `lcdon` / `intr_2` / `rapid_toggle`
* [dmg-acid2](https://github.com/mattcurrie/dmg-acid2) — PPU renders a reference smiley, compared pixel by pixel
* [Mealybug Tearoom Tests](https://github.com/mattcurrie/mealybug-tearoom-tests) — mid-mode-3 register changes, compared pixel by pixel (see `tools/mealybug_compare.py`)
