//! OpenGL synchronization primitives.

use crate::rhi::sync::Fence;

/// OpenGL fence.
pub struct OpenGLFence {
    /// Signal state (kept for API parity with other backends).
    #[allow(dead_code)]
    signaled: bool,
}

impl OpenGLFence {
    pub fn create(_signaled: bool) -> Fence {
        // OpenGL fences use GLsync, but for simplicity we use a boolean flag
        // In a full implementation, you'd use glFenceSync/glClientWaitSync
        Fence::Uninitialized
    }

    pub fn wait(fence: &Fence, _timeout_ns: u64) -> Result<(), Box<dyn std::error::Error>> {
        match fence {
            Fence::Uninitialized => Ok(()),
            _ => Err("Invalid fence type".into()),
        }
    }
}

/// OpenGL semaphore (no-op in OpenGL).
pub struct OpenGLSemaphore;
