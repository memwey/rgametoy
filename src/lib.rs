//! rgametoy — a DMG Game Boy emulator.
//!
//! The crate is split into two top-level modules that mirror the hardware /
//! host boundary:
//!
//! - [`console`]: the emulated Game Boy (CPU, PPU, APU, timer, cartridge, …).
//!   Deterministic and free of host I/O.
//! - [`emulator`]: the host-facing frontend (window, scaling, input mapping,
//!   audio backend, pacing, save files) that drives the console.

pub mod console;
pub mod emulator;

// Façade re-exports for the two top-level types.
pub use console::Console;
pub use emulator::Emulator;
