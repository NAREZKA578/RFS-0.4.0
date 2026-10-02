//! Diagnostics for the render layer.
//!
//! Every failure in this layer used to be an `eprintln!` at the point it was
//! noticed. That has two problems, both of which showed up in practice while
//! bringing up the geometry pass:
//!
//! - The messages repeat once per frame. A missing presentation target produced
//!   two lines of stderr *per frame*, which buries everything else and looks
//!   like a crash rather than one condition.
//! - There is no level and no way to silence them, so a headless test run is as
//!   noisy as a broken window.
//!
//! So messages are counted rather than repeated, and each one names the
//! subsystem it came from. The format is `WARN/<subsystem>: <message>`, with a
//! repeat count appended once a message has been seen again.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::sync::OnceLock;

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Something is broken and rendering is affected.
    Error = 0,
    /// Something is missing or wrong, but rendering continues.
    Warn = 1,
    /// Worth knowing, not worth stopping for.
    Info = 2,
}

impl Severity {
    fn prefix(self) -> &'static str {
        match self {
            Severity::Error => "ERROR/",
            Severity::Warn => "WARN/",
            Severity::Info => "INFO/",
        }
    }

    /// The prefix this severity prints.
    ///
    /// Exposed so the output format can be asserted. Nothing should format a
    /// message itself: a second place that builds the prefix is a second place
    /// that can get it wrong, which is how formats drift.
    pub fn prefix_for_test(self) -> &'static str {
        self.prefix()
    }
}

/// The lowest severity that is printed. Anything less severe is counted and
/// dropped, so raising this silences a whole class rather than needing call
/// sites changed.
static MIN_SEVERITY: AtomicU8 = AtomicU8::new(Severity::Warn as u8);

/// Set the lowest severity that is printed.
pub fn set_min_severity(severity: Severity) {
    MIN_SEVERITY.store(severity as u8, Ordering::Relaxed);
}

fn min_severity() -> Severity {
    match MIN_SEVERITY.load(Ordering::Relaxed) {
        0 => Severity::Error,
        1 => Severity::Warn,
        _ => Severity::Info,
    }
}

/// `subsystem -> message -> times seen`.
fn seen() -> &'static Mutex<HashMap<(&'static str, String), u64>> {
    static SEEN: OnceLock<Mutex<HashMap<(&'static str, String), u64>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Report a diagnostic.
///
/// The first time a given `(subsystem, message)` pair is seen it is printed on
/// its own. Every later report of the same pair increments a counter and prints
/// nothing, so a per-frame condition costs one line rather than sixty per
/// second. The count is readable with [`repeat_count`] and clearable with
/// [`reset`], so a test can assert on it instead of scraping stderr.
pub fn report(severity: Severity, subsystem: &'static str, message: impl std::fmt::Display) {
    if severity > min_severity() {
        return;
    }
    let message = message.to_string();
    let count = {
        let Ok(mut map) = seen().lock() else {
            // A poisoned diagnostics lock must not take the renderer with it.
            eprintln!("{}{subsystem}: {message}", severity.prefix());
            return;
        };
        let entry = map.entry((subsystem, message.clone())).or_insert(0);
        *entry += 1;
        *entry
    };
    match count {
        1 => eprintln!("{}{subsystem}: {message}", severity.prefix()),
        // Reported on the second sighting so a condition that resolves is not
        // permanently silent, then every hundredth so a long run still shows
        // that it is ongoing.
        2 => eprintln!("{}{subsystem}: {message} (now repeating)", severity.prefix()),
        n if n % 100 == 0 => {
            eprintln!("{}{subsystem}: {message} (x{n})", severity.prefix())
        }
        _ => {}
    }
}

/// How many times this exact diagnostic has been reported.
pub fn repeat_count(subsystem: &str, message: &str) -> u64 {
    seen()
        .lock()
        .ok()
        .and_then(|map| map.get(&(subsystem, message.to_string())).copied())
        .unwrap_or(0)
}

/// Forget every count, so a later report prints in full again.
pub fn reset() {
    if let Ok(mut map) = seen().lock() {
        map.clear();
    }
}

/// Report a warning.
#[macro_export]
macro_rules! warn {
    ($subsystem:expr, $($arg:tt)*) => {
        $crate::render::diagnostics::report(
            $crate::render::diagnostics::Severity::Warn,
            $subsystem,
            format_args!($($arg)*),
        )
    };
}

/// Report an error.
#[macro_export]
macro_rules! error {
    ($subsystem:expr, $($arg:tt)*) => {
        $crate::render::diagnostics::report(
            $crate::render::diagnostics::Severity::Error,
            $subsystem,
            format_args!($($arg)*),
        )
    };
}

/// Report a warning once. For conditions that cannot usefully repeat.
#[macro_export]
macro_rules! warn_once {
    ($subsystem:expr, $($arg:tt)*) => {{
        let message = format!($($arg)*);
        if $crate::render::diagnostics::repeat_count($subsystem, &message) == 0 {
            $crate::render::diagnostics::report(
                $crate::render::diagnostics::Severity::Warn,
                $subsystem,
                message,
            );
        }
    }};
}
