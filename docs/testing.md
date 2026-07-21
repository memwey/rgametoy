# Testing: method, scoreboard, and open issues

**English** | [中文](testing_cn.md)

This document records how rgametoy is validated, the current pass/fail state, and
the issues not yet cracked. The goal throughout is to "stay as close as possible
to the real hardware structure, at T-cycle precision" (see [AGENTS.md](../AGENTS.md)),
so beyond ordinary unit tests, accuracy is measured mainly with the community's
standard **test ROMs**.

---

## 1. Method

Two layers — **gray-box** module unit tests (fast, pin the internal timing) plus
**black-box** ROM suites (the ground truth).

### 1.1 Module unit tests (`cargo test`) — black-box; peek only through the debug tooling

Two rules:
1. **Tests don't reach into internals themselves** — no reading private fields, no
   `pub` opened just for a test. By default everything goes through the surface
   software can observe: register reads/writes (`read_register`/`write_register` =
   MMIO), `tick`, the framebuffer, interrupts, memory accessibility — the same as a
   real program and every test ROM.
2. **When a test genuinely needs an internal value software can't read, it goes
   through "an interface the debug tooling provides"** — `Ppu::get_mode`/`debug_state`
   behind `--features debug` (the same inspection surface `inspect`/`ppu_probe` use),
   not by poking a field. Such tests are also `#[cfg(feature="debug")]`-gated and
   only compile/run under `cargo test -p rgametoy-core --features debug`; the default public API
   stays black-box.

A Game Boy's "externally observable surface" is closed, so most internal timing can
be tested directly through it: e.g. the STAT-mode timeline (mode 2 through dot 83,
mode 3 from dot 84, including the 4-dot lag) and the first line after enable (LY
reaching dot 452, no scan) — those are default black-box tests. Only something like
the **internal** mode-3 length (172 + penalty), whose absolute value software can't
read, lives in `ppu_test.rs`'s `#[cfg(feature="debug")] mod debug_timing` and is
observed via `get_mode`; its observable consequences are also covered black-box by
rom_suite's mooneye `intr_2`/`lcdon` (see §1.2).

These files live under `crates/rgametoy-core/tests/` (paths below are shortened).

| Location | Covers |
|---|---|
| `tests/ppu_test.rs` (default black-box) | STAT-mode timeline, first line after enable (LY 452 + no scan), LY==LYC, OAM/VRAM locks, sprite priority/OBP, WX<7 clipping, mid-line register changes, whole-frame rendering |
| `tests/ppu_test.rs` `#[cfg(feature = "debug")] mod debug_timing` | mode-3 internal length / SCX / stacked-sprite-penalty aggregation (via the debug tool `get_mode`, `--features debug` only) |
| `tests/cpu_instructions_test.rs` / `registers_test.rs` | instruction semantics, flags, registers |
| `tests/cpu_integration_test.rs` | whole programs, `ADD HL` carries, `ie_push` vector re-evaluation, illegal-opcode lock-up |
| `tests/timer_test.rs` | 16-bit counter, four frequencies, falling-edge glitches (TAC/DIV), the three-state reload delay |
| `tests/dma_test.rs` | OAM DMA start delay, source-bus blocking (VRAM/external), echo source, I/O readable |
| `tests/joypad_test.rs` | P1 select mapping, interrupt gated by select, release/select-exposes-held-button edges |
| `tests/apu_test.rs` | four channels, envelope/sweep/length, DAC; plus the obscure length/sweep/power quirks (dmg_sound 03/05/08/11) pinned directly via NR52, each verified to fail if its fix is reverted |
| `tests/cartridge_test.rs` / `save_test.rs` | MBC, battery saves |
| `tests/save_state_test.rs` / `save_state_bytes_test.rs` (`--features serialize`) | save-state byte round-trip (re-serialize equality, lockstep, atomic reject) |
| `tests/serial_test.rs` / `rom_render_test.rs` | serial capture, whole-frame rendering |
| `tests/common/mod.rs` | shared scaffolding (black-box PPU helpers + ROM runner), pulled in per file via `mod common;` |

