//! The render diagnostics facility.
//!
//! Its whole reason for existing is the per-frame spam: while bringing up the
//! geometry pass, a single missing presentation target produced two lines of
//! stderr *per frame*. These tests pin the two properties that fix that —
//! repeat suppression, and a level that actually silences output.

use rfs_client::render::diagnostics::{
    repeat_count, report, reset, set_min_severity, Severity,
};

/// These share one process-wide map, so they must not run concurrently.
fn serial<T>(f: impl FnOnce() -> T) -> T {
    use std::sync::{Mutex, MutexGuard, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard: MutexGuard<()> = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner());
    reset();
    set_min_severity(Severity::Warn);
    let out = f();
    reset();
    set_min_severity(Severity::Warn);
    drop(guard);
    out
}

#[test]
fn a_repeated_diagnostic_is_counted_not_reprinted() {
    serial(|| {
        // The same condition, reported as it would be every frame.
        for _ in 0..500 {
            report(Severity::Warn, "test", "the same thing again");
        }
        // Counted exactly, and available to a test without scraping stderr.
        assert_eq!(repeat_count("test", "the same thing again"), 500);

        // A different message is a different condition and starts at one.
        report(Severity::Warn, "test", "a different thing");
        assert_eq!(repeat_count("test", "a different thing"), 1);

        // The same text from another subsystem is yet another condition: two
        // subsystems can fail for the same reason and must not be conflated.
        report(Severity::Warn, "other", "the same thing again");
        assert_eq!(repeat_count("other", "the same thing again"), 1);
    });
}

#[test]
fn severity_silences_whole_classes() {
    serial(|| {
        set_min_severity(Severity::Error);
        // Below the threshold: not printed, and not counted either, so raising
        // the level and lowering it again does not leave a stale count behind.
        report(Severity::Warn, "quiet", "suppressed");
        assert_eq!(
            repeat_count("quiet", "suppressed"),
            0,
            "a suppressed diagnostic must not be counted"
        );

        report(Severity::Error, "loud", "reported");
        assert_eq!(repeat_count("loud", "reported"), 1);
    });
}

#[test]
fn reset_makes_a_message_print_in_full_again() {
    serial(|| {
        report(Severity::Warn, "test", "after a reset");
        assert_eq!(repeat_count("test", "after a reset"), 1);
        reset();
        assert_eq!(repeat_count("test", "after a reset"), 0);
    });
}

#[test]
fn the_format_is_the_requested_prefix() {
    // Guards the exact prefix, since the whole point of the facility was the
    // agreed `WARN/` form. This is the one thing about the output worth pinning
    // down, because nothing else observes it.
    let prefix = match Severity::Warn.prefix_for_test() {
        p => p,
    };
    assert_eq!(prefix, "WARN/");
    assert_eq!(Severity::Error.prefix_for_test(), "ERROR/");
    assert_eq!(Severity::Info.prefix_for_test(), "INFO/");
}
