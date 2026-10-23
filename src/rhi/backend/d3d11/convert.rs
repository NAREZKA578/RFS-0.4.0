//! D3D11 conversion tables: RHI types to D3D11 / DXGI values.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::pipeline::graphics::{
    BlendFactor, BlendOp, CullMode, PolygonMode, PrimitiveTopology, StencilOp,
};
use crate::resource::{AddressMode, BorderColor, CompareOp, FilterMode};
use crate::types::{ColorComponentFlags, Format, IndexType};
use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_LINELIST, D3D_PRIMITIVE_TOPOLOGY_LINESTRIP,
    D3D_PRIMITIVE_TOPOLOGY_POINTLIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BLEND, D3D11_BLEND_BLEND_FACTOR, D3D11_BLEND_DEST_ALPHA, D3D11_BLEND_DEST_COLOR,
    D3D11_BLEND_INV_BLEND_FACTOR, D3D11_BLEND_INV_DEST_ALPHA, D3D11_BLEND_INV_DEST_COLOR,
    D3D11_BLEND_INV_SRC1_ALPHA, D3D11_BLEND_INV_SRC1_COLOR, D3D11_BLEND_INV_SRC_ALPHA,
    D3D11_BLEND_INV_SRC_COLOR, D3D11_BLEND_ONE, D3D11_BLEND_OP, D3D11_BLEND_OP_ADD,
    D3D11_BLEND_OP_MAX, D3D11_BLEND_OP_MIN, D3D11_BLEND_OP_REV_SUBTRACT, D3D11_BLEND_OP_SUBTRACT,
    D3D11_BLEND_SRC1_ALPHA, D3D11_BLEND_SRC1_COLOR, D3D11_BLEND_SRC_ALPHA,
    D3D11_BLEND_SRC_ALPHA_SAT, D3D11_BLEND_SRC_COLOR, D3D11_BLEND_ZERO, D3D11_COLOR_WRITE_ENABLE,
    D3D11_COLOR_WRITE_ENABLE_ALPHA, D3D11_COLOR_WRITE_ENABLE_BLUE, D3D11_COLOR_WRITE_ENABLE_GREEN,
    D3D11_COLOR_WRITE_ENABLE_RED, D3D11_COMPARISON_ALWAYS, D3D11_COMPARISON_EQUAL,
    D3D11_COMPARISON_FUNC, D3D11_COMPARISON_GREATER, D3D11_COMPARISON_GREATER_EQUAL,
    D3D11_COMPARISON_LESS, D3D11_COMPARISON_LESS_EQUAL, D3D11_COMPARISON_NEVER,
    D3D11_COMPARISON_NOT_EQUAL, D3D11_CULL_BACK, D3D11_CULL_FRONT, D3D11_CULL_MODE,
    D3D11_CULL_NONE, D3D11_FILL_MODE, D3D11_FILL_SOLID, D3D11_FILL_WIREFRAME, D3D11_FILTER,
    D3D11_STENCIL_OP, D3D11_STENCIL_OP_DECR, D3D11_STENCIL_OP_DECR_SAT, D3D11_STENCIL_OP_INCR,
    D3D11_STENCIL_OP_INCR_SAT, D3D11_STENCIL_OP_INVERT, D3D11_STENCIL_OP_KEEP,
    D3D11_STENCIL_OP_REPLACE, D3D11_STENCIL_OP_ZERO, D3D11_TEXTURE_ADDRESS_BORDER,
    D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE_ADDRESS_MIRROR, D3D11_TEXTURE_ADDRESS_MIRROR_ONCE,
    D3D11_TEXTURE_ADDRESS_MODE, D3D11_TEXTURE_ADDRESS_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
    DXGI_FORMAT_BC1_UNORM, DXGI_FORMAT_BC1_UNORM_SRGB, DXGI_FORMAT_BC2_UNORM,
    DXGI_FORMAT_BC2_UNORM_SRGB, DXGI_FORMAT_BC3_UNORM, DXGI_FORMAT_BC3_UNORM_SRGB,
    DXGI_FORMAT_BC4_SNORM, DXGI_FORMAT_BC4_UNORM, DXGI_FORMAT_BC5_SNORM, DXGI_FORMAT_BC5_UNORM,
    DXGI_FORMAT_BC6H_SF16, DXGI_FORMAT_BC6H_UF16, DXGI_FORMAT_BC7_UNORM,
    DXGI_FORMAT_BC7_UNORM_SRGB, DXGI_FORMAT_D16_UNORM, DXGI_FORMAT_D24_UNORM_S8_UINT,
    DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R11G11B10_FLOAT,
    DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R16G16B16A16_UNORM, DXGI_FORMAT_R16G16_FLOAT,
    DXGI_FORMAT_R16G16_SINT, DXGI_FORMAT_R16G16_SNORM, DXGI_FORMAT_R16G16_UINT,
    DXGI_FORMAT_R16G16_UNORM, DXGI_FORMAT_R16_FLOAT, DXGI_FORMAT_R16_SINT, DXGI_FORMAT_R16_SNORM,
    DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R32G32B32A32_FLOAT,
    DXGI_FORMAT_R32G32B32A32_SINT, DXGI_FORMAT_R32G32B32A32_UINT, DXGI_FORMAT_R32G32B32_FLOAT,
    DXGI_FORMAT_R32G32B32_SINT, DXGI_FORMAT_R32G32B32_UINT, DXGI_FORMAT_R32G32_FLOAT,
    DXGI_FORMAT_R32G32_SINT, DXGI_FORMAT_R32G32_UINT, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32_SINT,
    DXGI_FORMAT_R32_UINT, DXGI_FORMAT_R8G8B8A8_SINT, DXGI_FORMAT_R8G8B8A8_SNORM,
    DXGI_FORMAT_R8G8B8A8_UINT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8_SINT,
    DXGI_FORMAT_R8G8_SNORM, DXGI_FORMAT_R8G8_UINT, DXGI_FORMAT_R8G8_UNORM, DXGI_FORMAT_R8_SINT,
    DXGI_FORMAT_R8_SNORM, DXGI_FORMAT_R8_UINT, DXGI_FORMAT_R8_UNORM,
    DXGI_FORMAT_R9G9B9E5_SHAREDEXP,
    DXGI_FORMAT_R16_TYPELESS, DXGI_FORMAT_R24G8_TYPELESS, DXGI_FORMAT_R32G8X24_TYPELESS,
    DXGI_FORMAT_R24_UNORM_X8_TYPELESS, DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS,
    DXGI_FORMAT_R32_TYPELESS,
};

