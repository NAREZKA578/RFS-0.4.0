//! Fence
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::error::RhiResult;
use std::sync::atomic::{AtomicBool, Ordering};

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

    /// Blocks until the fence is signaled. The stub backend has no GPU work,
    /// so pending fences are reported as immediately signaled; a signaled
    /// fence returns immediately.
    pub fn wait(&self, _timeout: Option<u64>) -> RhiResult<bool> {
        Ok(self.signaled.load(Ordering::Acquire))
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