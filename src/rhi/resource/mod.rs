//! Resource Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

pub mod acceleration;
pub mod buffer;
pub mod sampler;
pub mod texture;

pub use acceleration::*;
pub use buffer::*;
pub use sampler::*;
pub use texture::*;

/// Raw image resource. Texture is the concrete GPU image container in this
/// RHI, so `Image` is provided as an alias for the render layer.
pub type Image = Texture;

/// Opaque native handle attached to a resource by the active backend. The
/// fields are interpreted as `vk` handles when the Vulkan backend is active.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GpuResource {
    /// Native object handle (buffer / image / image view / sampler).
    pub handle: u64,
    /// Backing device memory handle (0 when the resource owns no memory).
    pub memory: u64,
    /// Mapped host pointer (0 when the memory is not mapped).
    pub mapped: usize,
}