/// RHI format to DXGI format. `None` means D3D11 has no mapping for the
/// format: pure stencil (`S8_UINT`), the mobile-oriented ASTC/ETC2/EAC
/// family, and `B8G8R8A8_SNORM`/`B8G8R8A8_UINT`, which DXGI never defined.
pub fn dxgi_format(format: Format) -> Option<DXGI_FORMAT> {
    use Format::*;
    Some(match format {
        R8_UNORM => DXGI_FORMAT_R8_UNORM,
        R8_SNORM => DXGI_FORMAT_R8_SNORM,
        R8_UINT => DXGI_FORMAT_R8_UINT,
        R8_SINT => DXGI_FORMAT_R8_SINT,
        R16_UNORM => DXGI_FORMAT_R16_UNORM,
        R16_SNORM => DXGI_FORMAT_R16_SNORM,
        R16_UINT => DXGI_FORMAT_R16_UINT,
        R16_SINT => DXGI_FORMAT_R16_SINT,
        R16_SFLOAT => DXGI_FORMAT_R16_FLOAT,
        RG8_UNORM => DXGI_FORMAT_R8G8_UNORM,
        RG8_SNORM => DXGI_FORMAT_R8G8_SNORM,
        RG8_UINT => DXGI_FORMAT_R8G8_UINT,
        RG8_SINT => DXGI_FORMAT_R8G8_SINT,
        R32_UINT => DXGI_FORMAT_R32_UINT,
        R32_SINT => DXGI_FORMAT_R32_SINT,
        R32_SFLOAT => DXGI_FORMAT_R32_FLOAT,
        RG16_UNORM => DXGI_FORMAT_R16G16_UNORM,
        RG16_SNORM => DXGI_FORMAT_R16G16_SNORM,
        RG16_UINT => DXGI_FORMAT_R16G16_UINT,
        RG16_SINT => DXGI_FORMAT_R16G16_SINT,
        RG16_SFLOAT => DXGI_FORMAT_R16G16_FLOAT,
        RGBA16_UNORM => DXGI_FORMAT_R16G16B16A16_UNORM,
        RGBA16_SFLOAT => DXGI_FORMAT_R16G16B16A16_FLOAT,
        RGBA8_UNORM => DXGI_FORMAT_R8G8B8A8_UNORM,
        RGBA8_SNORM => DXGI_FORMAT_R8G8B8A8_SNORM,
        RGBA8_UINT => DXGI_FORMAT_R8G8B8A8_UINT,
        RGBA8_SINT => DXGI_FORMAT_R8G8B8A8_SINT,
        B8G8R8A8_UNORM => DXGI_FORMAT_B8G8R8A8_UNORM,
        B8G8R8A8_SRGB => DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        R32G32_UINT => DXGI_FORMAT_R32G32_UINT,
        R32G32_SINT => DXGI_FORMAT_R32G32_SINT,
        R32G32_SFLOAT => DXGI_FORMAT_R32G32_FLOAT,
        R32G32B32_UINT => DXGI_FORMAT_R32G32B32_UINT,
        R32G32B32_SINT => DXGI_FORMAT_R32G32B32_SINT,
        R32G32B32_SFLOAT => DXGI_FORMAT_R32G32B32_FLOAT,
        RGBA32_UINT => DXGI_FORMAT_R32G32B32A32_UINT,
        RGBA32_SINT => DXGI_FORMAT_R32G32B32A32_SINT,
        RGBA32_SFLOAT => DXGI_FORMAT_R32G32B32A32_FLOAT,
        // D3D11 has no stencil-only format and no D24 without stencil; the
        // nearest legal equivalent is D24S8.
        D16_UNORM => DXGI_FORMAT_D16_UNORM,
        D24_UNORM => DXGI_FORMAT_D24_UNORM_S8_UINT,
        D32_SFLOAT => DXGI_FORMAT_D32_FLOAT,
        D24_UNORM_S8_UINT => DXGI_FORMAT_D24_UNORM_S8_UINT,
        D32_SFLOAT_S8_UINT => DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
        BC1_RGB_UNORM | BC1_RGBA_UNORM => DXGI_FORMAT_BC1_UNORM,
        BC1_RGB_SRGB | BC1_RGBA_SRGB => DXGI_FORMAT_BC1_UNORM_SRGB,
        BC2_UNORM => DXGI_FORMAT_BC2_UNORM,
        BC2_SRGB => DXGI_FORMAT_BC2_UNORM_SRGB,
        BC3_UNORM => DXGI_FORMAT_BC3_UNORM,
        BC3_SRGB => DXGI_FORMAT_BC3_UNORM_SRGB,
        BC4_UNORM => DXGI_FORMAT_BC4_UNORM,
        BC4_SNORM => DXGI_FORMAT_BC4_SNORM,
        BC5_UNORM => DXGI_FORMAT_BC5_UNORM,
        BC5_SNORM => DXGI_FORMAT_BC5_SNORM,
        BC6H_UFLOAT => DXGI_FORMAT_BC6H_UF16,
        BC6H_SFLOAT => DXGI_FORMAT_BC6H_SF16,
        BC7_UNORM => DXGI_FORMAT_BC7_UNORM,
        BC7_SRGB => DXGI_FORMAT_BC7_UNORM_SRGB,
        B10G11R11_UFLOAT => DXGI_FORMAT_R11G11B10_FLOAT,
        E5B9G9R9_UFLOAT => DXGI_FORMAT_R9G9B9E5_SHAREDEXP,
        A8_UNORM => DXGI_FORMAT_A8_UNORM,
        _ => return None,
    })
}