```sh
cargo test --release
```

### 1.2 Test-ROM validation — black-box

Accuracy is measured with the community's canonical test ROMs. The ROMs are **not
distributed with the repo** (size + licensing); they come from the release bundle
of [`c-sp/game-boy-test-roms`](https://github.com/c-sp/game-boy-test-roms) (this
project uses v7.0). Each ROM signals "pass" differently:

| Suite | How it's judged | Entry point |
|---|---|---|
| **Blargg** (cpu/timing) | prints the result ("Passed"/"Failed") over the **serial** port, captured and compared | `rom_suite` / `examples/run_serial.rs` |
| **Blargg `dmg_sound`** (APU) | writes a status byte to **`$A000`** (signature `DE B0 61`; 0x00 = passed) — not serial | `rom_suite` (`blargg_ram_status`) |
| **mooneye** | on success loads the Fibonacci signature `3,5,8,13,21,34` into `B,C,D,E,H,L`; anything else is failure | `rom_suite` / `examples/run_mooneye.rs` |
| **dmg-acid2** | renders a reference image, compared **pixel by pixel** | `examples/screenshot.rs` (to BMP) / `dump_fb.rs` |
| **mealybug** | one specific frame compared **pixel by pixel** to the reference PNG (`*_dmg_blob.png`) | `tools/mealybug_compare.py` |

**Committed ROM integration test (`crates/rgametoy-core/tests/rom_suite.rs`)**: makes the **self-signalling**
black-box suites a `cargo test` — set `GB_TEST_ROMS` to the bundle root to run them,
leave it unset to skip the whole thing (default `cargo test` is unaffected):

```sh
GB_TEST_ROMS=/path/to/game-boy-test-roms cargo test --release -p rgametoy-core --test rom_suite
```

Three assertions: mooneye acceptance **all non-boot pass** (walks the tree, skips
`boot*`), Blargg cpu/timing report "Passed" over serial, and Blargg `dmg_sound`'s
**9 passing subtests stay passing** (ratchet — the other 3 are documented in §2.5,
not asserted). These self-certify via register signature / serial / `$A000`, so
they **need no reference data** and are a good fit for a committed automated test.

**mealybug is a local scaffold, not an automated test**: it is a **per-pixel
similarity** metric (not pass/fail), and each test needs a reference image — which
(`<name>_dmg_blob.png`) already sits next to the ROM in the bundle, so committing a
second copy would be redundant. Hence `tools/mealybug_compare.py` (see §2.4): with
the ROMs present it reads the colocated PNG and compares, with **zero committed
data**. It is Python because Rust's std has no inflate, so decoding a PNG would pull
in a third-party crate (against the "no third-party libs in the core"), whereas
Python's stdlib zlib decodes it directly.

**Debugger (`--features debug`)**: an in-project inspector (see `crates/rgametoy-core/src/debug.rs`).
`Console::snapshot()` grabs the whole machine's observable state at once (including
the PPU internal mode/dot, STAT line and LY==LYC latch that registers don't show),
and `run_until` sets a breakpoint. `examples/inspect.rs` is its CLI:

```sh
cargo run --release --features debug -p rgametoy-core --example inspect -- rom.gb break 0x48    # run to PC, print a full snapshot
cargo run --release --features debug -p rgametoy-core --example inspect -- rom.gb watch 0x48 6  # snapshot on each PC hit
cargo run --release --features debug -p rgametoy-core --example inspect -- rom.gb line 0        # mode/dot changes on a scanline
cargo run --release --features debug -p rgametoy-core --example inspect -- rom.gb dumpat PC A L  # hex-dump a memory range at a PC hit
```
`stat_lyc_onoff` and `lcdon_*` were located step by step with it.

