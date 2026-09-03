//! Surface abstraction (window surface for rendering).

use super::types::Extent2D;

/// Surface descriptor.
#[derive(Debug, Clone)]
pub struct SurfaceDescriptor {
    /// Window handle (HWND on Windows, Window handle on other platforms).
    pub window_handle: u64,
    /// Instance handle (HINSTANCE on Windows).
    pub instance_handle: u64,
    /// Desired surface width.
    pub width: u32,
    /// Desired surface height.
    pub height: u32,
}

impl SurfaceDescriptor {
    pub fn new(window_handle: u64, width: u32, height: u32) -> Self {
        Self {
            window_handle,
            instance_handle: 0,
            width,
            height,
        }
    }
}

/// Surface trait — implemented by each backend.
pub trait Surface: Send + Sync {
    /// Returns the surface size.
    fn size(&self) -> Extent2D;

    /// Resizes the surface.
    fn resize(&mut self, width: u32, height: u32);

    /// Returns the backend-specific handle.
    fn native_handle(&self) -> u64;
}