/// The typeless base format every depth/stencil texture is created with.
///
/// D3D11 allows a DSV and shader read views to attach to the same resource
/// only when the resource itself is typeless; the DSV and the SRV each carry
/// the typed format. `None` for formats with no typeless base: `S8_UINT`, the
/// applications-only format with no created resource.
pub const fn typeless_base(format: Format) -> Option<DXGI_FORMAT> {
    match format {
        Format::D16_UNORM => Some(DXGI_FORMAT_R16_TYPELESS),
        Format::D24_UNORM | Format::D24_UNORM_S8_UINT => Some(DXGI_FORMAT_R24G8_TYPELESS),
        Format::D32_SFLOAT => Some(DXGI_FORMAT_R32_TYPELESS),
        Format::D32_SFLOAT_S8_UINT => Some(DXGI_FORMAT_R32G8X24_TYPELESS),
        _ => None,
    }
}

/// How a shader reads a depth/stencil texture. Complementary to
/// `typeless_base`; the DSV carries the typed format instead.
pub const fn depth_srv_format(format: Format) -> Option<DXGI_FORMAT> {
    match format {
        Format::D16_UNORM => Some(DXGI_FORMAT_R16_UNORM),
        Format::D24_UNORM | Format::D24_UNORM_S8_UINT => Some(DXGI_FORMAT_R24_UNORM_X8_TYPELESS),
        Format::D32_SFLOAT => Some(DXGI_FORMAT_R32_FLOAT),
        Format::D32_SFLOAT_S8_UINT => Some(DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS),
        _ => None,
    }
}