A few judging details:
- The **mooneye** Fibonacci signature is the universal "pass" convention for these
  hardware tests. `rom_suite`/`run_mooneye` run some frames then read the registers:
  240 frames is enough for the vast majority; `intr_2_mode0_timing_sprites` needs
  ~2900 frames (100+ testcases, each waiting a frame), so 240 falls back to 3000.
- The **mealybug** effect appears on a **single frame**: the CPU does a burst of
  dot-aligned mid-line writes, executes `ld b,b` (a breakpoint) and stops; later
  frames redraw without the effect. But these tests loop the same effect every
  frame, so ~frame 30 is stable. The reference PNG is grayscale (bit-depth 1/2),
  mapped to our 0–3 shade with polarity `shade = 3 - gray` (gray normalised to
  0–3). **A 100% match is required to PASS.**

---

## 2. Scoreboard

> Data for the current `dev` branch. The core CPU is **cycle-accurate to the
> M-cycle** (every access / internal cycle advances the peripherals).

### 2.1 Blargg — all pass ✅

| ROM | Result |
|---|---|
| `cpu_instrs` (all 11) | Passed |
| `instr_timing` | Passed |
| `mem_timing` | Passed |

### 2.2 dmg-acid2 — pass ✅

Renders the full reference smiley (byte-identical before/after the FIFO rewrite,
evidence the rendering is correct).

### 2.3 mooneye acceptance — 63 / 75 (every non-boot test passes ✅)

| Group | Score | Notes |
|---|---|---|
| bits | 3/3 ✅ | |
| instr | 1/1 ✅ | |
| interrupts | 1/1 ✅ | `ie_push` (a push over IE re-evaluates the vector, §3.1) |
| oam_dma | 3/3 ✅ | source-bus blocking (§3.1) |
| timer | 13/13 ✅ | `rapid_toggle` (TAC store lands on T3, replayed inside the timer, §3.2) |
| ppu | 12/12 ✅ | STAT lag (§3.3), `lcdon_*` (§3.4), `intr_2_..._sprites` (§3.5) |
| serial | 0/1 | `boot_sclk_align` (needs boot timing, out of scope) |
| root | 30/41 | only the boot class left (§3.8); the control-flow read/write-timing cluster passes (§3.1) |

The remaining 12 failures are **all boot state** (11 boot tests + `boot_sclk_align`):
they check the post-boot register/IO/DIV state of **specific models** — we only do
DMG and don't run a real boot ROM, so they are out of scope (the real DMG variants
`*-dmgABC` pass). See §3.8.

### 2.4 mealybug tearoom (DMG) — 1 / 24 pass, per-pixel similarity quantified

The committed comparison scaffold `tools/mealybug_compare.py` (pure-stdlib zlib PNG
decode, polarity `3 - gray`; reads the `*_dmg_blob.png` colocated with each ROM in
the bundle, zero committed data):

```sh
GB_TEST_ROMS=/path/to/game-boy-test-roms tools/mealybug_compare.py [name-substr]
# prints per-test similarity + a pixel-perfect count (optional name-substr runs a subset)
```

**PASS needs 100%.** Currently:

| Similarity | Test |
|---|---|
| **100%** ✅ | `m2_win_en_toggle` |
| 99%+ | `m3_wx_4_change_sprites` (99.96), `m3_scx_high_5_bits` (99.64), `m3_lcdc_obj_en_change` (99.37), `m3_lcdc_obj_size_change_scx` (99.18), `m3_wx_4_change` (99.01) |
| 95–99% | `m3_window_timing_wx_0`, `m3_obp0_change`, `m3_lcdc_obj_size_change`, `m3_scx_low_3_bits`, `m3_wx_5_change`, `m3_lcdc_bg_map_change`, `m3_lcdc_obj_en_change_variant` |
| 88–94% | `m3_lcdc_win_map_change`, `m3_window_timing`, `m3_lcdc_tile_sel_win_change`, `m3_lcdc_tile_sel_change`, `m3_lcdc_bg_en_change` |
| < 80% | `m3_bgp_change` (78), `m3_bgp_change_sprites` (75), `m3_lcdc_win_en_change_multiple_wx` (74), `m3_lcdc_win_en_change_multiple` (64), `m3_scy_change` (58), `m3_wx_6_change` (40) |

