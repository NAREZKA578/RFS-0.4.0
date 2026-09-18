//! Swap Chain
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::surface::Surface;
use crate::error::*;
use crate::resource::{Texture, TextureView};
use crate::types::*;

/// Color space
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorSpace {
    #[default]
    Srgb,
    Hdr10,
    DolbyVision,
    Hlg,
}

/// Present mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PresentMode {
    #[default]
    Immediate,
    Mailbox,
    Fifo,
    FifoRelaxed,
}

/// Surface transform
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SurfaceTransform {
    #[default]
    Identity,
    Rotate90,
    Rotate180,
    Rotate270,
    HorizontalFlip,
}

/// Composite alpha
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CompositeAlpha {
    #[default]
    Opaque,
    PreMultiplied,
    PostMultiplied,
    Inherit,
}

/// Fullscreen exclusive
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FullscreenExclusive {
    #[default]
    None,
    Fullscreen,
    ApplicationControlled,
}

/// Swap chain description
#[derive(Debug, Clone, Default)]
pub struct SwapChainDesc {
    pub surface: Surface,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub color_space: ColorSpace,
    pub present_mode: PresentMode,
    pub buffer_count: u32,
    pub usage: TextureUsage,
    pub sharing_mode: SharingMode,
    pub queue_family_indices: Vec<u32>,
    pub pre_transform: SurfaceTransform,
    pub alpha_composite: CompositeAlpha,
    pub clipped: bool,
    pub fullscreen_exclusive: FullscreenExclusive,
}

/// Swap chain image
pub struct SwapChainImage {
    pub texture: Texture,
    pub view: TextureView,
    pub index: u32,
}

/// Swap chain
pub struct SwapChain {
    desc: SwapChainDesc,
    images: Vec<SwapChainImage>,
    current_image_index: Option<u32>,
}

impl SwapChain {
    pub fn new(desc: SwapChainDesc) -> Self {
        Self {
            desc,
            images: Vec::new(),
            current_image_index: None,
        }
    }

    pub fn desc(&self) -> &SwapChainDesc {
        &self.desc
    }
    pub fn images(&self) -> &[SwapChainImage] {
        &self.images
    }

    /// Returns the current image index, if one has been acquired.
    pub fn current_image_index(&self) -> Option<u32> {
        self.current_image_index
    }

    fn ensure_images(&mut self) {
        if !self.images.is_empty() {
            return;
        }
        let count = self.desc.buffer_count.max(1);
        self.images = (0..count)
            .map(|i| {
                let texture = Texture::new(TextureDesc {
                    width: self.desc.width,
                    height: self.desc.height,
                    usage: self.desc.usage,
                    format: self.desc.format,
                    ..Default::default()
                });
                let view = texture.create_default_view();
                SwapChainImage {
                    texture,
                    view,
                    index: i,
                }
            })
            .collect();
    }

    /// Acquires the next swap chain image in round-robin order.
    pub fn acquire_next_image(&mut self) -> RhiResult<u32> {
        self.ensure_images();
        if self.images.is_empty() {
            // Should be unreachable because a default surface produces at
            // least one image.
            return Err(RhiError::SwapChainError("no images available".into()));
        }
        let n = self.images.len() as u32;
        let next = self.current_image_index.map_or(0, |i| (i + 1) % n);
        self.current_image_index = Some(next);
        Ok(next)
    }

    /// Presents the given image index back to the presentation engine.
    pub fn present(&mut self, image_index: u32) -> RhiResult<()> {
        if self.images.is_empty() {
            return Err(RhiError::SwapChainError("swap chain has no images".into()));
        }
        if image_index as usize >= self.images.len() {
            return Err(RhiError::SwapChainError(format!(
                "invalid image index {image_index}"
            )));
        }
        self.current_image_index = Some(image_index);
        Ok(())
    }
}

impl SwapChainDesc {
    /// Returns `true` if the presentation configuration is usable by the
    /// stub backend.
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.buffer_count > 0
    }
}

/// Default surface dimensions used by stub swap chains.
pub const DEFAULT_SURFACE_WIDTH: u32 = 1280;
pub const DEFAULT_SURFACE_HEIGHT: u32 = 720;