/// Bytes one row of a `width`-wide image occupies, honouring block
/// compression the same way `pipeline::format_size` does.
pub fn row_pitch(format: Format, width: u32) -> u32 {
    match crate::pipeline::format_block(format) {
        Some(block) => width.div_ceil(block.width) * block.bytes,
        None => width * crate::pipeline::format_size(format),
    }
}

/// The primitive topology, as D3D11 names it.
///
/// `None` for the modes D3D11 cannot draw: it has no triangle fan. Returning
/// the adjacent-line value by rank or a list would both be silent wrong-geometry
/// mappings — the caller must refuse a fan explicitly.
pub fn d3d_topology(topology: PrimitiveTopology) -> Option<D3D_PRIMITIVE_TOPOLOGY> {
    Some(match topology {
        PrimitiveTopology::PointList => D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
        PrimitiveTopology::LineList => D3D_PRIMITIVE_TOPOLOGY_LINELIST,
        PrimitiveTopology::LineStrip => D3D_PRIMITIVE_TOPOLOGY_LINESTRIP,
        PrimitiveTopology::TriangleList => D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
        PrimitiveTopology::TriangleStrip => D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
        PrimitiveTopology::TriangleFan => return None,
    })
}

/// The rasterizer's fill mode. `None` for `Point`: D3D11 has no point-mode
/// rasterizer — points are drawn through the point-list topology — and
/// filling instead would draw surfaces where the caller asked for dots.
pub fn d3d_fill_mode(mode: PolygonMode) -> Option<D3D11_FILL_MODE> {
    Some(match mode {
        PolygonMode::Fill => D3D11_FILL_SOLID,
        PolygonMode::Line => D3D11_FILL_WIREFRAME,
        PolygonMode::Point => return None,
    })
}

/// The rasterizer's cull mode. `None` for `FrontAndBack`: D3D11 has no
/// "cull both" mode, and culling nothing would draw the exact surfaces the
/// caller asked to hide.
pub fn d3d_cull_mode(mode: CullMode) -> Option<D3D11_CULL_MODE> {
    Some(match mode {
        CullMode::None => D3D11_CULL_NONE,
        CullMode::Front => D3D11_CULL_FRONT,
        CullMode::Back => D3D11_CULL_BACK,
        CullMode::FrontAndBack => return None,
    })
}

