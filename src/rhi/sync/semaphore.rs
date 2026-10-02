//! Semaphore
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::error::RhiResult;
use bitflags::bitflags;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// Semaphore flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct SemaphoreFlags: u32 {
        const NONE = 0;
    }
}

/// Semaphore description
#[derive(Debug, Clone, Default)]
pub struct SemaphoreDesc {
    pub flags: SemaphoreFlags,
}

/// Semaphore
pub struct Semaphore {
    desc: SemaphoreDesc,
    signaled: AtomicBool,
}

impl Semaphore {
    pub fn new(desc: SemaphoreDesc) -> Self {
        Self {
            desc,
            signaled: AtomicBool::new(false),
        }
    }

    pub fn desc(&self) -> &SemaphoreDesc {
        &self.desc
    }

    /// Returns `true` if the semaphore is currently signaled.
    pub fn signaled(&self) -> bool {
        self.signaled.load(Ordering::Acquire)
    }

    /// Signals the semaphore (CPU stub convenience for testing).
    pub fn signal(&self) {
        self.signaled.store(true, Ordering::Release);
    }

    /// Resets the semaphore back to the unsignaled state.
    pub fn reset(&self) {
        self.signaled.store(false, Ordering::Release);
    }
}

/// Timeline semaphore
pub struct TimelineSemaphore {
    desc: SemaphoreDesc,
    initial_value: u64,
    value: AtomicU64,
}

/// How long `wait` sleeps between re-checks, and the default budget when the
/// caller passes `None`. See `Fence::wait`: an unlimited default is a trap for
/// a stub that never signals, so the default is one second and a caller that
/// wants to block for a long time passes an explicit timeout.
const TIMELINE_POLL: std::time::Duration = std::time::Duration::from_micros(50);
const TIMELINE_DEFAULT_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

impl TimelineSemaphore {
    pub fn new(initial_value: u64) -> Self {
        Self {
            desc: SemaphoreDesc::default(),
            initial_value,
            value: AtomicU64::new(initial_value),
        }
    }

    /// Signals the semaphore with the given payload value.
    ///
    /// Bug №182: this used to be `value.store(value)` with no check, so a
    /// signal could move a timeline semaphore *backwards*. A Vulkan timeline
    /// semaphore is monotonic by definition — a reader that already observed
    /// value N would then see N-1, and any frame counted against the higher
    /// value silently disappears from the accounting.
    pub fn signal(&self, value: u64) -> RhiResult<()> {
        let current = self.value.load(Ordering::Acquire);
        if value < current {
            return Err(crate::error::RhiError::SemaphoreError(format!(
                "timeline semaphore cannot go backwards: current {current}, signalled {value}"
            )));
        }
        // fetch_max keeps the check and the store atomic even if two threads
        // signal concurrently.
        self.value.fetch_max(value, Ordering::AcqRel);
        Ok(())
    }

    /// Blocks until the semaphore reaches `value`.
    ///
    /// Bug №182: the old body was `Ok(self.value.load(..) >= value)` — it never
    /// blocked and the `timeout` parameter was named `_timeout`, i.e.
    /// deliberately ignored, while the doc comment promised a block. A caller
    /// therefore treated a frame as synchronised when the GPU had not finished.
    ///
    /// `timeout` is in milliseconds; `None` means `TIMELINE_DEFAULT_WAIT` (1 s).
    /// Pass `Some(0)` for a non-blocking poll. Returning `Ok(false)` means the
    /// wait expired; an error means the request itself was invalid.
    pub fn wait(&self, value: u64, timeout: Option<u64>) -> RhiResult<bool> {
        let current = self.value.load(Ordering::Acquire);
        if current >= value {
            return Ok(true);
        }

        let budget = match timeout {
            Some(0) => return Ok(false),
            Some(ms) => std::time::Duration::from_millis(ms),
            None => TIMELINE_DEFAULT_WAIT,
        };
        let deadline = std::time::Instant::now() + budget;

        loop {
            if self.value.load(Ordering::Acquire) >= value {
                return Ok(true);
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Ok(false);
            }
            let remaining = deadline - now;
            std::thread::sleep(std::cmp::min(TIMELINE_POLL, remaining));
        }
    }

    /// Returns the current semaphore value.
    pub fn get_value(&self) -> RhiResult<u64> {
        Ok(self.value.load(Ordering::Acquire))
    }

    /// Returns the value the semaphore was created with.
    pub fn initial_value(&self) -> u64 {
        self.initial_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RhiError;

    #[test]
    fn a_signal_cannot_move_the_value_backwards() {
        let s = TimelineSemaphore::new(0);
        s.signal(5).unwrap();
        assert_eq!(s.get_value().unwrap(), 5);

        let err = s.signal(3).expect_err("a backwards signal must be refused");
        assert!(
            matches!(err, RhiError::SemaphoreError(_)),
            "expected a semaphore error, got {err:?}"
        );
        // And the value is untouched.
        assert_eq!(s.get_value().unwrap(), 5);
    }

    #[test]
    fn an_already_reached_value_returns_immediately() {
        let s = TimelineSemaphore::new(10);
        assert!(s.wait(5, Some(0)).unwrap());
        assert!(s.wait(10, Some(0)).unwrap());
    }

    #[test]
    fn a_zero_timeout_reports_failure_instead_of_blocking_forever() {
        let s = TimelineSemaphore::new(0);
        let start = std::time::Instant::now();
        assert!(!s.wait(1, Some(0)).unwrap(), "must report the timeout");
        assert!(
            start.elapsed() < std::time::Duration::from_millis(200),
            "a zero timeout must not block"
        );
    }

    #[test]
    fn wait_returns_false_when_the_value_never_arrives() {
        let s = TimelineSemaphore::new(0);
        let start = std::time::Instant::now();
        assert!(!s.wait(1, Some(20)).unwrap());
        assert!(start.elapsed() >= std::time::Duration::from_millis(15));
    }

    #[test]
    fn wait_actually_blocks_until_another_thread_signals() {
        let s = std::sync::Arc::new(TimelineSemaphore::new(0));
        let writer = s.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            writer.signal(1).unwrap();
        });

        // Bug №182: the old implementation returned true immediately here
        // without ever waiting.
        let start = std::time::Instant::now();
        assert!(s.wait(1, Some(5_000)).unwrap(), "the signal must be observed");
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(25),
            "wait returned before the signal arrived: {:?}",
            start.elapsed()
        );
        handle.join().unwrap();
    }
}