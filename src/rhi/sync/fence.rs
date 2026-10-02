//! Fence
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::error::RhiResult;
use std::sync::atomic::{AtomicBool, Ordering};

/// How long `Fence::wait` sleeps between re-checks, and the default budget when
/// the caller passes `None`.
///
/// Bug №181 follow-up: an "unlimited" default is a trap. The CPU stub never
/// signals on its own, so `wait(None)` on a pending fence would either hang
/// forever or — as a 10 s ceiling — make every such call cost 10 s. One second
/// is long enough for a real GPU and short enough that a lost fence surfaces as
/// `Ok(false)` quickly. A caller that genuinely wants to block for a long time
/// passes an explicit timeout.
const FENCE_POLL: std::time::Duration = std::time::Duration::from_micros(50);
const FENCE_DEFAULT_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// Fence description
#[derive(Debug, Clone, Default)]
pub struct FenceDesc {
    pub signaled: bool,
}

/// Fence
pub struct Fence {
    desc: FenceDesc,
    signaled: AtomicBool,
}

impl Fence {
    pub fn new(desc: FenceDesc) -> Self {
        Self {
            signaled: AtomicBool::new(desc.signaled),
            desc,
        }
    }
    pub fn from_desc(desc: FenceDesc) -> Self {
        Self::new(desc)
    }

    pub fn desc(&self) -> &FenceDesc {
        &self.desc
    }

    /// Blocks until the fence is signaled.
    ///
    /// `timeout` is in milliseconds; `None` means `FENCE_DEFAULT_WAIT` (1 s).
    /// Pass `Some(0)` for a non-blocking poll.
    ///
    /// Bug №181: the parameter was named `_timeout` and ignored, while the doc
    /// comment promised a block. A caller that asked "wait 16 ms" got an
    /// immediate answer instead, so a frame could be read while the GPU was
    /// still writing it.
    pub fn wait(&self, timeout: Option<u64>) -> RhiResult<bool> {
        if self.signaled.load(Ordering::Acquire) {
            return Ok(true);
        }
        let budget = match timeout {
            Some(0) => return Ok(false),
            Some(ms) => std::time::Duration::from_millis(ms),
            None => FENCE_DEFAULT_WAIT,
        };
        let deadline = std::time::Instant::now() + budget;
        loop {
            if self.signaled.load(Ordering::Acquire) {
                return Ok(true);
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Ok(false);
            }
            std::thread::sleep(std::cmp::min(FENCE_POLL, deadline - now));
        }
    }

    /// Resets the fence to the unsignaled state.
    pub fn reset(&self) -> RhiResult<()> {
        self.signaled.store(false, Ordering::Release);
        Ok(())
    }

    /// Returns `true` if the fence is currently signaled.
    pub fn get_status(&self) -> RhiResult<bool> {
        Ok(self.signaled.load(Ordering::Acquire))
    }

    /// Signals the fence. CPU stub convenience for testing.
    pub fn signal(&self) -> RhiResult<()> {
        self.signaled.store(true, Ordering::Release);
        Ok(())
    }
}