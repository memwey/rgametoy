//! Save state serialization: error type, format constants, CRC32 and the
//! little-endian read/write helpers shared by every per-module
//! `write_state` / `read_state` pair.
//!
//! Terminology: a **save state** is this — a serialized image of the *whole
//! machine* as a portable byte blob (`Console::save_state_bytes` /
//! `load_state_bytes`, behind the opt-in `serialize` feature). It is distinct
//! from `debug::Snapshot` (behind the `debug` feature), which is a read-only
//! *view* of live state for the inspector and is never serialized. "Snapshot"
//! is reserved for
//! that debug view; the serialized form is always called a save state.
//!
//! Format:
//!
//! ```text
//! +---------------- header ----------------+
//! | magic "RGSV"        | 4 bytes         |
//! | version             | 1 byte          |
//! | CRC32(payload)      | 4 bytes LE      |
//! +---------------- body ------------------+
//! | total_cycles        | 8 bytes LE      |
//! | ... per-module state in fixed order   |
//! +--------------------------------------+
//! ```
//!
//! The CRC covers everything after the CRC field, so corruption is caught
//! before any partial state is applied. A version bump is reserved for
//! incompatible layout changes; reading future versions returns
//! `UnsupportedVersion` so an older build can refuse cleanly.

use std::fmt;

/// Errors that can occur while reading a save state blob.
#[derive(Debug, PartialEq, Eq)]
pub enum SaveStateError {
    /// The 4-byte magic at the head of the blob didn't match `"RGSV"`.
    BadMagic,
    /// The format version byte is one we don't know how to parse.
    UnsupportedVersion(u8),
    /// The blob is too short to contain the data the header says it does.
    Truncated,
    /// The CRC32 stored in the header doesn't match the recomputed payload.
    CrcMismatch,
    /// The cartridge kind tag is one we don't implement. Refuse rather than
    /// silently load an MBC1 emulation of e.g. an MBC2 game.
    UnknownCartridgeKind(u8),
    /// A length field held a value larger than the machine could ever produce
    /// (e.g. an APU sample count exceeding the buffer cap). Refuse rather than
    /// trust it — a crafted blob could otherwise overflow a size calc or force
    /// a huge allocation.
    Corrupt,
}

impl fmt::Display for SaveStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveStateError::BadMagic => f.write_str("save state: bad magic"),
            SaveStateError::UnsupportedVersion(v) => {
                write!(f, "save state: unsupported version {v}")
            }
            SaveStateError::Truncated => f.write_str("save state: blob truncated"),
            SaveStateError::CrcMismatch => f.write_str("save state: CRC mismatch"),
            SaveStateError::UnknownCartridgeKind(k) => {
                write!(f, "save state: unknown cartridge kind {k:#04x}")
            }
            SaveStateError::Corrupt => f.write_str("save state: corrupt (length out of range)"),
        }
    }
}

impl std::error::Error for SaveStateError {}

/// Current save state format version. Bump whenever the on-disk layout changes,
/// including when fields are appended: readers require the payload to be
/// consumed exactly so malformed or cross-version trailing data is rejected.
///
/// History: v2 = the 0xFF46 DMA register moved out of the PPU's serialized
/// register block (the DMA unit is owned by the system, not the PPU).
pub const SAVE_STATE_VERSION: u8 = 2;

/// File magic ("RGSV" = "rgametoy save"). Prepended to every blob.
pub const SAVE_STATE_MAGIC: [u8; 4] = *b"RGSV";

/// Standard CRC-32 with the IEEE polynomial (used by zip/ethernet/etc.),
/// built once as a const table and applied per byte.
const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB88320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

/// IEEE CRC-32 over `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = CRC32_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

// -- Little-endian writers --------------------------------------------------

pub fn write_u8(out: &mut Vec<u8>, v: u8) {
    out.push(v);
}

pub fn write_u16_le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub fn write_u32_le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub fn write_u64_le(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub fn write_bool(out: &mut Vec<u8>, v: bool) {
    out.push(v as u8);
}

pub fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
}

// -- Cursor reader ----------------------------------------------------------

/// Cursor over an input byte slice. Each per-module `read_state` advances
/// the cursor as it pulls fields. Truncation is reported as
/// [`SaveStateError::Truncated`].
pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn check(&self, n: usize) -> Result<(), SaveStateError> {
        if self.remaining() < n {
            Err(SaveStateError::Truncated)
        } else {
            Ok(())
        }
    }

    pub fn read_u8(&mut self) -> Result<u8, SaveStateError> {
        self.check(1)?;
        let v = self.bytes[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_u16_le(&mut self) -> Result<u16, SaveStateError> {
        self.check(2)?;
        let v = u16::from_le_bytes(self.bytes[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        Ok(v)
    }

    pub fn read_u32_le(&mut self) -> Result<u32, SaveStateError> {
        self.check(4)?;
        let v = u32::from_le_bytes(self.bytes[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    pub fn read_u64_le(&mut self) -> Result<u64, SaveStateError> {
        self.check(8)?;
        let v = u64::from_le_bytes(self.bytes[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }

    pub fn read_bool(&mut self) -> Result<bool, SaveStateError> {
        Ok(self.read_u8()? != 0)
    }

    /// Copy the next `n` bytes out of the cursor. The returned slice borrows
    /// from the source blob; per-module callers should `copy_from_slice`
    /// rather than hold the borrow.
    pub fn read_exact(&mut self, n: usize) -> Result<&'a [u8], SaveStateError> {
        self.check(n)?;
        let s = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}
