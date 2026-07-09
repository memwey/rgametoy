# tools/

Local developer scaffolding — **not** part of the automated build or CI. These
help measure things the committed test suite can't self-certify. All are
env-gated on `GB_TEST_ROMS` (a [c-sp/game-boy-test-roms](https://github.com/c-sp/game-boy-test-roms)
bundle root, not vendored here) and read reference data that lives next to the
ROMs in that bundle.

## `mealybug_compare.py`

A per-pixel similarity scoreboard for the **mealybug-tearoom** PPU suite.
mealybug is *fuzzy* (each test is a similarity %, not pass/fail), so it can't be
a plain `cargo test` assertion the way mooneye / Blargg / dmg_sound are — hence a
manual scaffold. It runs each ROM through the `dump_fb` example, decodes the
`<name>_dmg_blob.png` reference beside it in the bundle (stdlib `zlib` only —
Rust std has no inflate, which is why this is Python), and prints similarity per
test plus a pixel-perfect count.

```sh
GB_TEST_ROMS=/path/to/game-boy-test-roms tools/mealybug_compare.py [name-substr]
```

- `name-substr` (optional) runs only tests whose name contains it.
- `MEALYBUG_DUMP_FB` overrides the framebuffer-dump command (default: a release
  `cargo run` of the `dump_fb` example; the command takes `<rom> <out.raw> <frames>`).

The scoreboard and the per-test analysis are tracked in
[`docs/testing.md`](../docs/testing.md) §2.4 — update it when the numbers move.
