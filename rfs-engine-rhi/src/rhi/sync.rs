//! Synchronization primitives (fences, semaphores).

use std::fmt;

/// Fence for CPU-GPU synchronization.
/// Each variant stores the native fence handle for its backend.
#[derive(Clone)]
pub enum Fence {
    Uninitialized,
    /// OpenGL fence (stores native GLsync)
    OpenGL(glow::Fence),
    /// Vulkan fence (stores raw handle)
    Vulkan(u64),
    /// DirectX 11 fence
    Dx11(u64),
    /// DirectX 12 fence
    Dx12(u64),
}

impl Fence {
    /// Creates an uninitialized fence.
    pub const fn uninitialized() -> Self {
        Self::Uninitialized
    }

    /// Creates an OpenGL fence from a GLsync pointer.
    pub fn opengl(sync: glow::Fence) -> Self {
        Self::OpenGL(sync)
    }

    /// Creates a Vulkan fence from a handle.
    pub fn vulkan(handle: u64) -> Self {
        Self::Vulkan(handle)
    }

    /// Creates a DX11 fence from a handle.
    pub fn dx11(handle: u64) -> Self {
        Self::Dx11(handle)
    }

    /// Creates a DX12 fence from a handle.
    pub fn dx12(handle: u64) -> Self {
        Self::Dx12(handle)
    }

    /// Returns true if the fence is initialized.
    pub fn is_initialized(&self) -> bool {
        !matches!(self, Self::Uninitialized)
    }

    /// Returns true if this is an OpenGL fence.
    pub fn is_opengl(&self) -> bool {
        matches!(self, Self::OpenGL(_))
    }

    /// Returns the OpenGL sync pointer, or None if not an OpenGL fence.
    pub fn opengl_sync(&self) -> Option<glow::Fence> {
        match self {
            Self::OpenGL(sync) => Some(*sync),
            _ => None,
        }
    }

    /// Returns the backend-specific handle, or None if not applicable.
    pub fn handle(&self) -> Option<u64> {
        match self {
            Self::Vulkan(h) | Self::Dx11(h) | Self::Dx12(h) => Some(*h),
            _ => None,
        }
    }
}

impl Default for Fence {
    fn default() -> Self {
        Self::uninitialized()
    }
}

impl fmt::Debug for Fence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uninitialized => write!(f, "Fence::Uninitialized"),
            Self::OpenGL(sync) => write!(f, "Fence::OpenGL({:?})", sync),
            Self::Vulkan(h) => write!(f, "Fence::Vulkan({})", h),
            Self::Dx11(h) => write!(f, "Fence::Dx11({})", h),
            Self::Dx12(h) => write!(f, "Fence::Dx12({})", h),
        }
    }
}

impl PartialEq for Fence {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Uninitialized, Self::Uninitialized) => true,
            (Self::OpenGL(a), Self::OpenGL(b)) => a == b,
            (Self::Vulkan(a), Self::Vulkan(b)) => a == b,
            (Self::Dx11(a), Self::Dx11(b)) => a == b,
            (Self::Dx12(a), Self::Dx12(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Fence {}

/// Semaphore for GPU-GPU synchronization (primarily Vulkan).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Semaphore {
    #[default]
    Uninitialized,
    Vulkan(u64),
}

impl Semaphore {
    pub fn is_initialized(&self) -> bool {
        !matches!(self, Self::Uninitialized)
    }

    pub fn handle(&self) -> Option<u64> {
        match self {
            Self::Vulkan(h) => Some(*h),
            _ => None,
        }
    }
}
