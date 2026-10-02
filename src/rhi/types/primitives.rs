//! Primitive Types (Format, SampleCount, Extent, etc.)
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use serde::{Deserialize, Serialize};

/// Texture and buffer formats
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[derive(Default)]
pub enum Format {
    // 8-bit
    R8_UNORM,
    R8_SNORM,
    R8_UINT,
    R8_SINT,
    // 16-bit
    R16_UNORM,
    R16_SNORM,
    R16_UINT,
    R16_SINT,
    R16_SFLOAT,
    RG8_UNORM,
    RG8_SNORM,
    RG8_UINT,
    RG8_SINT,
    // 32-bit
    R32_UINT,
    R32_SINT,
    R32_SFLOAT,
    RG16_UNORM,
    RG16_SNORM,
    RG16_UINT,
    RG16_SINT,
    RG16_SFLOAT,
    RGBA16_UNORM,
    RGBA16_SFLOAT,
    #[default]
    RGBA8_UNORM,
    RGBA8_SNORM,
    RGBA8_UINT,
    RGBA8_SINT,
    // BGR order, and sRGB.
    //
    // A window surface almost always offers `B8G8R8A8_SRGB` — it is what the
    // desktop compositor wants, so nothing has to be converted on the way out.
    // Without these two, a swapchain format could not be described by the RHI
    // at all, which is why presentation could not be finished: the render pass
    // has to be built for the format the *surface* chose.
    B8G8R8A8_UNORM,
    B8G8R8A8_SRGB,
    // 64-bit
    R32G32_UINT,
    R32G32_SINT,
    R32G32_SFLOAT,
    // 96-bit
    R32G32B32_UINT,
    R32G32B32_SINT,
    R32G32B32_SFLOAT,
    // 128-bit
    RGBA32_UINT,
    RGBA32_SINT,
    RGBA32_SFLOAT,
    // Depth/Stencil
    D16_UNORM,
    D24_UNORM,
    D32_SFLOAT,
    D24_UNORM_S8_UINT,
    D32_SFLOAT_S8_UINT,
    S8_UINT,
    // Compressed (BC)
    BC1_RGB_UNORM,
    BC1_RGB_SRGB,
    BC1_RGBA_UNORM,
    BC1_RGBA_SRGB,
    BC2_UNORM,
    BC2_SRGB,
    BC3_UNORM,
    BC3_SRGB,
    BC4_UNORM,
    BC4_SNORM,
    BC5_UNORM,
    BC5_SNORM,
    BC6H_UFLOAT,
    BC6H_SFLOAT,
    BC7_UNORM,
    BC7_SRGB,
    // Compressed (ASTC)
    ASTC_4x4_UNORM,
    ASTC_4x4_SRGB,
    ASTC_5x4_UNORM,
    ASTC_5x4_SRGB,
    ASTC_5x5_UNORM,
    ASTC_5x5_SRGB,
    ASTC_6x5_UNORM,
    ASTC_6x5_SRGB,
    ASTC_6x6_UNORM,
    ASTC_6x6_SRGB,
    ASTC_8x5_UNORM,
    ASTC_8x5_SRGB,
    ASTC_8x6_UNORM,
    ASTC_8x6_SRGB,
    ASTC_8x8_UNORM,
    ASTC_8x8_SRGB,
    // Compressed (ETC2)
    ETC2_R8G8B8_UNORM,
    ETC2_R8G8B8_SRGB,
    ETC2_R8G8B8A1_UNORM,
    ETC2_R8G8B8A1_SRGB,
    ETC2_R8G8B8A8_UNORM,
    ETC2_R8G8B8A8_SRGB,
    EAC_R11_UNORM,
    EAC_R11_SNORM,
    EAC_R11G11_UNORM,
    EAC_R11G11_SNORM,
    // Packed
    B10G11R11_UFLOAT,
    E5B9G9R9_UFLOAT,
    // Special
    A8_UNORM,
}


impl Format {
    pub const fn is_depth(&self) -> bool {
        matches!(
            self,
            Format::D16_UNORM
                | Format::D24_UNORM
                | Format::D32_SFLOAT
                | Format::D24_UNORM_S8_UINT
                | Format::D32_SFLOAT_S8_UINT
        )
    }

    pub const fn is_stencil(&self) -> bool {
        matches!(
            self,
            Format::S8_UINT | Format::D24_UNORM_S8_UINT | Format::D32_SFLOAT_S8_UINT
        )
    }

