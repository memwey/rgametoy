# Embedded mealybug-tearoom-tests references

Each `*.bin` is the expected DMG output for the matching mealybug test ROM,
decoded from that test's `<name>_dmg_blob.png` reference image into our
framebuffer's shade values (0–3), then packed 4 pixels per byte (2 bits each,
MSB-first) — 160×144 → 5760 bytes.

These are used by `tests/rom_suite.rs` (the `mealybug_no_regression_vs_embedded_refs`
test) so the pixel comparison is self-contained and needs no PNG decoder. The
test ROMs themselves are **not** vendored — point `GB_TEST_ROMS` at a
game-boy-test-roms bundle to run the suite.

Source: [mealybug-tearoom-tests](https://github.com/mattcurrie/mealybug-tearoom-tests)
by Matt Currie, MIT licensed. These files are a mechanical re-encoding of that
project's reference images and carry the same license.

To regenerate after updating the references: decode each `*_dmg_blob.png`,
map gray→shade as `3 - gray`, and pack 2bpp (see the generator recipe in the
commit that introduced this directory).
