//! Texture
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::rhi::{
    Format, Texture as RhiTexture, TextureUsage as RhiTextureUsage,
    TextureView as RhiTextureView,
};
use std::sync::Arc;

/// Texture type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureType {
    Texture2D,
    Texture3D,
    TextureCube,
    TextureArray,
    DepthTexture,
    RenderTarget,
}

impl Default for TextureType {
    fn default() -> Self {
        Self::Texture2D
    }
}

/// Texture format (maps to RHI format)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    RGBA8,
    RGBA16,
    RGBA32,
    RGB8,
    RGB16,
    RGB32,
    RG8,
    RG16,
    R8,
    R16,
    R32,
    Depth16,
    Depth24,
    Depth32,
    Depth24Stencil8,
    Depth32Stencil8,
}

impl From<TextureFormat> for Format {
    fn from(format: TextureFormat) -> Self {
        match format {
            TextureFormat::RGBA8 => Format::RGBA8_UNORM,
            TextureFormat::RGBA16 => Format::RG16_UNORM,
            TextureFormat::RGBA32 => Format::RGBA32_SFLOAT,
            TextureFormat::RGB8 => Format::RGBA8_UNORM,
            TextureFormat::RGB16 => Format::RG16_UNORM,
            TextureFormat::RGB32 => Format::R32G32B32_SFLOAT,
            TextureFormat::RG8 => Format::RG8_UNORM,
            TextureFormat::RG16 => Format::RG16_UNORM,
            TextureFormat::R8 => Format::R8_UNORM,
            TextureFormat::R16 => Format::R16_UNORM,
            TextureFormat::R32 => Format::R32_SFLOAT,
            TextureFormat::Depth16 => Format::D16_UNORM,
            TextureFormat::Depth24 => Format::D24_UNORM,
            TextureFormat::Depth32 => Format::D32_SFLOAT,
            TextureFormat::Depth24Stencil8 => Format::D24_UNORM_S8_UINT,
            TextureFormat::Depth32Stencil8 => Format::D32_SFLOAT_S8_UINT,
        }
    }
}

impl Default for TextureFormat {
    fn default() -> Self {
        Self::RGBA8
    }
}

/// Texture usage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureUsage {
    Diffuse,
    Normal,
    Specular,
    Roughness,
    Metallic,
    AmbientOcclusion,
    Emissive,
    Height,
    Depth,
    Stencil,
    RenderTarget,
    Sampled,
    Storage,
}

impl From<TextureUsage> for RhiTextureUsage {
    fn from(usage: TextureUsage) -> Self {
        match usage {
            TextureUsage::Diffuse
            | TextureUsage::Normal
            | TextureUsage::Specular
            | TextureUsage::Roughness
            | TextureUsage::Metallic
            | TextureUsage::AmbientOcclusion
            | TextureUsage::Emissive
            | TextureUsage::Height
            | TextureUsage::Sampled
            | TextureUsage::Storage => RhiTextureUsage::SAMPLED,
            TextureUsage::Depth | TextureUsage::Stencil => {
                RhiTextureUsage::DEPTH_STENCIL_ATTACHMENT | RhiTextureUsage::SAMPLED
            }
            TextureUsage::RenderTarget => {
                RhiTextureUsage::COLOR_ATTACHMENT | RhiTextureUsage::SAMPLED
            }
        }
    }
}

/// Texture wrap mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureWrapMode {
    Repeat,
    MirroredRepeat,
    ClampToEdge,
    ClampToBorder,
}

impl Default for TextureWrapMode {
    fn default() -> Self {
        Self::Repeat
    }
}

/// Texture filter mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFilterMode {
    Nearest,
    Linear,
    NearestMipmapNearest,
    NearestMipmapLinear,
    LinearMipmapNearest,
    LinearMipmapLinear,
    Anisotropic,
}

impl Default for TextureFilterMode {
    fn default() -> Self {
        Self::Linear
    }
}

/// Sampler description
#[derive(Debug, Clone)]
pub struct SamplerDesc {
    pub wrap_mode: TextureWrapMode,
    pub filter_mode: TextureFilterMode,
    pub anisotropy: f32,
    pub lod_bias: f32,
    pub min_lod: f32,
    pub max_lod: f32,
    pub border_color: [f32; 4],
}

