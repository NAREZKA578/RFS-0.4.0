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

impl TimelineSemaphore {
    pub fn new(initial_value: u64) -> Self {
        Self {
            desc: SemaphoreDesc::default(),
            initial_value,
            value: AtomicU64::new(initial_value),
        }
    }

    /// Signals the semaphore with the given payload value.
    pub fn signal(&self, value: u64) -> RhiResult<()> {
        self.value.store(value, Ordering::Release);
        Ok(())
    }

    /// Blocks until the semaphore reaches `value`.
    pub fn wait(&self, value: u64, _timeout: Option<u64>) -> RhiResult<bool> {
        Ok(self.value.load(Ordering::Acquire) >= value)
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