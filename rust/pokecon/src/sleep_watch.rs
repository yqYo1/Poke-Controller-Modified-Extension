//! PC resume-from-sleep detection (upstream `services/sleep_watch.py`).
//!
//! The runtime drives [`SleepWatch::check`] once per second. When the wall
//! clock shows a gap larger than the interval plus [`GAP_THRESHOLD`], the PC
//! was suspended (sleep/hibernate, or the clock was moved by hand): the caller
//! stops the running command, because the console kept operating while the PC
//! was away. The PC sleep settings are never read or changed.

/// How often the wall clock is checked (upstream `CHECK_INTERVAL`).
pub const CHECK_INTERVAL: f64 = 1.0;
/// Extra seconds beyond the check interval that still count as normal
/// scheduling jitter (upstream `GAP_THRESHOLD`).
pub const GAP_THRESHOLD: f64 = 20.0;

/// Detects that the PC was suspended between two checks.
#[derive(Debug)]
pub struct SleepWatch {
    interval: f64,
    threshold: f64,
    last: Option<f64>,
}

impl SleepWatch {
    #[must_use]
    pub fn new() -> Self {
        Self {
            interval: CHECK_INTERVAL,
            threshold: GAP_THRESHOLD,
            last: None,
        }
    }

    /// Checks the wall clock; returns the suspension time in seconds when the
    /// PC was away, or 0 for a normal tick (including the first check and a
    /// clock moved backwards).
    pub fn check(&mut self, now: f64) -> f64 {
        let last = self.last.replace(now);
        let Some(last) = last else {
            return 0.0;
        };
        let gap = now - last - self.interval;
        if gap <= self.threshold {
            return 0.0;
        }
        tracing::info!(
            gap_seconds = gap,
            "the PC seems to have been suspended (sleep watch)"
        );
        gap
    }
}

/// Wall-clock seconds for [`SleepWatch::check`]: sleep moves this clock even
/// where a monotonic source may pause.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "millisecond wall clock for gap detection"
)]
pub fn wall_clock_seconds() -> f64 {
    chrono::Local::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        reason = "the harness advances exact values, so gaps compare exactly"
    )]

    use super::*;

    struct Harness {
        now: f64,
        watch: SleepWatch,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                now: 1000.0,
                watch: SleepWatch::new(),
            }
        }

        fn tick(&mut self, seconds: f64) -> f64 {
            self.now += seconds;
            self.watch.check(self.now)
        }
    }

    #[test]
    fn normal_ticks_are_not_a_sleep() {
        let mut h = Harness::new();
        assert_eq!(h.watch.check(h.now), 0.0);
        for seconds in [1.0, 1.02, 0.98, 3.0, 20.9] {
            assert_eq!(h.tick(seconds), 0.0, "{seconds}");
        }
    }

    #[test]
    fn a_long_gap_is_a_sleep_reported_once_per_sleep() {
        let mut h = Harness::new();
        h.watch.check(h.now);
        h.tick(1.0);
        assert_eq!(h.tick(3601.0), 3600.0);
        // その後は、ふつうに戻る(同じスリープを 2 回知らせない)。
        assert_eq!(h.tick(1.0), 0.0);
        // 次のスリープは、また知らせる。
        assert_eq!(h.tick(3601.0), 3600.0);
    }

    #[test]
    fn the_first_check_and_a_clock_set_back_are_ignored() {
        let mut h = Harness::new();
        assert_eq!(h.watch.check(h.now), 0.0);
        assert_eq!(h.tick(-500.0), 0.0);
        assert_eq!(h.tick(1.0), 0.0);
    }

    #[test]
    fn the_gap_boundary_is_strict() {
        let mut h = Harness::new();
        h.watch.check(h.now);
        // gap == threshold は、ふつうの揺らぎとして扱う。
        assert_eq!(h.tick(21.0), 0.0);
        let gap = h.tick(21.5);
        assert!(gap > 20.0 && gap < 20.6, "{gap}");
    }

    #[test]
    fn the_sleep_setting_of_the_pc_is_not_touched() {
        // Scan only the production section: the assertions below contain the
        // forbidden literals themselves.
        let source = include_str!("sleep_watch.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production section");
        assert!(!production.contains("SetThreadExecutionState"));
        assert!(!production.contains("ctypes"));
    }
}