impl Default for SamplerDesc {
    fn default() -> Self {
        Self {
            wrap_mode: TextureWrapMode::Repeat,
            filter_mode: TextureFilterMode::Linear,
            anisotropy: 1.0,
            lod_bias: 0.0,
            min_lod: 0.0,
            max_lod: f32::MAX,
            border_color: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

/// Texture struct (wraps RHI texture)
#[derive(Debug, Clone)]
pub struct Texture {
    pub name: String,
    pub texture_type: TextureType,
    pub format: TextureFormat,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub layers: u32,
    pub mip_levels: u32,
    pub usage: TextureUsage,
    /// RHI texture
    pub rhi_texture: Arc<RhiTexture>,
    /// Default view
    pub default_view: Arc<RhiTextureView>,
    /// Array of views (for cube maps, arrays, mip levels)
    pub views: Vec<Arc<RhiTextureView>>,
    /// Sampler
    pub sampler: Option<Arc<crate::rhi::Sampler>>,
}

impl Texture {
    /// Create a new texture
    pub fn new(
        name: &str,
        texture_type: TextureType,
        format: TextureFormat,
        width: u32,
        height: u32,
        depth: u32,
        layers: u32,
        mip_levels: u32,
        usage: TextureUsage,
        device: &Arc<crate::rhi::Device>,
    ) -> Self {
        let rhi_format = Format::from(format);
        let rhi_usage = RhiTextureUsage::from(usage);

        let rhi_texture =
            device.create_texture(width, height, depth, rhi_format, rhi_usage, mip_levels);

        let rhi_texture = Arc::new(rhi_texture);
        let default_view = Arc::new(rhi_texture.create_view(Default::default()));

        Self {
            name: name.to_string(),
            texture_type,
            format,
            width,
            height,
            depth,
            layers,
            mip_levels,
            usage,
            rhi_texture,
            default_view,
            views: Vec::new(),
            sampler: None,
        }
    }

    /// Create a 2D texture
    pub fn new_2d(
        name: &str,
        format: TextureFormat,
        width: u32,
        height: u32,
        usage: TextureUsage,
        device: &Arc<crate::rhi::Device>,
    ) -> Self {
        Self::new(
            name,
            TextureType::Texture2D,
            format,
            width,
            height,
            1,
            1,
            1,
            usage,
            device,
        )
    }

    /// Create a cube texture
    pub fn new_cube(
        name: &str,
        format: TextureFormat,
        size: u32,
        mip_levels: u32,
        usage: TextureUsage,
        device: &Arc<crate::rhi::Device>,
    ) -> Self {
        Self::new(
            name,
            TextureType::TextureCube,
            format,
            size,
            size,
            1,
            6,
            mip_levels,
            usage,
            device,
        )
    }

    /// Create a depth texture
    pub fn new_depth(
        name: &str,
        width: u32,
        height: u32,
        device: &Arc<crate::rhi::Device>,
    ) -> Self {
        Self::new_2d(
            name,
            TextureFormat::Depth32,
            width,
            height,
            TextureUsage::Depth,
            device,
        )
    }

    /// Create a render target texture
    pub fn new_render_target(
        name: &str,
        width: u32,
        height: u32,
        format: TextureFormat,
        device: &Arc<crate::rhi::Device>,
    ) -> Self {
        Self::new_2d(
            name,
            format,
            width,
            height,
            TextureUsage::RenderTarget,
            device,
        )
    }

    /// Get the RHI texture
    pub fn rhi_texture(&self) -> &Arc<RhiTexture> {
        &self.rhi_texture
    }

    /// Get the default view
    pub fn default_view(&self) -> &Arc<RhiTextureView> {
        &self.default_view
    }

    /// Get a specific view
    pub fn get_view(&self, index: usize) -> Option<&Arc<RhiTextureView>> {
        self.views.get(index)
    }

    /// Create a view for a specific layer (for cube maps, arrays)
    pub fn create_layer_view(&mut self, layer: u32, mip_level: u32) -> Arc<RhiTextureView> {
        let view = self.rhi_texture.create_view(crate::rhi::TextureViewDesc {
            base_array_layer: layer,
            array_layer_count: 1,
            base_mip_level: mip_level,
            mip_level_count: 1,
            ..Default::default()
        });
        let view = Arc::new(view);
        self.views.push(view.clone());
        view
    }

    /// Create a view for a specific mip level
    pub fn create_mip_view(&mut self, mip_level: u32) -> Arc<RhiTextureView> {
        let view = self.rhi_texture.create_view(crate::rhi::TextureViewDesc {
            base_mip_level: mip_level,
            mip_level_count: 1,
            ..Default::default()
        });
        let view = Arc::new(view);
        self.views.push(view.clone());
        view
    }

    /// Create a sampler
    pub fn create_sampler(&mut self, device: &Arc<crate::rhi::Device>, desc: &SamplerDesc) {
        let wrap_mode = match desc.wrap_mode {
            TextureWrapMode::Repeat => crate::rhi::AddressMode::Repeat,
            TextureWrapMode::MirroredRepeat => crate::rhi::AddressMode::MirroredRepeat,
            TextureWrapMode::ClampToEdge => crate::rhi::AddressMode::ClampToEdge,
            TextureWrapMode::ClampToBorder => crate::rhi::AddressMode::ClampToBorder,
        };

        let filter_mode = match desc.filter_mode {
            TextureFilterMode::Nearest => crate::rhi::FilterMode::Nearest,
            TextureFilterMode::Linear => crate::rhi::FilterMode::Linear,
            TextureFilterMode::NearestMipmapNearest => crate::rhi::FilterMode::Nearest,
            TextureFilterMode::NearestMipmapLinear => crate::rhi::FilterMode::Linear,
            TextureFilterMode::LinearMipmapNearest => crate::rhi::FilterMode::Linear,
            TextureFilterMode::LinearMipmapLinear => crate::rhi::FilterMode::Linear,
            TextureFilterMode::Anisotropic => crate::rhi::FilterMode::Linear,
        };

        let sampler = device.create_sampler(wrap_mode, filter_mode, desc.anisotropy);
        self.sampler = Some(Arc::new(sampler));
    }

    /// Get the sampler
    pub fn sampler(&self) -> Option<&Arc<crate::rhi::Sampler>> {
        self.sampler.as_ref()
    }

    /// Get width
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get height
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get depth
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// Get format
    pub fn format(&self) -> TextureFormat {
        self.format
    }

    /// Get texture type
    pub fn texture_type(&self) -> TextureType {
        self.texture_type
    }

    /// Upload data to the texture
    pub fn upload_data(&self, data: &[u8], device: &Arc<crate::rhi::Device>) {
        device.upload_texture(&self.rhi_texture, data);
    }
}

/// Texture builder
pub struct TextureBuilder {
    name: String,
    texture_type: TextureType,
    format: TextureFormat,
    width: u32,
    height: u32,
    depth: u32,
    layers: u32,
    mip_levels: u32,
    usage: TextureUsage,
}

impl TextureBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            texture_type: TextureType::Texture2D,
            format: TextureFormat::RGBA8,
            width: 1,
            height: 1,
            depth: 1,
            layers: 1,
            mip_levels: 1,
            usage: TextureUsage::Diffuse,
        }
    }

    pub fn texture_type(mut self, texture_type: TextureType) -> Self {
        self.texture_type = texture_type;
        self
    }

    pub fn format(mut self, format: TextureFormat) -> Self {
        self.format = format;
        self
    }

    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn depth(mut self, depth: u32) -> Self {
        self.depth = depth;
        self
    }

    pub fn layers(mut self, layers: u32) -> Self {
        self.layers = layers;
        self
    }

    pub fn mip_levels(mut self, mip_levels: u32) -> Self {
        self.mip_levels = mip_levels;
        self
    }

    pub fn usage(mut self, usage: TextureUsage) -> Self {
        self.usage = usage;
        self
    }

    pub fn build(self, device: &Arc<crate::rhi::Device>) -> Texture {
        Texture::new(
            &self.name,
            self.texture_type,
            self.format,
            self.width,
            self.height,
            self.depth,
            self.layers,
            self.mip_levels,
            self.usage,
            device,
        )
    }
}
