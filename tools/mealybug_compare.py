#!/usr/bin/env python3
"""Local mealybug-tearoom scoreboard — no committed data needed.

mealybug is a fuzzy (per-pixel similarity) PPU suite, so it is not part of the
automated `tests/rom_suite.rs`. This scaffold runs each test ROM through the
`dump_fb` example, decodes the reference image that sits next to it in the
bundle, and prints a similarity scoreboard.

Usage:
    GB_TEST_ROMS=/path/to/game-boy-test-roms tools/mealybug_compare.py [name-substr]

The reference PNGs live beside the ROMs (`<name>_dmg_blob.png`), so nothing is
vendored. Decoding uses only the Python standard library (zlib) — the reason
this is a Python tool and not part of the Rust suite (std has no inflate).

The command that dumps our framebuffer defaults to a release `cargo run` of the
`dump_fb` example; override it with the MEALYBUG_DUMP_FB env var (a shell
command taking `<rom> <out.raw> <frames>`), e.g. when cargo must be invoked
through a specific toolchain binary.
"""
import os
import shlex
import struct
import subprocess
import sys
import tempfile
import zlib

W, H = 160, 144
PIXELS = W * H


def decode_png(path):
    """Decode a grayscale (bit-depth 1 or 2) PNG to a list of shade values 0-3,
    in our framebuffer orientation (shade = 3 - gray)."""
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", path
    i, bd, ct, idat = 8, 0, 0, b""
    while i < len(data):
        (ln,) = struct.unpack(">I", data[i : i + 4])
        typ = data[i + 4 : i + 8]
        body = data[i + 8 : i + 8 + ln]
        if typ == b"IHDR":
            _, _, bd, ct = struct.unpack(">IIBB", body[:10])
        elif typ == b"IDAT":
            idat += body
        i += 12 + ln
    assert ct == 0, f"{path}: not grayscale (colour type {ct})"
    raw = zlib.decompress(idat)
    stride = (W * bd + 7) // 8
    maxv = (1 << bd) - 1
    prev = bytearray(stride)
    pos = 0
    px = []
    for _ in range(H):
        f = raw[pos]
        pos += 1
        line = bytearray(raw[pos : pos + stride])
        pos += stride
        for x in range(stride):
            a = line[x - 1] if x >= 1 else 0
            b = prev[x]
            c = prev[x - 1] if x >= 1 else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + ((a + b) >> 1)) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        prev = line
        ppb = 8 // bd
        for x in range(W):
            g = (line[x // ppb] >> (8 - bd - bd * (x % ppb))) & maxv
            px.append(3 - (g * 3 // maxv if maxv else 0))
    return px


def main():
    root = os.environ.get("GB_TEST_ROMS")
    if not root:
        sys.exit("set GB_TEST_ROMS to a game-boy-test-roms bundle root")
    mbdir = os.path.join(root, "mealybug-tearoom-tests", "ppu")
    if not os.path.isdir(mbdir):
        sys.exit(f"no mealybug ROMs under {mbdir}")
    dump = shlex.split(
        os.environ.get(
            "MEALYBUG_DUMP_FB",
            "cargo run --release --quiet --example dump_fb --",
        )
    )
    want = sys.argv[1] if len(sys.argv) > 1 else ""

    names = sorted(
        n[: -len("_dmg_blob.png")]
        for n in os.listdir(mbdir)
        if n.endswith("_dmg_blob.png") and want in n
    )
    rows = []
    with tempfile.TemporaryDirectory() as tmp:
        raw = os.path.join(tmp, "fb.raw")
        for n in names:
            rom = os.path.join(mbdir, f"{n}.gb")
            subprocess.run(dump + [rom, raw, "30"], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            ours = open(raw, "rb").read()
            ref = decode_png(os.path.join(mbdir, f"{n}_dmg_blob.png"))
            m = sum(1 for i in range(PIXELS) if ref[i] == ours[i])
            rows.append((n, m))

    rows.sort(key=lambda r: r[1])
    for n, m in rows:
        flag = "  PASS" if m == PIXELS else ""
        print(f"{100 * m / PIXELS:6.2f}%  {m:5}/{PIXELS}  {n}{flag}")
    passed = sum(1 for _, m in rows if m == PIXELS)
    print(f"\n{passed}/{len(rows)} pixel-perfect")


if __name__ == "__main__":
    main()
