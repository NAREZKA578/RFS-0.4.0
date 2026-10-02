//! Texture Resources
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::types::*;

use super::GpuResource;

/// Texture description
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub format: Format,
    pub usage: TextureUsage,
    pub sample_count: SampleCount,
    pub dimensions: TextureDimensions,
    pub sharing_mode: SharingMode,
    pub queue_family_indices: Vec<u32>,
}

impl TextureDesc {
    /// Returns the number of mip levels available for a texture with the
    /// given dimensions.
    pub fn max_mip_levels(width: u32, height: u32) -> u32 {
        if width == 0 || height == 0 {
            return 1;
        }
        let max_dim = width.max(height);
        32 - max_dim.leading_zeros()
    }

    /// Returns `true` when every mip level reduces the dimensions to at
    /// least 1x1.
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.depth > 0
            && self.array_layers > 0
            && self.mip_levels > 0
            && self.usage != TextureUsage::empty()
            && match self.sharing_mode {
                SharingMode::Exclusive => true,
                SharingMode::Concurrent => !self.queue_family_indices.is_empty(),
            }
    }
}

/// Texture resource
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Texture {
    desc: TextureDesc,
    pub(crate) backend: Option<GpuResource>,
}

impl Texture {
    pub fn new(desc: TextureDesc) -> Self {
        Self { desc, backend: None }
    }

    /// Adopt an image the backend already owns.
    ///
    /// A swapchain image is created by `vkCreateSwapchainKHR`, not by the RHI,
    /// so it arrives here as a raw handle. There is no descriptor to validate
    /// and no memory to own — the swapchain owns both — so this exists purely to
    /// let a presentation image be used wherever a `Texture` is expected, which
    /// is what building a render pass on a swapchain image needs.
    pub(crate) fn adopt_backend_image(
        width: u32,
        height: u32,
        format: Format,
        image: u64,
        image_view: u64,
    ) -> (Self, TextureView) {
        let texture = Texture {
            desc: TextureDesc {
                width,
                height,
                depth: 1,
                mip_levels: 1,
                array_layers: 1,
                format,
                usage: TextureUsage::COLOR_ATTACHMENT,
                sample_count: crate::types::SampleCount::X1,
                ..Default::default()
            },
            backend: Some(GpuResource { handle: image, memory: image_view, mapped: 0 }),
        };
        let view = TextureView {
            texture: texture.clone(),
            desc: TextureViewDesc {
                texture: texture.clone(),
                format: Some(format),
                view_type: TextureViewType::D2,
                aspects: TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
            backend: Some(GpuResource { handle: image_view, memory: 0, mapped: 0 }),
        };
        (texture, view)
    }

    pub fn from_desc(desc: TextureDesc) -> Self {
        Self::new(desc)
    }

    pub fn desc(&self) -> &TextureDesc {
        &self.desc
    }
    pub fn width(&self) -> u32 {
        self.desc.width
    }
    pub fn height(&self) -> u32 {
        self.desc.height
    }
    pub fn depth(&self) -> u32 {
        self.desc.depth
    }
    pub fn mip_levels(&self) -> u32 {
        self.desc.mip_levels
    }
    pub fn array_layers(&self) -> u32 {
        self.desc.array_layers
    }
    pub fn format(&self) -> Format {
        self.desc.format
    }
    pub fn sample_count(&self) -> SampleCount {
        self.desc.sample_count
    }

    /// Returns the number of texels covered by the texture at a mip level.
    pub fn mip_size(&self, mip: u32) -> (u32, u32, u32) {
        let mip = mip.min(self.desc.mip_levels.saturating_sub(1));
        (
            (self.desc.width >> mip).max(1),
            (self.desc.height >> mip).max(1),
            (self.desc.depth >> mip).max(1),
        )
    }

    /// Returns `true` when the texture can be sampled in shaders.
    pub fn is_sampled(&self) -> bool {
        self.desc.usage.contains(TextureUsage::SAMPLED)
    }

    /// Returns `true` when the texture can be written as a storage image.
    pub fn is_storage(&self) -> bool {
        self.desc.usage.contains(TextureUsage::STORAGE)
    }

    /// Returns `true` when the texture is a depth/stencil attachment.
    pub fn is_depth_stencil(&self) -> bool {
        self.desc
            .usage
            .contains(TextureUsage::DEPTH_STENCIL_ATTACHMENT)
    }

    /// Returns `true` when the texture is a color attachment.
    pub fn is_color_attachment(&self) -> bool {
        self.desc.usage.contains(TextureUsage::COLOR_ATTACHMENT)
    }

    /// Returns `true` when the texture is multisampled.
    pub fn is_multisampled(&self) -> bool {
        self.desc.sample_count.as_count() > 1
    }

    /// Returns the native backend handle attached by the active backend, if any.
    pub(crate) fn backend(&self) -> Option<GpuResource> {
        self.backend
    }

    /// Attaches a native backend handle to this texture.
    pub(crate) fn set_backend(&mut self, resource: GpuResource) {
        self.backend = Some(resource);
    }

    /// Returns `true` when this texture is backed by native GPU memory.
    pub fn has_gpu_backing(&self) -> bool {
        self.backend.is_some()
    }

    /// Create a texture view from this texture.
    pub fn create_view(&self, desc: TextureViewDesc) -> TextureView {
        TextureView {
            texture: self.clone(),
            desc,
            backend: None,
        }
    }

    /// Create a default (full) view over the whole texture.
    pub fn create_default_view(&self) -> TextureView {
        let view_type = match self.desc.dimensions {
            TextureDimensions::D1 => TextureViewType::D1,
            TextureDimensions::D2 => {
                if self.desc.array_layers > 1 {
                    TextureViewType::D2Array
                } else {
                    TextureViewType::D2
                }
            }
            TextureDimensions::D3 => TextureViewType::D3,
            TextureDimensions::Cube => {
                if self.desc.array_layers > 6 {
                    TextureViewType::CubeArray
                } else {
                    TextureViewType::Cube
                }
            }
        };

        self.create_view(TextureViewDesc {
            texture: self.clone(),
            format: None,
            view_type,
            aspects: if self.desc.format.is_depth() {
                TextureAspectFlags::DEPTH
            } else {
                TextureAspectFlags::COLOR
            },
            base_mip_level: 0,
            mip_level_count: self.desc.mip_levels,
            base_array_layer: 0,
            array_layer_count: self.desc.array_layers,
        })
    }
}

/// Texture view description
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextureViewDesc {
    pub texture: Texture,
    pub format: Option<Format>,
    pub view_type: TextureViewType,
    pub aspects: TextureAspectFlags,
    pub base_mip_level: u32,
    pub mip_level_count: u32,
    pub base_array_layer: u32,
    pub array_layer_count: u32,
}

/// Texture view type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureViewType {
    #[default]
    D1,
    D2,
    D3,
    Cube,
    CubeArray,
    D1Array,
    D2Array,
}

