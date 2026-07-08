//! Real-time frame pacing for the rAF loop — pure arithmetic, no browser API,
//! so it lives apart from `wasm_host`'s DOM glue and is unit-tested directly.
//!
//! The loop paces emulation against elapsed *real time*, not the display
//! refresh: a fixed one-frame-per-rAF would run the game 2× on a 120 Hz panel.

/// One DMG frame in milliseconds (4.194304 MHz / 70224 dots ≈ 59.7275 Hz).
pub(crate) const DMG_FRAME_MS: f64 = 70224.0 / 4_194_304.0 * 1000.0;

/// Cap on the real time a single tick may consume, so a long stall (e.g. a
/// backgrounded tab) is absorbed instead of triggering a catch-up avalanche.
pub(crate) const MAX_CATCHUP_MS: f64 = 100.0;

/// Hard cap on emulated frames run per rAF tick at 1× (belt-and-braces with the
/// catch-up clamp above). Fast-forward passes a larger cap.
pub(crate) const MAX_FRAMES_PER_TICK: u32 = 4;

/// Given the accumulator and this tick's already-clamped elapsed real time
/// `dt_ms`, return how many ~59.7 Hz emulated frames to run and the leftover
/// accumulator, capped at `max_frames`. Runs frames per *real time*, not per
/// refresh — that's what keeps a 120 Hz display from running 2× at 1×, and what
/// makes fast-forward a fixed multiple of real time (pass `dt_ms * multiplier`)
/// rather than a refresh-rate-dependent frames-per-repaint burst.
pub(crate) fn frames_to_run(accum_ms: f64, dt_ms: f64, max_frames: u32) -> (u32, f64) {
    let mut accum = accum_ms + dt_ms;
    let mut n = 0;
    while accum >= DMG_FRAME_MS && n < max_frames {
        accum -= DMG_FRAME_MS;
        n += 1;
    }
    (n, accum)
}

#[cfg(test)]
mod tests {
    use super::{frames_to_run, DMG_FRAME_MS, MAX_FRAMES_PER_TICK};

    #[test]
    fn one_dmg_frame_of_elapsed_runs_one_frame() {
        let (n, accum) = frames_to_run(0.0, DMG_FRAME_MS, MAX_FRAMES_PER_TICK);
        assert_eq!(n, 1);
        assert!(accum.abs() < 1e-9, "no leftover, got {accum}");
    }

    #[test]
    fn a_short_tick_runs_nothing_but_accumulates() {
        // A 60 Hz tick (16.667 ms) is just under one DMG frame (16.743 ms), so
        // it runs 0 frames and carries the remainder — the next tick runs 1.
        let (n, accum) = frames_to_run(0.0, 1000.0 / 60.0, MAX_FRAMES_PER_TICK);
        assert_eq!(n, 0);
        assert!(accum > 16.0, "carried the elapsed time, got {accum}");
    }

    #[test]
    fn catch_up_is_capped() {
        // 10 frames' worth of elapsed time in one tick is clamped to the cap.
        let (n, _) = frames_to_run(0.0, DMG_FRAME_MS * 10.0, MAX_FRAMES_PER_TICK);
        assert_eq!(n, MAX_FRAMES_PER_TICK);
    }

    /// The regression that motivated the accumulator: emulation must run at the
    /// DMG's ~59.7 Hz for *one real second* regardless of the display refresh —
    /// a fixed one-frame-per-rAF ran the game 2× on a 120 Hz panel.
    #[test]
    fn paces_to_dmg_rate_regardless_of_refresh() {
        for hz in [60.0_f64, 120.0, 144.0] {
            let dt = 1000.0 / hz;
            let mut accum = 0.0;
            let mut total = 0u32;
            for _ in 0..(hz as u32) {
                // one real second of ticks
                let (n, a) = frames_to_run(accum, dt, MAX_FRAMES_PER_TICK);
                total += n;
                accum = a;
            }
            assert!(
                (total as i32 - 60).abs() <= 1,
                "{hz} Hz ran {total} frames/s (expected ~59.7, not ~{hz})"
            );
        }
    }

    /// Fast-forward is a fixed multiple of real time, not a per-repaint burst:
    /// at 4× it runs ~4×59.7 emulated frames per real second on any refresh
    /// rate, given a per-tick cap high enough not to bind.
    #[test]
    fn fast_forward_is_a_fixed_multiple_regardless_of_refresh() {
        for hz in [60.0_f64, 120.0, 144.0] {
            let dt = 1000.0 / hz;
            let mut accum = 0.0;
            let mut total = 0u32;
            for _ in 0..(hz as u32) {
                let (n, a) = frames_to_run(accum, dt * 4.0, 12);
                total += n;
                accum = a;
            }
            assert!(
                (total as i32 - 239).abs() <= 2,
                "{hz} Hz at 4× ran {total} frames/s (expected ~239)"
            );
        }
    }
}