Fixed the **WX<7 window left-clip** (§3.6), pushing `m3_wx_4_change` 56→99,
`m3_wx_5_change` 59→97, `m3_window_timing_wx_0` 96→99. Each of the rest is an
**independent dot-precise timing puzzle** (characterized one by one in §3.6);
mealybug is the strictest suite, and many mature emulators sit at single-digit
PASS counts for a long time.

### 2.5 Blargg `dmg_sound` (APU) — 9 / 12 pass

The APU suite reports via the **`$A000` memory protocol** (signature `DE B0 61`
at `$A001-3`, status at `$A000`), not serial — `common::blargg_ram_status`. The
`rom_suite` test `blargg_dmg_sound_known_passing` ratchets the passing set so it
can't regress.

| Subtest | Result |
|---|---|
| 01-registers | ✅ |
| 02-len ctr | ✅ |
| 03-trigger | ✅ length extra-clock on enable (§3.9) |
| 04-sweep | ✅ |
| 05-sweep details | ✅ sweep negate-mode disable (§3.9) |
| 06-overflow on trigger | ✅ |
| 07-len sweep period sync | ✅ |
| 08-len ctr during power | ✅ length preserved + NRx1 writable while off (§3.9) |
| 11-regs after power | ✅ |
| 09-wave read while on | ❌ wave-RAM read while the channel is playing |
| 10-wave trigger while on | ❌ wave-RAM state on trigger while on |
| 12-wave write while on | ❌ wave-RAM write while the channel is playing |

Went 5→9 by implementing the length-counter obscure behaviour (extra clock when
length is enabled in the first half of a period; preservation across power-off;
NRx1 length-load writable while powered off on DMG), the sweep negate-mode
disable, and NR41-after-power (§3.9). The 3 that remain are the **wave-channel
RAM access** quirks (09/10/12) — reading/triggering/writing wave RAM while CH3
is playing hits the byte the channel is currently reading, within a tight
timing window. That needs cycle-exact wave-read timing and is the hardest APU
cluster; not yet attacked.

---

## 3. Open issues and key fixes

In one line: **every non-boot mooneye test passes** — the acceptance tests at both
M-cycle and dot/T granularity are in place. What's left is the boot class (out of
scope) and mealybug (the **exact latch dot** of mid-mode-3 effects, the strictest
suite). Below, per subsystem: how it was fixed / what was learned, so the method
can be reused for the mealybug push.

### 3.1 Control-flow read/write timing + `ie_push` (bus/interrupts)
These mooneye timing tests use **OAM DMA as an oscilloscope**: point the stack into
OAM, or fetch instructions from ROM, then use the DMA window to straddle a boundary.
The **write** direction was fixed with a start delay + dropping writes inside the
window.

The **read** direction's root cause was located with trace + disassembly:
`ret_timing`'s RET is in **ROM** (0x0192), and the DMA source is **$80 (VRAM)**. On
real hardware **the DMA only occupies the one bus that conflicts with its source**:
a VRAM source occupies the **video bus** (VRAM+OAM), and the CPU can still read the
**external bus** (ROM) — so the RET fetch succeeds and only the OAM pop is blocked.
We had been blocking `addr < 0xFEA0` blindly (same for all sources), so the ROM
fetch was blocked too → read `$FF` = RST 38 → ran off the rails. Switching to
per-source bus blocking (`dma_conflicts`, only OAM always locked) turned the whole
**cluster of 9 read-timing tests green**, with no regression in the oam_dma group.

`ie_push`: when the interrupt dispatch pushes the return address's high byte, if
`SP=0` that byte lands on **IE (0xFFFF)** and rewrites the enable bits, so the vector
is chosen from the IE value **after** the push — a dispatch begun for the Timer can
land on the VBlank vector. Implemented in `service_interrupt` as "push high byte →
re-sample IE&IF → pick the vector" (unit test `test_ie_push_...`).