    pub const fn is_compressed(&self) -> bool {
        matches!(
            self,
            Format::BC1_RGB_UNORM
                | Format::BC1_RGB_SRGB
                | Format::BC1_RGBA_UNORM
                | Format::BC1_RGBA_SRGB
                | Format::BC2_UNORM
                | Format::BC2_SRGB
                | Format::BC3_UNORM
                | Format::BC3_SRGB
                | Format::BC4_UNORM
                | Format::BC4_SNORM
                | Format::BC5_UNORM
                | Format::BC5_SNORM
                | Format::BC6H_UFLOAT
                | Format::BC6H_SFLOAT
                | Format::BC7_UNORM
                | Format::BC7_SRGB
                | Format::ASTC_4x4_UNORM
                | Format::ASTC_4x4_SRGB
                | Format::ASTC_5x4_UNORM
                | Format::ASTC_5x4_SRGB
                | Format::ASTC_5x5_UNORM
                | Format::ASTC_5x5_SRGB
                | Format::ASTC_6x5_UNORM
                | Format::ASTC_6x5_SRGB
                | Format::ASTC_6x6_UNORM
                | Format::ASTC_6x6_SRGB
                | Format::ASTC_8x5_UNORM
                | Format::ASTC_8x5_SRGB
                | Format::ASTC_8x6_UNORM
                | Format::ASTC_8x6_SRGB
                | Format::ASTC_8x8_UNORM
                | Format::ASTC_8x8_SRGB
                | Format::ETC2_R8G8B8_UNORM
                | Format::ETC2_R8G8B8_SRGB
                | Format::ETC2_R8G8B8A1_UNORM
                | Format::ETC2_R8G8B8A1_SRGB
                | Format::ETC2_R8G8B8A8_UNORM
                | Format::ETC2_R8G8B8A8_SRGB
                | Format::EAC_R11_UNORM
                | Format::EAC_R11_SNORM
                | Format::EAC_R11G11_UNORM
                | Format::EAC_R11G11_SNORM
        )
    }
}

/// Sample count for multi-sampling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SampleCount {
    #[default]
    X1,
    X2,
    X4,
    X8,
    X16,
    X32,
    X64,
}

impl SampleCount {
    pub const fn as_count(&self) -> u32 {
        match self {
            SampleCount::X1 => 1,
            SampleCount::X2 => 2,
            SampleCount::X4 => 4,
            SampleCount::X8 => 8,
            SampleCount::X16 => 16,
            SampleCount::X32 => 32,
            SampleCount::X64 => 64,
        }
    }
}

/// 2D extent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

impl Extent2D {
    pub const fn new(w: u32, h: u32) -> Self {
        Self {
            width: w,
            height: h,
        }
    }
}

/// 3D extent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Extent3D {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

/// 2D offset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Offset2D {
    pub x: i32,
    pub y: i32,
}

/// 3D offset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Offset3D {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// 2D rectangle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rect2D {
    pub offset: Offset2D,
    pub extent: Extent2D,
}

/// Viewport
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

pub type Scissor = Rect2D;

/// Clear value
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ClearValue {
    Color { r: f32, g: f32, b: f32, a: f32 },
    DepthStencil { depth: f32, stencil: u32 },
}

impl ClearValue {
    pub const fn color(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::Color { r, g, b, a }
    }

    /// A depth/stencil clear.
    ///
    /// Present because `color` was, and a render pass with a depth attachment
    /// needs one of these per frame: a depth attachment with a clear load
    /// operation and no matching clear value is invalid.
    pub const fn depth_stencil(depth: f32, stencil: u32) -> Self {
        Self::DepthStencil { depth, stencil }
    }
}

/// Primitive topology
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrimitiveTopology {
    PointList,
    LineList,
    LineStrip,
    TriangleList,
    TriangleStrip,
    TriangleFan,
}

/// Index type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndexType {
    U16,
    U32,
}

/// Texture dimensions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextureDimensions {
    #[default]
    D1,
    D2,
    D3,
    Cube,
}

/// Sharing mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SharingMode {
    #[default]
    Exclusive,
    Concurrent,
}

/// Graphics API backend types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GraphicsApi {
    Vulkan,
    Direct3D12,
    Direct3D11,
    OpenGL,
}