/// The blend factor, as D3D11 names it.
pub fn d3d_blend_factor(factor: BlendFactor) -> D3D11_BLEND {
    match factor {
        BlendFactor::Zero => D3D11_BLEND_ZERO,
        BlendFactor::One => D3D11_BLEND_ONE,
        BlendFactor::SrcColor => D3D11_BLEND_SRC_COLOR,
        BlendFactor::DstColor => D3D11_BLEND_DEST_COLOR,
        BlendFactor::SrcAlpha => D3D11_BLEND_SRC_ALPHA,
        BlendFactor::DstAlpha => D3D11_BLEND_DEST_ALPHA,
        BlendFactor::ConstantColor => D3D11_BLEND_BLEND_FACTOR,
        BlendFactor::ConstantAlpha => D3D11_BLEND_BLEND_FACTOR,
        BlendFactor::SrcAlphaSaturate => D3D11_BLEND_SRC_ALPHA_SAT,
        BlendFactor::Src1Color => D3D11_BLEND_SRC1_COLOR,
        BlendFactor::Src1Alpha => D3D11_BLEND_SRC1_ALPHA,
        BlendFactor::OneMinusSrcColor => D3D11_BLEND_INV_SRC_COLOR,
        BlendFactor::OneMinusDstColor => D3D11_BLEND_INV_DEST_COLOR,
        BlendFactor::OneMinusSrcAlpha => D3D11_BLEND_INV_SRC_ALPHA,
        BlendFactor::OneMinusDstAlpha => D3D11_BLEND_INV_DEST_ALPHA,
        BlendFactor::OneMinusConstantColor => D3D11_BLEND_INV_BLEND_FACTOR,
        BlendFactor::OneMinusConstantAlpha => D3D11_BLEND_INV_BLEND_FACTOR,
        BlendFactor::OneMinusSrc1Color => D3D11_BLEND_INV_SRC1_COLOR,
        BlendFactor::OneMinusSrc1Alpha => D3D11_BLEND_INV_SRC1_ALPHA,
    }
}

/// The blend operation, as D3D11 names it.
pub fn d3d_blend_op(op: BlendOp) -> D3D11_BLEND_OP {
    match op {
        BlendOp::Add => D3D11_BLEND_OP_ADD,
        BlendOp::Subtract => D3D11_BLEND_OP_SUBTRACT,
        BlendOp::ReverseSubtract => D3D11_BLEND_OP_REV_SUBTRACT,
        BlendOp::Min => D3D11_BLEND_OP_MIN,
        BlendOp::Max => D3D11_BLEND_OP_MAX,
    }
}

/// The stencil operation, as D3D11 names it.
pub fn d3d_stencil_op(op: StencilOp) -> D3D11_STENCIL_OP {
    match op {
        StencilOp::Keep => D3D11_STENCIL_OP_KEEP,
        StencilOp::Zero => D3D11_STENCIL_OP_ZERO,
        StencilOp::Replace => D3D11_STENCIL_OP_REPLACE,
        StencilOp::IncrementAndClamp => D3D11_STENCIL_OP_INCR_SAT,
        StencilOp::DecrementAndClamp => D3D11_STENCIL_OP_DECR_SAT,
        StencilOp::Invert => D3D11_STENCIL_OP_INVERT,
        StencilOp::IncrementAndWrap => D3D11_STENCIL_OP_INCR,
        StencilOp::DecrementAndWrap => D3D11_STENCIL_OP_DECR,
    }
}

/// The depth comparison, as D3D11 names it.
pub fn d3d_compare_func(op: CompareOp) -> D3D11_COMPARISON_FUNC {
    match op {
        CompareOp::Never => D3D11_COMPARISON_NEVER,
        CompareOp::Less => D3D11_COMPARISON_LESS,
        CompareOp::Equal => D3D11_COMPARISON_EQUAL,
        CompareOp::LessOrEqual => D3D11_COMPARISON_LESS_EQUAL,
        CompareOp::Greater => D3D11_COMPARISON_GREATER,
        CompareOp::NotEqual => D3D11_COMPARISON_NOT_EQUAL,
        CompareOp::GreaterOrEqual => D3D11_COMPARISON_GREATER_EQUAL,
        CompareOp::Always => D3D11_COMPARISON_ALWAYS,
    }
}