### 3.2 `rapid_toggle` — fixed: the TAC store lands on T3, replayed inside the timer
Quantified with `inspect`: real hardware services the timer interrupt at **BC=FFD9**,
we were one loop late (FFD8). Hand-tracing the whole ROM's glitch-increment table
pinned the missing one: on iteration 29 the **enable** write lands with the counter
at 2047 (bit 9 about to fall) — real hardware's write lands on **T3** (*before* that
M-cycle's last tick), so the just-enabled input sees the 2047→2048 falling edge and
TIMA increments; our write on T4 (counter already 2048) misses it. With that added,
the 16th increment moves earlier to iteration 37's disable → exactly FFD9.

Fix: **don't move the global write position** (moving it to T3 empirically regressed
`call/push/rst/call_cc2`, proving the bus write really is T4); instead **replay the
hardware order** inside `Timer::write_register(FF07)` — record the pre-increment
counter (`prev_counter`), compute the write as of the previous counter with the edge
re-evaluated under the new TAC on the last tick, and reconcile against the "T4-old-TAC"
edge already counted. A phase-scan experiment (no offset 0/±1/… satisfies both
rapid_toggle and tim*) confirms it: the ordinary increment phase was already right;
only the **observation point of the TAC write** needed to move one tick earlier.
Timer group 13/13.

Pitfall: the replay first reconstructed "one tick earlier" as `counter-1`, which for
a cold call (a unit test calling `write_register` directly with no prior tick) wraps
around to a phantom high bit of 0xFFFF → two timer unit tests failed; recording the
real `prev_counter` fixed it cleanly.

### 3.3 The constant-tick offset in STAT-mode reads (the intr_2 cluster) — fixed
A full "disprove → locate → fix" cycle, worth recording as a method:

1. `ppu_probe` measured the PPU's mode2-int / mode3 / mode0 landing points at the
   textbook values **dot 0 / 80 / 252** — **the PPU internal timing was not off**.
2. It was assumed a fix required stepping the **whole CPU T-by-T**. **Empirically
   disproved**: a full cycle-stepped core was implemented (`tick_t` looping
   `bus.tick(1)` + a timer guard hook + T-cycle HALT wake), yet it changed nothing
   for the running case, `intr_2` still all failed, and the T-cycle HALT wake even
   regressed `hblank`. **Conclusion: the CPU structure is not the bottleneck.**
3. Key observation: `intr_2_0_timing` (which tests the mode-2 interrupt itself)
   **passes**, while `intr_2_mode0/mode3/oam_ok` (which test from the mode-2
   interrupt to "STAT reads modeX / OAM accessible") **fail**. So the offset is in
   **when software observes the mode**, not in the interrupt, not in the internal
   transition. `hblank` (a relative measurement) is immune to a constant offset, so
   it always passed.
4. **Fix**: on real hardware the STAT register's mode bits, and the OAM/VRAM locks,
   lag the internal mode transition by **~4 dots** (only 1 dot out of Drawing, see
   §3.5). Add `prev_mode` / `transition_age`, and use `visible_mode()` for STAT reads
   and OAM-access decisions — `intr_2_mode0/mode3/oam_ok` all green, acid2
   byte-identical, no regression.

Also fixed in the same cluster: **`hblank_ly_scx` + `intr_2_0`** — mode 3 had been
only 167 dots (bare FIFO warmup) vs the hardware's 172; adding a **fixed fetch-startup
stall** makes mode 3 = 172 + (SCX&7) + sprite/window penalties, moving the HBlank
start back into place (pixels unchanged). **`vblank_stat_intr`** — entering VBlank at
line 144 also fires the mode-2 STAT interrupt, so line 144 is folded into the STAT
line condition. **`stat_lyc_onoff`** — three things: ① the first line after enable
runs mode 0 (no OAM scan), so the first STAT read after enable is mode 0; ② while the
LCD is off the scan stops, so only the **frozen LYC-match bit** can hold the STAT line
up, hence off-LCD keeps `stat_line = match & bit6`, and re-enabling is a real rising
edge that fires; ③ an enable/LYC write that causes a rising edge fires the STAT
interrupt **immediately** (before the next `DI`).

