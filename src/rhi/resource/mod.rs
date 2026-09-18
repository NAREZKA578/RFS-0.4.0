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
