# Agent Guidelines for rgametoy

A cycle-accurate **DMG** (original Game Boy) emulator in Rust. Accuracy over
features; the emulated machine is a dependency-free library that also compiles to
`wasm32`, with native and web frontends on top.

## Requirements
* Accuracy first — mirror real DMG hardware behaviour as closely as practical.
* Timing: aim for **T-cycle** accuracy. In practice the CPU's memory accesses
  already land on the correct M-cycle tick (T4); the finer sub-cycle landing
  points (e.g. a TAC write, mid-mode-3 PPU register latches) are modelled
  **locally in the peripheral that needs them**, not by stepping the whole CPU
  T-by-T. See `docs/testing.md` §3.
* **DMG model only** — no CGB / SGB / MGB behaviours.
* **The core has no third-party dependencies** — implement the emulation itself.
  (Frontends *may* use crates: minifb, cpal, wasm-bindgen / web-sys.)
* A well-structured, rigorous architecture.

## Project Layout (Cargo workspace)
* `crates/rgametoy-core` — the emulated DMG machine. Deterministic, no host I/O,
  no third-party deps, compiles to `wasm32`. **Keep it to the machine**: don't
  put logic that isn't part of the DMG hardware in here, except behind a
  clearly-declared feature. Current exceptions: `debug` (an in-process machine
  inspector) and `serialize` (whole-machine save-states / instant save-load). A
  host concern like a save-file content hash (`rom_hash`) stays in the frontends.
* `crates/rgametoy-desktop` — native frontend: minifb window, optional cpal audio
  (`audio` feature). Builds the `rgametoy` binary.
* `crates/rgametoy-web` — browser frontend: bare wasm-bindgen / web-sys (no UI
  framework), Trunk build, cdylib + rlib.

Shared contracts belong in the lowest crate both consumers depend on (`core`),
**not** a grab-bag `util` crate — e.g. the joypad button bits `set_buttons`
consumes live in `core::joypad` and both frontends `pub use` them.

## Testing & the ratchet
Accuracy is measured with community test ROMs. `docs/testing.md` (EN) and
`docs/testing_cn.md` (CN) hold the scoreboard and per-test analysis and are the
**source of truth** — update them when scores move.

* Fast gray-box unit tests, no ROMs needed: `cargo test`.
* Black-box ROM suites are env-gated on `GB_TEST_ROMS` (a
  [c-sp/game-boy-test-roms](https://github.com/c-sp/game-boy-test-roms) bundle
  root; not committed): `GB_TEST_ROMS=… cargo test --release -p rgametoy-core --test rom_suite`.
  Asserts mooneye acceptance (every non-boot test passes), Blargg cpu/timing, and
  Blargg `dmg_sound`'s passing subtests.
* The **mealybug-tearoom** PPU suite is fuzzy (per-pixel similarity, not
  pass/fail), so it's a manual scaffold rather than an automated test — see
  `tools/README.md`.
* **Never regress a passing test.** Before finishing a change, keep these green:
  1. `cargo clippy -p rgametoy-core --all-targets`
  2. `cargo clippy --workspace --all-targets --all-features`
  3. `cargo clippy -p rgametoy-web --target wasm32-unknown-unknown -- -D warnings`

  plus `cargo test` **and** `cargo test --all-features` — the save-state tests
  are behind the `serialize` feature, so a plain `cargo test` compiles and runs
  zero of them and would miss a serialization regression. Also run the ROM suite
  when the change could affect accuracy. A new accuracy fix should add a targeted
  test and ratchet the now-passing ROM tests so they can't silently regress.

## Code Style
* Idiomatic Rust; match the surrounding code's naming, comment density, and idiom.
* Keep EN and CN docs in sync — docs come in `<name>.md` / `<name>_cn.md` pairs.