Lesson: don't jump to a big rewrite; first **quantify** the offset with probe/trace —
it is often a single constant tick in a peripheral.

### 3.4 `lcdon_timing-GS` + `lcdon_write_timing-GS` — fixed: dot-precise first line
The project's most thorough "decode the expectations from the test source" calibration
so far: each test has 3 passes offset by 1 nop (=4T), giving **4-dot resolution** over
157 expected values; decoding the `.s` expectation tables value by value back-derives
a self-consistent model, then `inspect dumpat` dumps the test's own result buffer to
compare against the table — a clean sweep on the first try:

- **First line after enable (line 0)**: length **452** (LY=1 lands on dot 452); no
  OAM scan (a fake mode 0), drawing starts at dot 80; and this line's mode transitions
  have **no visible lag** (internal == visible, this line only).
- **Asymmetric OAM/VRAM locks**: **reads lock on the internal edge** (scan/fetch takes
  the bus at once) and **unlock on the visible edge**; **writes see the visible mode
  only** — which naturally produces the **write-through windows** at line-start dots
  0..3 and at the mode2→3 handoff dots 80..83 (a measured hardware behaviour the
  expectation table pins down).
- **LYC-match line-start blank**: after LY changes the match reads 0 for the first 4
  dots, re-latching the comparison at dot 4.

### 3.5 `intr_2_mode0_timing_sprites` — fixed: penalty aggregation + fetcher bubble + visible lag
One test pins down three defects. Its ~100 testcases each encode `floor(total_penalty/4)`;
verifying them one by one back-derives:

1. **Penalty aggregation**: at a given **OAM X**, only the first sprite pays the
   BG-fetch abort (`11 - min(5,(x+SCX)%8)`, X=0 always 11); further sprites at the
   same X pay only the 6-dot fetch (10 stacked at X=0 = 5 + 6×10 = 65, not 110). Key
   on OAM X, not the trigger pixel: X=0 and X=8 both trigger at pixel 0 yet each pays
   in full.
2. **Pause determinism**: during a sprite pause the BG fetcher **keeps running** (the
   penalty formula already prices in its lost progress), so mode 3 stretches by exactly
   the penalty; the old "frozen fetcher" model leaked 1–2-dot emergent bubbles.
3. **Every-dot push retry**: the fetcher's final push needs no memory access and retries
   every dot (odd penalties 11/7 used to leave a 1-dot parity bubble); the baseline
   warmup becomes 5→6 accordingly, keeping the no-sprite line at 172.
4. **The Drawing→HBlank STAT visible lag is 1 dot, not 4**: this test's odd penalties
   break the mod-4 sampling degeneracy every other test left (the rest are insensitive
   to that lag mod 4, so 4 was a coincidental solution).

The earlier conclusion "the measured line LY=68 has no sprite" was a **misread**: the
sprite at Y=$52 covers screen lines 66..73, so line 68 is included. `ppu_probe`'s 12
configurations (single / stacked / spread) all match the hardware table dot for dot.

### 3.6 mealybug tearoom — fixed WX<7 clipping; the rest characterized
`tools/mealybug_compare.py` replaced "0/24 by vibes" with **per-test similarity +
diff structure** (see §2.4).