/// Texture view
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextureView {
    texture: Texture,
    desc: TextureViewDesc,
    pub(crate) backend: Option<GpuResource>,
}

impl TextureView {
    pub fn from_parts(texture: Texture, desc: TextureViewDesc) -> Self {
        Self {
            texture,
            desc,
            backend: None,
        }
    }

    /// Returns the native backend handle attached by the active backend, if any.
    pub(crate) fn backend(&self) -> Option<GpuResource> {
        self.backend
    }

    /// Attaches a native backend handle to this view.
    pub(crate) fn set_backend(&mut self, resource: GpuResource) {
        self.backend = Some(resource);
    }

    /// Returns `true` when this view is backed by a native GPU image view.
    pub fn has_gpu_backing(&self) -> bool {
        self.backend.is_some()
    }
    pub fn texture(&self) -> &Texture {
        &self.texture
    }
    pub fn desc(&self) -> &TextureViewDesc {
        &self.desc
    }
    pub fn view_type(&self) -> TextureViewType {
        self.desc.view_type
    }
    pub fn format(&self) -> Format {
        self.desc.format.unwrap_or(self.texture.format())
    }
    pub fn base_mip_level(&self) -> u32 {
        self.desc.base_mip_level
    }
    pub fn mip_level_count(&self) -> u32 {
        self.desc.mip_level_count
    }
    pub fn array_layers(&self) -> u32 {
        self.desc.array_layer_count
    }
}

/// Texture layout
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureLayout {
    #[default]
    Undefined,
    General,
    ColorAttachmentOptimal,
    TransferSrcOptimal,
    TransferDstOptimal,
    ShaderReadOnlyOptimal,
    DepthStencilAttachmentOptimal,
    DepthStencilReadOnlyOptimal,
    DepthAttachmentOptimal,
    DepthReadOnlyOptimal,
    StencilAttachmentOptimal,
    StencilReadOnlyOptimal,
    Preinitialized,
    PresentSrc,
}