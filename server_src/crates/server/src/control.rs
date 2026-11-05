//! The main loop's own housekeeping: cadence accounting that a real loop
//! cannot drive with testable deadlines, so it lives apart from the loop.
//!
//! The `control/` children are the control door of
//! `.DOC/new/server-interface-plan.md` (stage 1, Приложение А):
//! `proto` — the wire, `status` — the shared snapshot, `commands` — the pure
//! state+request→answer layer, `ring` — the event ring the `log` command
//! reads, `token` — the door's key file, `journal` — the action journal
//! (stage 2, А.2 item 6), `server` — the listener and its caps. This file
//! stays the module's root and keeps the tick-cadence machinery.

pub mod commands;
pub mod journal;
pub mod proto;
pub mod ring;
pub mod server;
pub mod status;
pub mod token;

use std::time::Duration;

/// Counts ticks the interval skipped, which it does silently by design.
///
/// Bug №614 (analysis 3.6.4): `MissedTickBehavior::Skip` drops a tick whose
/// deadline passed while the loop was busy, and says nothing. The only evidence
/// was `loop avg` sitting above budget — which cannot separate "the iteration
/// is slow" from "the iteration never happened". A server silently running at
/// 23 Hz against a 30 Hz contract looks like a network problem to every client
/// and like nothing at all in the log.
///
/// `Interval::tick()` hands back the *scheduled* deadline even when it is
/// already late, so the expected count is the deadline's distance from the first
/// one, and the skipped count is whatever the loop did not run. No wall-clock
/// guessing: the answer comes from the same clock the scheduler uses.
#[derive(Default)]
pub struct TickCadence {
    origin: Option<tokio::time::Instant>,
    ran: u64,
    skipped: u64,
}

impl TickCadence {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one completed iteration against the deadline that woke it.
    pub fn observe(&mut self, deadline: tokio::time::Instant, period: Duration) {
        let period_nanos = period.as_nanos();
        if period_nanos == 0 {
            // A zero period would divide by zero. The tick budget is a constant
            // of the crate, but this is the answer to "how many did we skip",
            // and a panic here would be reported as a tick-rate bug.
            self.ran += 1;
            return;
        }
        match self.origin {
            // The first deadline is the origin, not a measurement.
            None => self.origin = Some(deadline),
            Some(origin) => {
                self.ran += 1;
                let expected = deadline.saturating_duration_since(origin).as_nanos() / period_nanos;
                // Assigned, not accumulated. `expected` and `ran` are both
                // absolute from the first deadline, so their difference is the
                // total skipped so far; adding the difference on every tick
                // would count the same shortfall once per tick it persists, and
                // the number would climb for as long as the loop stayed behind.
                if expected > self.ran as u128 {
                    self.skipped = (expected - self.ran as u128) as u64;
                }
            }
        }
    }

    pub fn ran(&self) -> u64 {
        self.ran
    }

    pub fn skipped(&self) -> u64 {
        self.skipped
    }
}

/// Bug №614 (analysis 3.6.4): the skipped-tick arithmetic, separated from the
/// loop so it can be driven with deadlines no test can actually wait for.
#[cfg(test)]
mod tick_cadence_tests {
    use super::*;
    use rfs_core::time::TICK_DURATION_MS;

    const PERIOD: Duration = Duration::from_millis(TICK_DURATION_MS);

    /// The headline case: a loop that keeps up skips nothing, and the counter
    /// saying otherwise would send someone hunting a bug that is not there.
    #[test]
    fn a_loop_that_keeps_up_skips_nothing() {
        let mut cadence = TickCadence::new();
        let start = tokio::time::Instant::now();
        for i in 0..30u32 {
            cadence.observe(start + PERIOD * i, PERIOD);
        }
        assert_eq!(cadence.ran(), 29, "the origin deadline is not a run");
        assert_eq!(
            cadence.skipped(),
            0,
            "a perfectly paced loop reported {} skipped ticks",
            cadence.skipped()
        );
    }

    /// The defect itself: one wake-up that arrives three periods late has
    /// swallowed two ticks, and the old code reported nothing at all.
    #[test]
    fn a_late_iteration_reports_the_ticks_it_swallowed() {
        let mut cadence = TickCadence::new();
        let start = tokio::time::Instant::now();
        cadence.observe(start, PERIOD);
        // One iteration, but the deadline is three periods on: two were skipped.
        cadence.observe(start + PERIOD * 3, PERIOD);
        assert_eq!(cadence.ran(), 1);
        assert_eq!(cadence.skipped(), 2, "the swallowed ticks were not counted");
    }

    /// Recovery must not manufacture skips or erase real ones: the counter is
    /// absolute against the first deadline, so returning to pace only adds
    /// future catches up, never rewrites history.
    #[test]
    fn recovering_does_not_rewrite_what_was_already_skipped() {
        let mut cadence = TickCadence::new();
        let start = tokio::time::Instant::now();
        cadence.observe(start, PERIOD);
        cadence.observe(start + PERIOD * 4, PERIOD);
        assert_eq!(cadence.skipped(), 3);
        // Back on pace for the next two deadlines.
        cadence.observe(start + PERIOD * 5, PERIOD);
        cadence.observe(start + PERIOD * 6, PERIOD);
        assert_eq!(
            cadence.skipped(),
            3,
            "the tally changed while the loop was back on schedule"
        );
        assert_eq!(cadence.ran(), 3);
    }

    /// A zero period must not divide by zero: this number exists to explain a
    /// tick-rate problem, and panicking inside it would be reported as one.
    #[test]
    fn a_zero_period_does_not_panic() {
        let mut cadence = TickCadence::new();
        let start = tokio::time::Instant::now();
        cadence.observe(start, Duration::ZERO);
        cadence.observe(start + Duration::from_millis(100), Duration::ZERO);
        assert_eq!(cadence.skipped(), 0);
    }
}