**Fixed** (`fix(ppu): clip the window's left edge when WX < 7`): `m3_wx_5_change`'s
row dump showed our output was exactly the reference **shifted right by (7−WX) pixels**.
On real hardware, with WX<7 the window's left (7−WX) pixels fall off-screen and are
clipped; we didn't clip → the whole window shifts right. Reusing the SCX fine-scroll
discard, set `discard = 7 - WX` when the window activates. `m3_wx_4` 56→99, `m3_wx_5`
59→97 (pixel-aligned), `m3_window_timing_wx_0` 96→99; acid2 byte-identical, mooneye
ppu all green, `m2_win_en_toggle` still 100%.

**The rest, characterized** (each an independent dot-precise timing puzzle, not a
few-line fix, and touching rendering with regression risk):
- **Palette write latency + transient** (`m3_bgp_change` 78 / `_sprites` 75): triggered
  by a mode-2 STAT interrupt, writing BGP repeatedly at nop-spaced delays across a line.
  Our transition is **~7px later** than the reference, and the reference has an **isolated
  intermediate-shade pixel** at the write's landing dot (the famous DMG BGP write-vs-push
  glitch) that we don't model. The OBJ version `m3_obp0_change` is already 98%; this
  BG/OBJ path difference is a hardware feature. The 78% is **not a regression** (same
  score before/after the related changes).
- **Exact window-trigger dot** (`m3_wx_6_change` 40 / `win_en_multiple` 64): on some
  lines the reference shows background while we show the window — WX rewritten on the
  exact comparison tick should **suppress** the trigger; we lack that dot-precise
  comparison timing.
- **Coarse-scroll sample point** (`m3_scx_high_5_bits` 99.64, a single tile column
  x16-23): SCX high bits changed mid-line, off by one tile's fetch timing.
- **A single sprite/window-edge pixel** (`m3_wx_4_change_sprites` 99.96, only 10 px):
  the single sprite pixel bleeding through the WX=4 window edge is dropped.
- **Mid-line SCY change** (`m3_scy_change` 58): changing SCY mid-line affects which
  tile row is fetched, off over a large area.

### 3.7 Reverted experiments (lessons)
Several attempts at T-cycle precision were pushed back by the ratchet — recorded so as
not to repeat them:
- **Byte-by-byte DMA conflict read**: assumed reading the external bus during DMA
  returned the "in-flight byte"; empirically **regressed `oam_dma_start/restart/timing`**,
  proving DMG reads `$FF` open bus. Reverted.
- **T-cycle HALT wake** (per-T polling): fixed nothing for intr_2 and **regressed a
  HALT-timing test** (HALT became variable-length, breaking cycle counts elsewhere).
  The interrupt path must change as a whole, not just the wake.
- **Early lcdon first-line model** (mode 3 at dot 82): guessed the dot offset wrong, and
  the change broke a local `ppu_test` assumption of "mode 2 immediately on enable" —
  §3.4 later redid it with the expectation tables.
- **Moving the write position to T3** (tick3-write-tick1): tried to crack rapid_toggle /
  lcdon_write together, but **regressed `call/push/rst/call_cc2`** and didn't fix
  rapid_toggle. Proves the SM83 write really lands on **T4**; the existing "tick(4) then
  write" is correct, leave it (rapid_toggle is fixed by replaying inside the timer, §3.2).

### 3.8 Boot state (out of scope)
`boot_regs`/`boot_div`/`boot_hwio`'s `dmg0/mgb/sgb/sgb2` variants and `boot_sclk_align`
check specific models' post-boot state; we only do DMG and don't run a real boot ROM.
The real DMG variants (`*-dmgABC`) pass.

### 3.9 APU obscure corners (`dmg_sound` 9/12)

