//! Surface
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::types::*;

/// Surface description
#[derive(Debug, Clone, Default)]
pub struct SurfaceDesc {
    pub width: u32,
    pub height: u32,
}

/// Surface
#[derive(Debug, Clone, Default)]
pub struct Surface {
    desc: SurfaceDesc,
}

impl Surface {
    pub fn new(desc: SurfaceDesc) -> Self {
        Self { desc }
    }

    pub fn desc(&self) -> &SurfaceDesc {
        &self.desc
    }
}

/// Surface capabilities
#[derive(Debug, Clone, Default)]
pub struct SurfaceCapabilities {
    pub supported_present_modes: Vec<PresentMode>,
    pub supported_formats: Vec<Format>,
    pub supported_color_spaces: Vec<ColorSpace>,
    pub supported_usage_flags: TextureUsage,
    pub min_image_count: u32,
    pub max_image_count: u32,
    pub max_image_extent: Extent2D,
    pub current_extent: Extent2D,
}

// Re-export types
pub use super::swapchain::{
    ColorSpace, CompositeAlpha, FullscreenExclusive, PresentMode, SurfaceTransform,
};
