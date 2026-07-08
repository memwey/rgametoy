//! Minimal, dependency-free frontend logging. Three levels — `info` → stdout,
//! `warn` / `error` → stderr — each with a short tag, plus `heading` / `field`
//! for the startup banner. Colour (plain ANSI, no crate) is emitted only when
//! the destination is a terminal and `NO_COLOR` is unset, so piped or
//! redirected output stays clean.
//!
//! Emulated serial output is deliberately NOT routed through here: a test ROM's
//! text prints raw (see `Emulator::run`), so it never gets our tags or colours.

use std::io::IsTerminal;
use std::sync::OnceLock;

// ANSI SGR codes. Kept tiny; only what the tags below use.
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";

/// Colour is on only when `NO_COLOR` is unset *and* the stream is a real
/// terminal. Decided once per stream (a process keeps the same stdio).
fn color(stream_is_tty: bool) -> bool {
    stream_is_tty && std::env::var_os("NO_COLOR").is_none()
}

fn stdout_color() -> bool {
    static C: OnceLock<bool> = OnceLock::new();
    *C.get_or_init(|| color(std::io::stdout().is_terminal()))
}

fn stderr_color() -> bool {
    static C: OnceLock<bool> = OnceLock::new();
    *C.get_or_init(|| color(std::io::stderr().is_terminal()))
}

/// Informational line (a running-state event) → stdout.
pub fn info(msg: &str) {
    if stdout_color() {
        println!("{CYAN}·{RESET} {msg}");
    } else {
        println!("· {msg}");
    }
}

/// A recoverable problem (e.g. a save that couldn't be read) → stderr.
pub fn warn(msg: &str) {
    if stderr_color() {
        eprintln!("{YELLOW}warning:{RESET} {msg}");
    } else {
        eprintln!("warning: {msg}");
    }
}

/// A failure the user should see → stderr.
pub fn error(msg: &str) {
    if stderr_color() {
        eprintln!("{RED}error:{RESET} {msg}");
    } else {
        eprintln!("error: {msg}");
    }
}

/// A section heading for the startup banner (bold when colour is on) → stdout.
pub fn heading(title: &str) {
    if stdout_color() {
        println!("{BOLD}{title}{RESET}");
    } else {
        println!("{title}");
    }
}

/// An aligned `key  value` field line under a [`heading`] → stdout. The key is
/// dimmed and right-aligned so a block of fields lines up.
pub fn field(key: &str, value: &str) {
    if stdout_color() {
        println!("  {DIM}{key:>9}{RESET}  {value}");
    } else {
        println!("  {key:>9}  {value}");
    }
}