**Fixed (5→9):**
- **Length extra-clock on enable (03)** — writing NRx4 with the length-enable
  bit going 0→1 while the frame sequencer is in the *first half* of a length
  period (its next step won't clock length) clocks the length counter once
  immediately; if that hits 0 and it's not a trigger, the channel disables. A
  trigger that reloads length to max in that same phase then clocks it once too.
  `length_enable_write` + `Apu::length_first_half` (length clocks on FS steps
  0/2/4/6, so the first half is the odd steps), applied at all four NRx4 writes.
- **Sweep negate-mode disable (05)** — once a sweep calc has run in negate mode
  since the last trigger (`sweep_neg_used`), clearing the negate bit via NR10
  disables the channel.
- **Power edges (08/11)** — on DMG the length *counters* survive a power-off
  (`power_off` saves/restores them), and the NRx1 length-load registers are
  writable while powered off (only their length field, not duty).

Each of the four is now pinned by a gray-box `tests/apu_test.rs` case that drives
the registers and reads channel-enable back from NR52 — so they're covered by a
plain `cargo test`, not only the env-gated Blargg ROM. Each was checked to fail
if its fix is reverted.

**Remaining (09/10/12 — wave-channel RAM access while on):** while CH3 is
playing, CPU access to wave RAM doesn't hit the addressed byte — it hits the
byte the channel is *currently* reading, and only inside a tight window around
that read (else read returns 0xFF / write is dropped); a trigger while on
corrupts the first bytes in a set pattern. Our wave channel exposes the RAM as a
plain array regardless of play state. This is the hardest cluster — it needs
cycle-exact modelling of the wave read position and its access window — and is
not yet attacked. The `blargg_dmg_sound_known_passing` ratchet guards the 9 that
pass while this stays open.

### M-cycle ↔ sub-cycle spectrum

```
Passing (incl. those won by targeted calibration)   Still failing
──────────────────────────────────────────────────┼────────────────────────────────────────────
Blargg cpu_instrs/instr/mem_timing                  boot_* (need a real boot ROM / other model, out of scope)
dmg-acid2                                           boot_sclk_align (same)
mooneye: bits/instr/interrupts                      mealybug (23/24; the exact latch dot of mid-mode-3 effects)
oam_dma group; control-flow read/write timing; ie_push
timer group (incl. rapid_toggle: TAC write T3 replay)
PPU group: mode-3 length (172 + penalty aggregation)
  hblank_ly_scx / vblank_stat_intr
  stat_lyc_onoff / the whole intr_2 cluster
  lcdon_* (first line 452 dots + asymmetric locks)
  OBJ penalty aggregation (first pays full, same-X rest pay 6)
  mealybug m2_win_en_toggle (1/24)
STAT-mode lag: 4 dots out of scan/blank, 1 dot out of Drawing
```

> **T-cycle migration status**: CPU time advancement has converged to a single
> T-cycle seam (`Cpu::tick_t`). Measurement shows the CPU's memory accesses already
> land on the correct last tick of the M-cycle (T4, so Blargg `mem_timing` passes and
> a global T3 write regresses), so the CPU's **accesses** are already T-accurate; the
> remaining sub-cycle landing points (rapid_toggle's TAC write, mealybug's latch dots)
> are modelled locally in the relevant peripheral as needed, rather than by stepping
> the whole CPU T-by-T.

---

## 4. Reproduce

```sh
# 1) Get the test ROMs (not distributed with the repo)
#    https://github.com/c-sp/game-boy-test-roms/releases  (this project uses v7.0)
export GB_TEST_ROMS=/path/to/game-boy-test-roms

# 2) Module unit tests (gray-box, no ROMs needed)
cargo test --release

# 3) ROM integration suite (mooneye non-boot all pass + Blargg)
cargo test --release -p rgametoy-core --test rom_suite

# 4) mealybug similarity scoreboard
tools/mealybug_compare.py            # all 24; add a name-substr to run a subset

# 5) Inspect a single ROM by hand
cargo run --release -p rgametoy-core --example run_serial  -- "$GB_TEST_ROMS/blargg/cpu_instrs/cpu_instrs.gb"
cargo run --release -p rgametoy-core --example run_mooneye -- "$GB_TEST_ROMS/mooneye-test-suite/acceptance/timer/tima_reload.gb"
```

> `MEALYBUG_DUMP_FB` overrides the command the scaffold uses to dump the framebuffer
> (for environments that must invoke the toolchain binary directly).
