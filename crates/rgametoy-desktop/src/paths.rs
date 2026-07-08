//! Where the frontend keeps per-user files. One base data directory
//! (`$RGAMETOY_DATA_DIR`, default the working directory) holds two subfolders:
//!
//! ```text
//! <data-dir>/
//! ├── saves/        <rom-stem>-<rom-hash>.sav   (battery SRAM)
//! └── screenshots/  <rom-title>-<timestamp>.bmp (the "2" key captures)
//! ```
//!
//! A save is raw SRAM (portable across emulators), so it carries no ROM
//! identity of its own — the *name* is the link. We name it after the ROM's
//! file stem (readable) plus an 8-hex content hash (so two different ROMs that
//! happen to share a filename never clash, and the hash records exactly which
//! ROM the save belongs to).

use std::path::{Path, PathBuf};

/// Base data directory: `$RGAMETOY_DATA_DIR`, or the working directory.
pub fn data_dir() -> PathBuf {
    std::env::var_os("RGAMETOY_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `<data-dir>/saves`.
pub fn saves_dir() -> PathBuf {
    data_dir().join("saves")
}

/// `<data-dir>/screenshots`.
pub fn screenshots_dir() -> PathBuf {
    data_dir().join("screenshots")
}

/// FNV-1a 32-bit hash of the ROM bytes, as 8 lowercase hex digits — the
/// content-identity half of a save-file name. Dependency-free and stable.
pub fn rom_hash(rom: &[u8]) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for &b in rom {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

/// The save-file name for a ROM: `<sanitised file stem>-<content hash>.sav`.
pub fn save_name(rom_path: &Path, rom_bytes: &[u8]) -> String {
    let stem = rom_path.file_stem().and_then(|s| s.to_str()).unwrap_or("rom");
    format!("{}-{}.sav", sanitize(stem), rom_hash(rom_bytes))
}

/// Reduce an arbitrary string (ROM title or file stem) to a safe, readable
/// file-name stem: keep ASCII alphanumerics / `-` / `_`, replace the rest with
/// `_`, and fall back to `rgametoy` if nothing is left.
pub fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "rgametoy".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_hash_is_stable_and_content_addressed() {
        assert_eq!(rom_hash(b"hello world").len(), 8);
        assert_eq!(rom_hash(b"same"), rom_hash(b"same"));
        assert_ne!(rom_hash(b"rom A"), rom_hash(b"rom B"));
    }

    #[test]
    fn save_name_combines_readable_stem_and_hash() {
        let name = save_name(Path::new("/games/Pokemon Red.gb"), b"\x01\x02\x03");
        // "Pokemon Red" -> "Pokemon_Red", plus "-<8 hex>.sav".
        assert!(name.starts_with("Pokemon_Red-"), "got {name}");
        assert!(name.ends_with(".sav"));
        assert_eq!(name.len(), "Pokemon_Red-".len() + 8 + ".sav".len());
    }

    #[test]
    fn save_name_differs_for_same_filename_different_content() {
        let a = save_name(Path::new("game.gb"), b"content A");
        let b = save_name(Path::new("game.gb"), b"content B");
        assert_ne!(a, b, "same filename, different ROM must not collide");
    }

    #[test]
    fn sanitize_falls_back_and_strips() {
        assert_eq!(sanitize("Tetris"), "Tetris");
        assert_eq!(sanitize("SUPER MARIO"), "SUPER_MARIO");
        assert_eq!(sanitize(""), "rgametoy");
        assert_eq!(sanitize("\0\0"), "rgametoy");
    }
}
