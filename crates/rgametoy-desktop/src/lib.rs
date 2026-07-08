//! rgametoy-desktop — the native (desktop) frontend for the rgametoy emulator.
//!
//! The emulated machine lives in the [`rgametoy_core`] crate; this crate is the
//! host-facing frontend that drives it: the [`emulator`] module owns the window,
//! integer scaling, input mapping, audio backend, pacing, and save files. The
//! `rgametoy` binary ([`main`](../src/main.rs)) wires them together.
//!
//! Splitting the core out as its own dependency-free crate keeps it portable —
//! e.g. it compiles to `wasm32` for a future web frontend, which this desktop
//! crate (with its `minifb` / `cpal` dependencies) could not.

pub mod emulator;

// Façade re-export so `main` and examples can name the top-level type directly.
pub use emulator::Emulator;
