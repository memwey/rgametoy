# rgametoy

**English** | [中文](README_cn.md)

A DMG (original Game Boy) emulator written in Rust, with no third-party libraries
in the core logic (only `minifb` for the window).

## Build & run

```sh
cargo run --release -- path/to/rom.gb              # no sound, zero extra deps
cargo run --release --features audio -- rom.gb     # enable sound (pulls in cpal)
cargo run --release -- rom.gb 8                    # 2nd arg = fast-forward multiplier (default 4)
```

Key mapping: arrow keys = D-pad, `Z` = A, `X` = B, `Enter` = Start,
`Backspace` = Select, **hold `Tab` = fast-forward**, `F5` = save state /
`F7` = load state, `F2` = screenshot, `Esc` = quit.

Fast-forward is paced against the CPU clock: each emulated frame's real-time
budget is `frame_time / multiplier`, still presenting once per frame; releasing
`Tab` returns to full speed immediately. Audio is muted while fast-forwarding
(to avoid over-producing samples).

**Data directory**: saves and screenshots go into two subfolders under one base
directory. The base defaults to the working directory and can be overridden with
the `RGAMETOY_DATA_DIR` environment variable:

```text
<base>/
├── saves/        <rom-file-name>-<8-hex-content-hash>.sav   (battery SRAM)
└── screenshots/  <rom-title>-<epoch-millis>.bmp             (F2 screenshots)
```

**Screenshots**: `F2` saves the current frame at native 160×144 as a 24-bit BMP
(pixel-exact, good for debugging) and prints the path. The encoder is shared with
the headless `--example screenshot` (`src/emulator/screenshot.rs`), so the window
and the screenshots use the same colours.

Supported cartridges: no-MBC (32 KB), MBC1, MBC3 (no RTC) and MBC5, with external
RAM and bank switching. A battery-backed cartridge persists its external RAM to
`saves/<rom-file-name>-<content-hash>.sav` — the name is human-readable and the
hash ties it to the ROM content (two different ROMs sharing a filename never
clash); the `.sav` itself is raw SRAM (portable across emulators). It is read
back on load, flushed with debounce while running, and saved on exit.

Implemented: the full SM83 instruction set (including the CB prefix), interrupts
(VBlank/STAT/Timer/Serial/Joypad), the timer, pixel-FIFO PPU rendering
(background / window / sprites, dot-by-dot in mode 3), OAM DMA, serial (captured
output), keyboard input, screenshots (F2), battery saves (`.sav`), and
**4-channel APU sound** (2× square + wave + noise, with sweep / envelope / length
counters). The APU emulation core is pure Rust and always compiled; real audio
output goes through the `audio` feature (cpal), off by default.

Also supports **fast-forward** (hold Tab, configurable multiplier) and **instant
save / load state** (F5/F7, a deep copy of the whole machine to an in-memory
slot, zero-dependency). The PPU is a **pixel FIFO** (dot-by-dot in mode 3, so
mid-scanline register changes take effect). Not yet implemented: MBC3 RTC, MBC2.

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
    cargo test --release --test rom_suite                         # mooneye non-boot + Blargg
cargo run --release --example run_serial  -- path/to/test.gb      # print serial output (Blargg)
cargo run --release --example run_mooneye -- path/to/test.gb      # print PASS / FAIL (mooneye)
cargo run --release --example screenshot  -- rom.gb out.bmp       # headless-render one frame to BMP
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