/// The sampler's address mode, as D3D11 names it.
pub fn d3d_address_mode(mode: AddressMode) -> D3D11_TEXTURE_ADDRESS_MODE {
    match mode {
        AddressMode::Repeat => D3D11_TEXTURE_ADDRESS_WRAP,
        AddressMode::MirroredRepeat => D3D11_TEXTURE_ADDRESS_MIRROR,
        AddressMode::ClampToEdge => D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressMode::ClampToBorder => D3D11_TEXTURE_ADDRESS_BORDER,
        AddressMode::MirrorClampToEdge => D3D11_TEXTURE_ADDRESS_MIRROR_ONCE,
    }
}

/// The sampler's filter, as D3D11 names it.
///
/// D3D11 packs the minification, magnification and mip-level filters into a
/// single value: the bit `0x10` says "linear when minifying", `0x4` says
/// "linear when magnifying" and `0x1` says "linear between mip levels".
pub fn d3d_filter(min: FilterMode, mag: FilterMode, mip: FilterMode) -> D3D11_FILTER {
    let linear = |mode: FilterMode| matches!(mode, FilterMode::Linear);
    let mut bits = 0i32;
    if linear(min) {
        bits |= 0x10;
    }
    if linear(mag) {
        bits |= 0x4;
    }
    if linear(mip) {
        bits |= 0x1;
    }
    D3D11_FILTER(bits)
}

/// The sampler's filter with a comparison against the comparison value, as
/// D3D11 names it.
///
/// A comparison filter is the same filter with the `0x80` bit set.
pub fn d3d_comparison_filter(min: FilterMode, mag: FilterMode, mip: FilterMode) -> D3D11_FILTER {
    D3D11_FILTER(d3d_filter(min, mag, mip).0 | 0x80)
}

/// The index type, as the format D3D11's `IASetIndexBuffer` takes.
///
/// D3D11 has no index-format enum of its own: an index buffer's element size
/// is given as a `DXGI_FORMAT`.
pub fn d3d_index_format(index_type: IndexType) -> DXGI_FORMAT {
    match index_type {
        IndexType::U16 => DXGI_FORMAT_R16_UINT,
        IndexType::U32 => DXGI_FORMAT_R32_UINT,
    }
}

/// The border colour, as the four floats D3D11's sampler takes.
pub fn d3d_border_color(color: BorderColor) -> [f32; 4] {
    match color {
        BorderColor::FloatTransparentBlack => [0.0, 0.0, 0.0, 0.0],
        BorderColor::IntTransparentBlack => [0.0, 0.0, 0.0, 0.0],
        BorderColor::FloatOpaqueBlack => [0.0, 0.0, 0.0, 1.0],
        BorderColor::IntOpaqueBlack => [0.0, 0.0, 0.0, 1.0],
        BorderColor::FloatOpaqueWhite => [1.0, 1.0, 1.0, 1.0],
        BorderColor::IntOpaqueWhite => [1.0, 1.0, 1.0, 1.0],
    }
}

