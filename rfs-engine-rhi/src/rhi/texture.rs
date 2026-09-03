//! Texture abstraction.

use std::sync::Arc;

/// Texture format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFormat {
    /// 8-bit RGBA.
    Rgba8Unorm,
    /// 8-bit BGRA.
    Bgra8Unorm,
    /// 16-bit RGBA float.
    Rgba16Float,
    /// 32-bit RGBA float.
    Rgba32Float,
    /// 8-bit single channel.
    R8Unorm,
    /// 16-bit single channel float.
    R16Float,
    /// Depth 24-bit.
    Depth24,
    /// Depth 32-bit float.
    Depth32Float,
    /// Depth 24 + Stencil 8.
    Depth24Stencil8,
    /// BC1 (DXT1) compressed.
    Bc1Rgba,
    /// BC3 (DXT5) compressed.
    Bc3Rgba,
    /// BC7 compressed.
    Bc7Rgba,
}

impl TextureFormat {
    /// Returns the size in bytes per pixel/block.
    pub fn bytes_per_pixel(&self) -> u32 {
        match self {
            Self::Rgba8Unorm | Self::Bgra8Unorm => 4,
            Self::Rgba16Float => 8,
            Self::Rgba32Float => 16,
            Self::R8Unorm => 1,
            Self::R16Float => 2,
            Self::Depth24 => 3,
            Self::Depth32Float => 4,
            Self::Depth24Stencil8 => 4,
            Self::Bc1Rgba => 8,                  // 4x4 block = 8 bytes
            Self::Bc3Rgba | Self::Bc7Rgba => 16, // 4x4 block = 16 bytes
        }
    }

    /// Returns true if this is a depth format.
    pub fn is_depth(&self) -> bool {
        matches!(
            self,
            Self::Depth24 | Self::Depth32Float | Self::Depth24Stencil8
        )
    }

    /// Returns true if this format includes stencil.
    pub fn has_stencil(&self) -> bool {
        matches!(self, Self::Depth24Stencil8)
    }

    /// Returns true if this is a compressed format.
    pub fn is_compressed(&self) -> bool {
        matches!(self, Self::Bc1Rgba | Self::Bc3Rgba | Self::Bc7Rgba)
    }
}

/// Texture usage flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureUsage {
    /// Texture can be sampled in shaders.
    Sampled,
    /// Texture can be used as a render target.
    RenderTarget,
    /// Texture can be used as a depth/stencil target.
    DepthStencil,
    /// Texture can be used for both sampling and rendering.
    SampledRenderTarget,
}

/// Texture filter mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    /// Nearest neighbor filtering.
    Nearest,
    /// Linear filtering.
    #[default]
    Linear,
}

/// Texture wrap mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapMode {
    /// Clamp to edge.
    #[default]
    Clamp,
    /// Repeat (tile).
    Repeat,
    /// Mirror repeat.
    MirrorRepeat,
    /// Clamp to border (constant border color).
    Border,
}

/// Texture dimensionality/type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureType {
    /// Standard 2D texture.
    #[default]
    Texture2D,
    /// Cube map texture (6 faces).
    CubeMap,
}

/// Texture descriptor.
#[derive(Debug, Clone)]
pub struct TextureDescriptor {
    /// Texture width.
    pub width: u32,
    /// Texture height.
    pub height: u32,
    /// Texture depth (for 3D textures, default 1).
    pub depth: u32,
    /// Number of mip levels (default 1).
    pub mip_levels: u32,
    /// Number of array layers (default 1).
    pub array_layers: u32,
    /// Texture type (2D or cube map).
    pub texture_type: TextureType,
    /// Texture format.
    pub format: TextureFormat,
    /// Texture usage.
    pub usage: TextureUsage,
    /// Minification filter.
    pub min_filter: Filter,
    /// Magnification filter.
    pub mag_filter: Filter,
    /// Wrap mode for U coordinate.
    pub wrap_u: WrapMode,
    /// Wrap mode for V coordinate.
    pub wrap_v: WrapMode,
    /// Wrap mode for W coordinate (3D textures).
    pub wrap_w: WrapMode,
    /// Whether to generate mipmaps.
    pub generate_mipmaps: bool,
}

impl Default for TextureDescriptor {
    fn default() -> Self {
        Self {
            width: 1,
            height: 1,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            texture_type: TextureType::Texture2D,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsage::Sampled,
            min_filter: Filter::Linear,
            mag_filter: Filter::Linear,
            wrap_u: WrapMode::Clamp,
            wrap_v: WrapMode::Clamp,
            wrap_w: WrapMode::Clamp,
            generate_mipmaps: false,
        }
    }
}

impl TextureDescriptor {
    pub fn new_2d(width: u32, height: u32, format: TextureFormat) -> Self {
        Self {
            width,
            height,
            format,
            ..Default::default()
        }
    }

    pub fn render_target(width: u32, height: u32, format: TextureFormat) -> Self {
        Self {
            width,
            height,
            format,
            usage: TextureUsage::RenderTarget,
            min_filter: Filter::Linear,
            mag_filter: Filter::Linear,
            wrap_u: WrapMode::Clamp,
            wrap_v: WrapMode::Clamp,
            ..Default::default()
        }
    }

    pub fn with_mipmaps(mut self) -> Self {
        self.generate_mipmaps = true;
        self.mip_levels = (self.width.max(self.height) as f32).log2() as u32 + 1;
        self
    }
}
/// Texture trait — implemented by each backend.
pub trait Texture: Send + Sync + std::fmt::Debug {
    /// Returns the texture width.
    fn width(&self) -> u32;

    /// Returns the texture height.
    fn height(&self) -> u32;

    /// Returns the texture depth.
    fn depth(&self) -> u32;

    /// Returns the texture format.
    fn format(&self) -> TextureFormat;

    /// Returns the number of mip levels.
    fn mip_levels(&self) -> u32;

    /// Updates the texture data.
    fn update(
        &mut self,
        data: &[u8],
        mip_level: u32,
        layer: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Binds the texture to a texture unit.
    fn bind(&self, unit: u32) -> Result<(), Box<dyn std::error::Error>>;

    /// Returns the backend-specific handle.
    fn native_handle(&self) -> u64;
}

/// A reference-counted texture handle.
#[derive(Debug, Clone)]
pub struct TextureHandle {
    inner: Arc<dyn Texture>,
}

impl TextureHandle {
    pub fn new<T: Texture + 'static>(texture: T) -> Self {
        Self {
            inner: Arc::new(texture),
        }
    }

    pub fn width(&self) -> u32 {
        self.inner.width()
    }
    pub fn height(&self) -> u32 {
        self.inner.height()
    }
    pub fn depth(&self) -> u32 {
        self.inner.depth()
    }
    pub fn format(&self) -> TextureFormat {
        self.inner.format()
    }
    pub fn mip_levels(&self) -> u32 {
        self.inner.mip_levels()
    }
    pub fn native_handle(&self) -> u64 {
        self.inner.native_handle()
    }
    pub fn bind(&self, unit: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.bind(unit)
    }
}

impl std::ops::Deref for TextureHandle {
    type Target = dyn Texture;
    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}
