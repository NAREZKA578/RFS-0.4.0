//! Swap chain abstraction (presenting rendered images to screen).

use super::texture::{Texture, TextureFormat};
use super::types::Extent2D;

/// Swap chain descriptor.
#[derive(Debug, Clone)]
pub struct SwapChainDescriptor {
    /// Width of the swap chain.
    pub width: u32,
    /// Height of the swap chain.
    pub height: u32,
    /// Number of images in the swap chain.
    pub image_count: u32,
    /// Format of the swap chain images.
    pub format: TextureFormat,
    /// Whether to enable vsync.
    pub vsync: bool,
    /// Maximum number of frames in flight.
    pub max_frames_in_flight: u32,
}

impl Default for SwapChainDescriptor {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            image_count: 2,
            format: TextureFormat::Rgba8Unorm,
            vsync: true,
            max_frames_in_flight: 2,
        }
    }
}

/// Swap chain trait — implemented by each backend.
pub trait SwapChain: Send + Sync {
    /// Returns the current image index.
    fn current_image_index(&self) -> u32;

    /// Returns the number of images.
    fn image_count(&self) -> u32;

    /// Returns the image at the given index.
    fn get_image(&self, index: u32) -> &dyn Texture;

    /// Returns the current image.
    fn current_image(&self) -> &dyn Texture;

    /// Acquires the next image for rendering.
    fn acquire_next_image(&mut self) -> Result<u32, Box<dyn std::error::Error>>;

    /// Presents the current image.
    fn present(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// Resizes the swap chain.
    fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>>;

    /// Returns the swap chain size.
    fn size(&self) -> Extent2D;
}