/// The colour write mask, as D3D11 names it.
pub fn d3d_color_write(mask: ColorComponentFlags) -> D3D11_COLOR_WRITE_ENABLE {
    let mut bits = 0i32;
    if mask.contains(ColorComponentFlags::R) {
        bits |= D3D11_COLOR_WRITE_ENABLE_RED.0;
    }
    if mask.contains(ColorComponentFlags::G) {
        bits |= D3D11_COLOR_WRITE_ENABLE_GREEN.0;
    }
    if mask.contains(ColorComponentFlags::B) {
        bits |= D3D11_COLOR_WRITE_ENABLE_BLUE.0;
    }
    if mask.contains(ColorComponentFlags::A) {
        bits |= D3D11_COLOR_WRITE_ENABLE_ALPHA.0;
    }
    D3D11_COLOR_WRITE_ENABLE(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_common_format_maps_to_the_dxgi_format_of_the_same_name() {
        // The names line up for these, so a mismatch would mean a typo in the
        // table rather than a deliberate difference.
        assert_eq!(dxgi_format(Format::RGBA8_UNORM).unwrap().0, 28);
        assert_eq!(dxgi_format(Format::R32_SFLOAT).unwrap().0, 41);
        assert_eq!(dxgi_format(Format::D32_SFLOAT).unwrap().0, 40);
    }

    #[test]
    fn a_format_d3d11_has_no_name_for_has_no_mapping() {
        // D3D11 has no stencil-only format, and the ASTC/ETC2 family is
        // mobile-only. Saying "no mapping" is the honest answer; picking
        // something close would produce a texture the GPU reads wrong.
        assert!(dxgi_format(Format::S8_UINT).is_none());
        assert!(dxgi_format(Format::ASTC_4x4_UNORM).is_none());
    }

    #[test]
    fn a_block_compressed_row_is_counted_in_blocks() {
        // A BC1 block is 4x4 texels in 8 bytes, so a 10-wide image is 3
        // blocks wide: 24 bytes, not 20.
        assert_eq!(row_pitch(Format::BC1_RGB_UNORM, 10), 24);
        assert_eq!(row_pitch(Format::RGBA8_UNORM, 10), 40);
    }

    #[test]
    fn the_filter_packs_one_bit_per_axis() {
        // D3D11_FILTER: 0x10 minifies linearly, 0x4 magnifies linearly, 0x1
        // filters between mip levels.
        assert_eq!(
            d3d_filter(FilterMode::Linear, FilterMode::Linear, FilterMode::Linear).0,
            0x15
        );
        assert_eq!(
            d3d_filter(
                FilterMode::Nearest,
                FilterMode::Nearest,
                FilterMode::Nearest
            )
            .0,
            0
        );
        assert_eq!(
            d3d_filter(FilterMode::Linear, FilterMode::Nearest, FilterMode::Nearest).0,
            0x10
        );
        assert_eq!(
            d3d_filter(FilterMode::Nearest, FilterMode::Linear, FilterMode::Nearest).0,
            0x4
        );
        assert_eq!(
            d3d_filter(FilterMode::Nearest, FilterMode::Nearest, FilterMode::Linear).0,
            0x1
        );
    }

    #[test]
    fn a_comparison_filter_is_the_same_filter_with_one_more_bit() {
        let plain = d3d_filter(FilterMode::Linear, FilterMode::Linear, FilterMode::Linear).0;
        let comparison =
            d3d_comparison_filter(FilterMode::Linear, FilterMode::Linear, FilterMode::Linear).0;
        assert_eq!(comparison, plain | 0x80);
    }

    #[test]
    fn the_index_type_becomes_the_matching_unsigned_dxgi_format() {
        assert_eq!(d3d_index_format(IndexType::U16).0, 57);
        assert_eq!(d3d_index_format(IndexType::U32).0, 42);
    }

    #[test]
    fn the_colour_write_mask_is_the_union_of_the_written_channels() {
        let all = d3d_color_write(
            ColorComponentFlags::R
                | ColorComponentFlags::G
                | ColorComponentFlags::B
                | ColorComponentFlags::A,
        );
        assert_eq!(all.0, 15);
        let rgb = d3d_color_write(
            ColorComponentFlags::R | ColorComponentFlags::G | ColorComponentFlags::B,
        );
        assert_eq!(rgb.0, 7);
        assert_eq!(d3d_color_write(ColorComponentFlags::empty()).0, 0);
    }

    #[test]
    fn the_topology_refuses_the_fan_it_cannot_draw() {
        assert_eq!(d3d_topology(PrimitiveTopology::TriangleList).unwrap().0, 4);
        assert_eq!(d3d_topology(PrimitiveTopology::TriangleStrip).unwrap().0, 5);
        assert_eq!(d3d_topology(PrimitiveTopology::PointList).unwrap().0, 1);
        // D3D11 has no fan topology; mapping it to rank 6 would silently
        // select LINELIST_ADJ, which is not a fan.
        assert!(d3d_topology(PrimitiveTopology::TriangleFan).is_none());
    }

    #[test]
    fn the_point_fill_mode_and_double_culling_are_refused() {
        assert!(d3d_fill_mode(PolygonMode::Point).is_none());
        assert_eq!(d3d_fill_mode(PolygonMode::Fill).unwrap().0, 3);
        assert!(d3d_cull_mode(CullMode::FrontAndBack).is_none());
        assert_eq!(d3d_cull_mode(CullMode::Back).unwrap().0, 3);
    }
}
