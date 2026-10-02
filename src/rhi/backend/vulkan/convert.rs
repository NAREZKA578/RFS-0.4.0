//! Vulkan conversion helpers: RHI types to `vk` values.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::resource::{AddressMode, BorderColor, CompareOp, FilterMode, TextureViewType};
use crate::types::*;
use ash::vk;

/// Convert an RHI format to a Vulkan format. Returns `None` when no Vulkan
/// equivalent exists.
pub(crate) fn format(fmt: Format) -> Option<vk::Format> {
    use Format::*;
    use vk::Format as Vk;
    Some(match fmt {
        R8_UNORM => Vk::R8_UNORM,
        R8_SNORM => Vk::R8_SNORM,
        R8_UINT => Vk::R8_UINT,
        R8_SINT => Vk::R8_SINT,
        R16_UNORM => Vk::R16_UNORM,
        R16_SNORM => Vk::R16_SNORM,
        R16_UINT => Vk::R16_UINT,
        R16_SINT => Vk::R16_SINT,
        R16_SFLOAT => Vk::R16_SFLOAT,
        RG8_UNORM => Vk::R8G8_UNORM,
        RG8_SNORM => Vk::R8G8_SNORM,
        RG8_UINT => Vk::R8G8_UINT,
        RG8_SINT => Vk::R8G8_SINT,
        R32_UINT => Vk::R32_UINT,
        R32_SINT => Vk::R32_SINT,
        R32_SFLOAT => Vk::R32_SFLOAT,
        RG16_UNORM => Vk::R16G16_UNORM,
        RG16_SNORM => Vk::R16G16_SNORM,
        RG16_UINT => Vk::R16G16_UINT,
        RG16_SINT => Vk::R16G16_SINT,
        RG16_SFLOAT => Vk::R16G16_SFLOAT,
        RGBA16_UNORM => Vk::R16G16B16A16_UNORM,
        RGBA16_SFLOAT => Vk::R16G16B16A16_SFLOAT,
        RGBA8_UNORM => Vk::R8G8B8A8_UNORM,
        RGBA8_SNORM => Vk::R8G8B8A8_SNORM,
        RGBA8_UINT => Vk::R8G8B8A8_UINT,
        RGBA8_SINT => Vk::R8G8B8A8_SINT,
        // Presentation formats. A swapchain surface picks the format, so these
        // have to exist on the RHI side or the pass cannot be described.
        B8G8R8A8_UNORM => Vk::B8G8R8A8_UNORM,
        B8G8R8A8_SRGB => Vk::B8G8R8A8_SRGB,
        R32G32_UINT => Vk::R32G32_UINT,
        R32G32_SINT => Vk::R32G32_SINT,
        R32G32_SFLOAT => Vk::R32G32_SFLOAT,
        R32G32B32_UINT => Vk::R32G32B32_UINT,
        R32G32B32_SINT => Vk::R32G32B32_SINT,
        R32G32B32_SFLOAT => Vk::R32G32B32_SFLOAT,
        RGBA32_UINT => Vk::R32G32B32A32_UINT,
        RGBA32_SINT => Vk::R32G32B32A32_SINT,
        RGBA32_SFLOAT => Vk::R32G32B32A32_SFLOAT,
        D16_UNORM => Vk::D16_UNORM,
        D24_UNORM => Vk::D24_UNORM_S8_UINT,
        D32_SFLOAT => Vk::D32_SFLOAT,
        D24_UNORM_S8_UINT => Vk::D24_UNORM_S8_UINT,
        D32_SFLOAT_S8_UINT => Vk::D32_SFLOAT_S8_UINT,
        S8_UINT => Vk::S8_UINT,
        BC1_RGB_UNORM => Vk::BC1_RGB_UNORM_BLOCK,
        BC1_RGB_SRGB => Vk::BC1_RGB_SRGB_BLOCK,
        BC1_RGBA_UNORM => Vk::BC1_RGBA_UNORM_BLOCK,
        BC1_RGBA_SRGB => Vk::BC1_RGBA_SRGB_BLOCK,
        BC2_UNORM => Vk::BC2_UNORM_BLOCK,
        BC2_SRGB => Vk::BC2_SRGB_BLOCK,
        BC3_UNORM => Vk::BC3_UNORM_BLOCK,
        BC3_SRGB => Vk::BC3_SRGB_BLOCK,
        BC4_UNORM => Vk::BC4_UNORM_BLOCK,
        BC4_SNORM => Vk::BC4_SNORM_BLOCK,
        BC5_UNORM => Vk::BC5_UNORM_BLOCK,
        BC5_SNORM => Vk::BC5_SNORM_BLOCK,
        BC6H_UFLOAT => Vk::BC6H_UFLOAT_BLOCK,
        BC6H_SFLOAT => Vk::BC6H_SFLOAT_BLOCK,
        BC7_UNORM => Vk::BC7_UNORM_BLOCK,
        BC7_SRGB => Vk::BC7_SRGB_BLOCK,
        ASTC_4x4_UNORM => Vk::ASTC_4X4_UNORM_BLOCK,
        ASTC_4x4_SRGB => Vk::ASTC_4X4_SRGB_BLOCK,
        ASTC_5x4_UNORM => Vk::ASTC_5X4_UNORM_BLOCK,
        ASTC_5x4_SRGB => Vk::ASTC_5X4_SRGB_BLOCK,
        ASTC_5x5_UNORM => Vk::ASTC_5X5_UNORM_BLOCK,
        ASTC_5x5_SRGB => Vk::ASTC_5X5_SRGB_BLOCK,
        ASTC_6x5_UNORM => Vk::ASTC_6X5_UNORM_BLOCK,
        ASTC_6x5_SRGB => Vk::ASTC_6X5_SRGB_BLOCK,
        ASTC_6x6_UNORM => Vk::ASTC_6X6_UNORM_BLOCK,
        ASTC_6x6_SRGB => Vk::ASTC_6X6_SRGB_BLOCK,
        ASTC_8x5_UNORM => Vk::ASTC_8X5_UNORM_BLOCK,
        ASTC_8x5_SRGB => Vk::ASTC_8X5_SRGB_BLOCK,
        ASTC_8x6_UNORM => Vk::ASTC_8X6_UNORM_BLOCK,
        ASTC_8x6_SRGB => Vk::ASTC_8X6_SRGB_BLOCK,
        ASTC_8x8_UNORM => Vk::ASTC_8X8_UNORM_BLOCK,
        ASTC_8x8_SRGB => Vk::ASTC_8X8_SRGB_BLOCK,
        ETC2_R8G8B8_UNORM => Vk::ETC2_R8G8B8_UNORM_BLOCK,
        ETC2_R8G8B8_SRGB => Vk::ETC2_R8G8B8_SRGB_BLOCK,
        ETC2_R8G8B8A1_UNORM => Vk::ETC2_R8G8B8A1_UNORM_BLOCK,
        ETC2_R8G8B8A1_SRGB => Vk::ETC2_R8G8B8A1_SRGB_BLOCK,
        ETC2_R8G8B8A8_UNORM => Vk::ETC2_R8G8B8A8_UNORM_BLOCK,
        ETC2_R8G8B8A8_SRGB => Vk::ETC2_R8G8B8A8_SRGB_BLOCK,
        EAC_R11_UNORM => Vk::EAC_R11_UNORM_BLOCK,
        EAC_R11_SNORM => Vk::EAC_R11_SNORM_BLOCK,
        EAC_R11G11_UNORM => Vk::EAC_R11G11_UNORM_BLOCK,
        EAC_R11G11_SNORM => Vk::EAC_R11G11_SNORM_BLOCK,
        B10G11R11_UFLOAT => Vk::B10G11R11_UFLOAT_PACK32,
        E5B9G9R9_UFLOAT => Vk::E5B9G9R9_UFLOAT_PACK32,
        A8_UNORM => return None,
    })
}

/// Convert a Vulkan format back to the RHI format it came from.
///
/// The forward table is not injective — several RHI formats share a Vulkan
/// code — so this returns the first RHI format that maps to `vk_format`. The
/// alternatives differ in interpretation rather than in size and layout, so
/// picking one is safe for the presentation case, which is the only caller: a
/// surface format has to be described back to the RHI so a render pass can be
/// built for it.
///
/// `None` means the surface offered something the RHI cannot express, which is
/// reported rather than papered over with a near-miss format.
pub(crate) fn format_from(vk_format: vk::Format) -> Option<Format> {
    use crate::types::Format::*;
    // Ordered so the unambiguous, commonly-negotiated formats are tried first.
    const CANDIDATES: [Format; 14] = [
        B8G8R8A8_SRGB,
        B8G8R8A8_UNORM,
        RGBA8_UNORM,
        RGBA8_SNORM,
        RGBA8_UINT,
        RGBA8_SINT,
        RGBA16_UNORM,
        RGBA16_SFLOAT,
        B10G11R11_UFLOAT,
        E5B9G9R9_UFLOAT,
        D32_SFLOAT,
        D24_UNORM_S8_UINT,
        D16_UNORM,
        D24_UNORM,
    ];
    CANDIDATES
        .into_iter()
        .find(|candidate| format(*candidate) == Some(vk_format))
}

/// Convert RHI buffer usage flags to Vulkan usage flags.
pub(crate) fn buffer_usage(usage: BufferUsage) -> vk::BufferUsageFlags {
    let mut flags = vk::BufferUsageFlags::empty();
    if usage.contains(BufferUsage::TRANSFER_SRC) {
        flags |= vk::BufferUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(BufferUsage::TRANSFER_DST) {
        flags |= vk::BufferUsageFlags::TRANSFER_DST;
    }
    if usage.contains(BufferUsage::UNIFORM) {
        flags |= vk::BufferUsageFlags::UNIFORM_BUFFER;
    }
    if usage.contains(BufferUsage::STORAGE) {
        flags |= vk::BufferUsageFlags::STORAGE_BUFFER;
    }
    if usage.contains(BufferUsage::INDEX) {
        flags |= vk::BufferUsageFlags::INDEX_BUFFER;
    }
    if usage.contains(BufferUsage::VERTEX) {
        flags |= vk::BufferUsageFlags::VERTEX_BUFFER;
    }
    if usage.contains(BufferUsage::INDIRECT) {
        flags |= vk::BufferUsageFlags::INDIRECT_BUFFER;
    }
    if usage.contains(BufferUsage::SHADER_DEVICE_ADDRESS) {
        flags |= vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;
    }
    flags
}

/// Convert RHI texture usage flags to Vulkan usage flags.
pub(crate) fn texture_usage(usage: TextureUsage) -> vk::ImageUsageFlags {
    let mut flags = vk::ImageUsageFlags::empty();
    if usage.contains(TextureUsage::TRANSFER_SRC) {
        flags |= vk::ImageUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(TextureUsage::TRANSFER_DST) {
        flags |= vk::ImageUsageFlags::TRANSFER_DST;
    }
    if usage.contains(TextureUsage::SAMPLED) {
        flags |= vk::ImageUsageFlags::SAMPLED;
    }
    if usage.contains(TextureUsage::STORAGE) {
        flags |= vk::ImageUsageFlags::STORAGE;
    }
    if usage.contains(TextureUsage::COLOR_ATTACHMENT) {
        flags |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
    }
    if usage.contains(TextureUsage::DEPTH_STENCIL_ATTACHMENT) {
        flags |= vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT;
    }
    flags
}

/// Convert RHI sample count to Vulkan sample count flags.
pub(crate) fn sample_count(count: SampleCount) -> vk::SampleCountFlags {
    match count {
        SampleCount::X1 => vk::SampleCountFlags::TYPE_1,
        SampleCount::X2 => vk::SampleCountFlags::TYPE_2,
        SampleCount::X4 => vk::SampleCountFlags::TYPE_4,
        SampleCount::X8 => vk::SampleCountFlags::TYPE_8,
        SampleCount::X16 => vk::SampleCountFlags::TYPE_16,
        SampleCount::X32 => vk::SampleCountFlags::TYPE_32,
        SampleCount::X64 => vk::SampleCountFlags::TYPE_64,
    }
}

/// Convert RHI texture dimensions to a Vulkan image type.
pub(crate) fn image_type(dim: TextureDimensions) -> vk::ImageType {
    match dim {
        TextureDimensions::D1 => vk::ImageType::TYPE_1D,
        TextureDimensions::D2 | TextureDimensions::Cube => vk::ImageType::TYPE_2D,
        TextureDimensions::D3 => vk::ImageType::TYPE_3D,
    }
}

/// Convert RHI texture view type to a Vulkan image view type.
pub(crate) fn image_view_type(view_type: TextureViewType) -> vk::ImageViewType {
    match view_type {
        TextureViewType::D1 => vk::ImageViewType::TYPE_1D,
        TextureViewType::D1Array => vk::ImageViewType::TYPE_1D_ARRAY,
        TextureViewType::D2 => vk::ImageViewType::TYPE_2D,
        TextureViewType::D2Array => vk::ImageViewType::TYPE_2D_ARRAY,
        TextureViewType::D3 => vk::ImageViewType::TYPE_3D,
        TextureViewType::Cube => vk::ImageViewType::CUBE,
        TextureViewType::CubeArray => vk::ImageViewType::CUBE_ARRAY,
    }
}

/// Convert RHI texture aspect flags to Vulkan image aspect flags.
pub(crate) fn image_aspects(aspects: TextureAspectFlags) -> vk::ImageAspectFlags {
    let mut flags = vk::ImageAspectFlags::empty();
    if aspects.contains(TextureAspectFlags::COLOR) {
        flags |= vk::ImageAspectFlags::COLOR;
    }
    if aspects.contains(TextureAspectFlags::DEPTH) {
        flags |= vk::ImageAspectFlags::DEPTH;
    }
    if aspects.contains(TextureAspectFlags::STENCIL) {
        flags |= vk::ImageAspectFlags::STENCIL;
    }
    flags
}

/// Convert RHI filter mode to a Vulkan filter.
pub(crate) fn filter(mode: FilterMode) -> vk::Filter {
    match mode {
        FilterMode::Nearest => vk::Filter::NEAREST,
        FilterMode::Linear => vk::Filter::LINEAR,
    }
}

/// Convert RHI filter mode to a Vulkan mipmap mode.
pub(crate) fn mipmap_mode(mode: FilterMode) -> vk::SamplerMipmapMode {
    match mode {
        FilterMode::Nearest => vk::SamplerMipmapMode::NEAREST,
        FilterMode::Linear => vk::SamplerMipmapMode::LINEAR,
    }
}

/// Convert RHI address mode to a Vulkan sampler address mode.
pub(crate) fn address_mode(mode: AddressMode) -> vk::SamplerAddressMode {
    match mode {
        AddressMode::Repeat => vk::SamplerAddressMode::REPEAT,
        AddressMode::MirroredRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
        AddressMode::ClampToEdge => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        AddressMode::ClampToBorder => vk::SamplerAddressMode::CLAMP_TO_BORDER,
        AddressMode::MirrorClampToEdge => vk::SamplerAddressMode::MIRROR_CLAMP_TO_EDGE,
    }
}

/// Convert RHI compare operation to a Vulkan compare operation.
pub(crate) fn compare_op(op: CompareOp) -> vk::CompareOp {
    match op {
        CompareOp::Never => vk::CompareOp::NEVER,
        CompareOp::Less => vk::CompareOp::LESS,
        CompareOp::Equal => vk::CompareOp::EQUAL,
        CompareOp::LessOrEqual => vk::CompareOp::LESS_OR_EQUAL,
        CompareOp::Greater => vk::CompareOp::GREATER,
        CompareOp::NotEqual => vk::CompareOp::NOT_EQUAL,
        CompareOp::GreaterOrEqual => vk::CompareOp::GREATER_OR_EQUAL,
        CompareOp::Always => vk::CompareOp::ALWAYS,
    }
}

/// Convert RHI border color to a Vulkan border color.
pub(crate) fn border_color(color: BorderColor) -> vk::BorderColor {
    match color {
        BorderColor::FloatTransparentBlack => vk::BorderColor::FLOAT_TRANSPARENT_BLACK,
        BorderColor::IntTransparentBlack => vk::BorderColor::INT_TRANSPARENT_BLACK,
        BorderColor::FloatOpaqueBlack => vk::BorderColor::FLOAT_OPAQUE_BLACK,
        BorderColor::IntOpaqueBlack => vk::BorderColor::INT_OPAQUE_BLACK,
        BorderColor::FloatOpaqueWhite => vk::BorderColor::FLOAT_OPAQUE_WHITE,
        BorderColor::IntOpaqueWhite => vk::BorderColor::INT_OPAQUE_WHITE,
    }
}

/// Convert RHI sharing mode to a Vulkan sharing mode.
pub(crate) fn sharing_mode(mode: SharingMode) -> vk::SharingMode {
    match mode {
        SharingMode::Exclusive => vk::SharingMode::EXCLUSIVE,
        SharingMode::Concurrent => vk::SharingMode::CONCURRENT,
    }
}

/// Convert RHI image layout to a Vulkan image layout.
pub(crate) fn texture_layout(layout: crate::resource::TextureLayout) -> vk::ImageLayout {
    use crate::resource::TextureLayout;
    match layout {
        TextureLayout::Undefined => vk::ImageLayout::UNDEFINED,
        TextureLayout::General => vk::ImageLayout::GENERAL,
        TextureLayout::ColorAttachmentOptimal => vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        TextureLayout::TransferSrcOptimal => vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        TextureLayout::TransferDstOptimal => vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        TextureLayout::ShaderReadOnlyOptimal => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        TextureLayout::DepthStencilAttachmentOptimal => {
            vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL
        }
        TextureLayout::DepthStencilReadOnlyOptimal => {
            vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL
        }
        TextureLayout::DepthAttachmentOptimal => vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
        TextureLayout::DepthReadOnlyOptimal => vk::ImageLayout::DEPTH_READ_ONLY_OPTIMAL,
        TextureLayout::StencilAttachmentOptimal => vk::ImageLayout::STENCIL_ATTACHMENT_OPTIMAL,
        TextureLayout::StencilReadOnlyOptimal => vk::ImageLayout::STENCIL_READ_ONLY_OPTIMAL,
        TextureLayout::Preinitialized => vk::ImageLayout::PREINITIALIZED,
        TextureLayout::PresentSrc => vk::ImageLayout::PRESENT_SRC_KHR,
    }
}

/// Convert a render pass load operation to a Vulkan attachment load operation.
pub(crate) fn attachment_load_op(op: crate::command::pass::render::LoadOp) -> vk::AttachmentLoadOp {
    use crate::command::pass::render::LoadOp;
    match op {
        LoadOp::Load => vk::AttachmentLoadOp::LOAD,
        LoadOp::Clear => vk::AttachmentLoadOp::CLEAR,
        LoadOp::DontCare => vk::AttachmentLoadOp::DONT_CARE,
    }
}

/// Convert a render pass store operation to a Vulkan attachment store operation.
pub(crate) fn attachment_store_op(
    op: crate::command::pass::render::StoreOp,
) -> vk::AttachmentStoreOp {
    use crate::command::pass::render::StoreOp;
    match op {
        StoreOp::Store => vk::AttachmentStoreOp::STORE,
        StoreOp::DontCare => vk::AttachmentStoreOp::DONT_CARE,
    }
}

/// Convert RHI primitive topology to a Vulkan primitive topology.
pub(crate) fn topology(topology: crate::pipeline::PrimitiveTopology) -> vk::PrimitiveTopology {
    use crate::pipeline::PrimitiveTopology;
    match topology {
        PrimitiveTopology::PointList => vk::PrimitiveTopology::POINT_LIST,
        PrimitiveTopology::LineList => vk::PrimitiveTopology::LINE_LIST,
        PrimitiveTopology::LineStrip => vk::PrimitiveTopology::LINE_STRIP,
        PrimitiveTopology::TriangleList => vk::PrimitiveTopology::TRIANGLE_LIST,
        PrimitiveTopology::TriangleStrip => vk::PrimitiveTopology::TRIANGLE_STRIP,
        PrimitiveTopology::TriangleFan => vk::PrimitiveTopology::TRIANGLE_FAN,
    }
}

/// Convert RHI polygon mode to a Vulkan polygon mode.
pub(crate) fn polygon_mode(mode: crate::pipeline::PolygonMode) -> vk::PolygonMode {
    use crate::pipeline::PolygonMode;
    match mode {
        PolygonMode::Fill => vk::PolygonMode::FILL,
        PolygonMode::Line => vk::PolygonMode::LINE,
        PolygonMode::Point => vk::PolygonMode::POINT,
    }
}

/// Convert RHI cull mode to Vulkan cull mode flags.
pub(crate) fn cull_mode(mode: crate::pipeline::CullMode) -> vk::CullModeFlags {
    use crate::pipeline::CullMode;
    match mode {
        CullMode::None => vk::CullModeFlags::NONE,
        CullMode::Front => vk::CullModeFlags::FRONT,
        CullMode::Back => vk::CullModeFlags::BACK,
        CullMode::FrontAndBack => vk::CullModeFlags::FRONT_AND_BACK,
    }
}

/// Convert RHI front face to a Vulkan front face.
pub(crate) fn front_face(face: crate::pipeline::FrontFace) -> vk::FrontFace {
    use crate::pipeline::FrontFace;
    match face {
        FrontFace::CounterClockwise => vk::FrontFace::COUNTER_CLOCKWISE,
        FrontFace::Clockwise => vk::FrontFace::CLOCKWISE,
    }
}

/// Convert an RHI index type to a Vulkan index type.
pub(crate) fn index_type(index_type: IndexType) -> vk::IndexType {
    match index_type {
        IndexType::U16 => vk::IndexType::UINT16,
        IndexType::U32 => vk::IndexType::UINT32,
    }
}

/// Convert RHI vertex input rate to a Vulkan vertex input rate.
pub(crate) fn vertex_input_rate(rate: crate::pipeline::VertexInputRate) -> vk::VertexInputRate {
    use crate::pipeline::VertexInputRate;
    match rate {
        VertexInputRate::Vertex => vk::VertexInputRate::VERTEX,
        VertexInputRate::Instance => vk::VertexInputRate::INSTANCE,
    }
}

/// Convert RHI blend factor to a Vulkan blend factor.
pub(crate) fn blend_factor(factor: crate::pipeline::BlendFactor) -> vk::BlendFactor {
    use crate::pipeline::BlendFactor;
    match factor {
        BlendFactor::Zero => vk::BlendFactor::ZERO,
        BlendFactor::One => vk::BlendFactor::ONE,
        BlendFactor::SrcColor => vk::BlendFactor::SRC_COLOR,
        BlendFactor::DstColor => vk::BlendFactor::DST_COLOR,
        BlendFactor::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
        BlendFactor::DstAlpha => vk::BlendFactor::DST_ALPHA,
        BlendFactor::ConstantColor => vk::BlendFactor::CONSTANT_COLOR,
        BlendFactor::ConstantAlpha => vk::BlendFactor::CONSTANT_ALPHA,
        BlendFactor::SrcAlphaSaturate => vk::BlendFactor::SRC_ALPHA_SATURATE,
        BlendFactor::Src1Color => vk::BlendFactor::SRC1_COLOR,
        BlendFactor::Src1Alpha => vk::BlendFactor::SRC1_ALPHA,
        BlendFactor::OneMinusSrcColor => vk::BlendFactor::ONE_MINUS_SRC_COLOR,
        BlendFactor::OneMinusDstColor => vk::BlendFactor::ONE_MINUS_DST_COLOR,
        BlendFactor::OneMinusSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        BlendFactor::OneMinusDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
        BlendFactor::OneMinusConstantColor => vk::BlendFactor::ONE_MINUS_CONSTANT_COLOR,
        BlendFactor::OneMinusConstantAlpha => vk::BlendFactor::ONE_MINUS_CONSTANT_ALPHA,
        BlendFactor::OneMinusSrc1Color => vk::BlendFactor::ONE_MINUS_SRC1_COLOR,
        BlendFactor::OneMinusSrc1Alpha => vk::BlendFactor::ONE_MINUS_SRC1_ALPHA,
    }
}

/// Convert RHI blend operation to a Vulkan blend operation.
pub(crate) fn blend_op(op: crate::pipeline::BlendOp) -> vk::BlendOp {
    use crate::pipeline::BlendOp;
    match op {
        BlendOp::Add => vk::BlendOp::ADD,
        BlendOp::Subtract => vk::BlendOp::SUBTRACT,
        BlendOp::ReverseSubtract => vk::BlendOp::REVERSE_SUBTRACT,
        BlendOp::Min => vk::BlendOp::MIN,
        BlendOp::Max => vk::BlendOp::MAX,
    }
}

/// Convert RHI logic operation to a Vulkan logic operation.
pub(crate) fn logic_op(op: crate::pipeline::LogicOp) -> vk::LogicOp {
    use crate::pipeline::LogicOp;
    match op {
        LogicOp::Clear => vk::LogicOp::CLEAR,
        LogicOp::And => vk::LogicOp::AND,
        LogicOp::AndReverse => vk::LogicOp::AND_REVERSE,
        LogicOp::Copy => vk::LogicOp::COPY,
        LogicOp::AndInverted => vk::LogicOp::AND_INVERTED,
        LogicOp::NoOp => vk::LogicOp::NO_OP,
        LogicOp::Xor => vk::LogicOp::XOR,
        LogicOp::Or => vk::LogicOp::OR,
        LogicOp::Nor => vk::LogicOp::NOR,
        LogicOp::Equivalent => vk::LogicOp::EQUIVALENT,
        LogicOp::Invert => vk::LogicOp::INVERT,
        LogicOp::OrReverse => vk::LogicOp::OR_REVERSE,
        LogicOp::CopyInverted => vk::LogicOp::COPY_INVERTED,
        LogicOp::OrInverted => vk::LogicOp::OR_INVERTED,
        LogicOp::Nand => vk::LogicOp::NAND,
        LogicOp::Set => vk::LogicOp::SET,
    }
}

/// Convert RHI stencil operation to a Vulkan stencil operation.
pub(crate) fn stencil_op(op: crate::pipeline::StencilOp) -> vk::StencilOp {
    use crate::pipeline::StencilOp;
    match op {
        StencilOp::Keep => vk::StencilOp::KEEP,
        StencilOp::Zero => vk::StencilOp::ZERO,
        StencilOp::Replace => vk::StencilOp::REPLACE,
        StencilOp::IncrementAndClamp => vk::StencilOp::INCREMENT_AND_CLAMP,
        StencilOp::DecrementAndClamp => vk::StencilOp::DECREMENT_AND_CLAMP,
        StencilOp::Invert => vk::StencilOp::INVERT,
        StencilOp::IncrementAndWrap => vk::StencilOp::INCREMENT_AND_WRAP,
        StencilOp::DecrementAndWrap => vk::StencilOp::DECREMENT_AND_WRAP,
    }
}

/// Convert RHI color component write flags to Vulkan flags.
pub(crate) fn color_components(flags: crate::types::ColorComponentFlags) -> vk::ColorComponentFlags {
    let mut out = vk::ColorComponentFlags::empty();
    if flags.contains(crate::types::ColorComponentFlags::R) {
        out |= vk::ColorComponentFlags::R;
    }
    if flags.contains(crate::types::ColorComponentFlags::G) {
        out |= vk::ColorComponentFlags::G;
    }
    if flags.contains(crate::types::ColorComponentFlags::B) {
        out |= vk::ColorComponentFlags::B;
    }
    if flags.contains(crate::types::ColorComponentFlags::A) {
        out |= vk::ColorComponentFlags::A;
    }
    out
}

/// Convert RHI descriptor types to Vulkan descriptor types.
pub(crate) fn descriptor_type(ty: crate::descriptor::DescriptorType) -> vk::DescriptorType {
    use crate::descriptor::DescriptorType;
    match ty {
        DescriptorType::Sampler => vk::DescriptorType::SAMPLER,
        DescriptorType::UniformBuffer => vk::DescriptorType::UNIFORM_BUFFER,
        DescriptorType::DynamicUniformBuffer => vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
        DescriptorType::StorageBuffer => vk::DescriptorType::STORAGE_BUFFER,
        DescriptorType::DynamicStorageBuffer => vk::DescriptorType::STORAGE_BUFFER_DYNAMIC,
        DescriptorType::SampledTexture => vk::DescriptorType::SAMPLED_IMAGE,
        DescriptorType::StorageTexture => vk::DescriptorType::STORAGE_IMAGE,
        DescriptorType::UniformTexelBuffer => vk::DescriptorType::UNIFORM_TEXEL_BUFFER,
        DescriptorType::StorageTexelBuffer => vk::DescriptorType::STORAGE_TEXEL_BUFFER,
        DescriptorType::CombinedTextureSampler => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
        DescriptorType::InputAttachment => vk::DescriptorType::INPUT_ATTACHMENT,
        DescriptorType::InlineUniformBlock => vk::DescriptorType::INLINE_UNIFORM_BLOCK_EXT,
        DescriptorType::AccelerationStructure => vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
    }
}

/// Convert RHI shader stages to Vulkan shader stage flags.
pub(crate) fn shader_stage_flags(stage: crate::types::ShaderStage) -> vk::ShaderStageFlags {
    use crate::types::ShaderStage;
    let mut flags = vk::ShaderStageFlags::empty();
    if stage.contains(ShaderStage::VERTEX) {
        flags |= vk::ShaderStageFlags::VERTEX;
    }
    if stage.contains(ShaderStage::FRAGMENT) {
        flags |= vk::ShaderStageFlags::FRAGMENT;
    }
    if stage.contains(ShaderStage::COMPUTE) {
        flags |= vk::ShaderStageFlags::COMPUTE;
    }
    if stage.contains(ShaderStage::RAY_GEN) {
        flags |= vk::ShaderStageFlags::RAYGEN_KHR;
    }
    if stage.contains(ShaderStage::ANY_HIT) {
        flags |= vk::ShaderStageFlags::ANY_HIT_KHR;
    }
    if stage.contains(ShaderStage::CLOSEST_HIT) {
        flags |= vk::ShaderStageFlags::CLOSEST_HIT_KHR;
    }
    if stage.contains(ShaderStage::MISS) {
        flags |= vk::ShaderStageFlags::MISS_KHR;
    }
    if stage.contains(ShaderStage::INTERSECTION) {
        flags |= vk::ShaderStageFlags::INTERSECTION_KHR;
    }
    if stage.contains(ShaderStage::ALL_GRAPHICS) {
        flags |= vk::ShaderStageFlags::ALL_GRAPHICS;
    }
    flags
}

/// Convert RHI pipeline stage flags to Vulkan pipeline stage flags.
pub(crate) fn pipeline_stage(stages: crate::types::PipelineStage) -> vk::PipelineStageFlags {
    use crate::types::PipelineStage;
    let mut flags = vk::PipelineStageFlags::empty();
    if stages.contains(PipelineStage::TOP_OF_PIPE) {
        flags |= vk::PipelineStageFlags::TOP_OF_PIPE;
    }
    if stages.contains(PipelineStage::DRAW_INDIRECT) {
        flags |= vk::PipelineStageFlags::DRAW_INDIRECT;
    }
    if stages.contains(PipelineStage::VERTEX_INPUT) {
        flags |= vk::PipelineStageFlags::VERTEX_INPUT;
    }
    if stages.contains(PipelineStage::VERTEX_SHADER) {
        flags |= vk::PipelineStageFlags::VERTEX_SHADER;
    }
    if stages.contains(PipelineStage::FRAGMENT_SHADER) {
        flags |= vk::PipelineStageFlags::FRAGMENT_SHADER;
    }
    if stages.contains(PipelineStage::COLOR_ATTACHMENT_OUTPUT) {
        flags |= vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
    }
    if stages.contains(PipelineStage::COMPUTE_SHADER) {
        flags |= vk::PipelineStageFlags::COMPUTE_SHADER;
    }
    if stages.contains(PipelineStage::TRANSFER) {
        flags |= vk::PipelineStageFlags::TRANSFER;
    }
    if stages.contains(PipelineStage::BOTTOM_OF_PIPE) {
        flags |= vk::PipelineStageFlags::BOTTOM_OF_PIPE;
    }
    if stages.contains(PipelineStage::HOST) {
        flags |= vk::PipelineStageFlags::HOST;
    }
    flags
}

/// Convert RHI access flags to Vulkan access flags.
pub(crate) fn access_flags(access: crate::types::AccessFlags) -> vk::AccessFlags {
    use crate::types::AccessFlags;
    let mut flags = vk::AccessFlags::empty();
    if access.contains(AccessFlags::INDIRECT_COMMAND_READ) {
        flags |= vk::AccessFlags::INDIRECT_COMMAND_READ;
    }
    if access.contains(AccessFlags::INDEX_READ) {
        flags |= vk::AccessFlags::INDEX_READ;
    }
    if access.contains(AccessFlags::VERTEX_ATTRIBUTE_READ) {
        flags |= vk::AccessFlags::VERTEX_ATTRIBUTE_READ;
    }
    if access.contains(AccessFlags::UNIFORM_READ) {
        flags |= vk::AccessFlags::UNIFORM_READ;
    }
    if access.contains(AccessFlags::SHADER_READ) {
        flags |= vk::AccessFlags::SHADER_READ;
    }
    if access.contains(AccessFlags::SHADER_WRITE) {
        flags |= vk::AccessFlags::SHADER_WRITE;
    }
    if access.contains(AccessFlags::COLOR_ATTACHMENT_READ) {
        flags |= vk::AccessFlags::COLOR_ATTACHMENT_READ;
    }
    if access.contains(AccessFlags::COLOR_ATTACHMENT_WRITE) {
        flags |= vk::AccessFlags::COLOR_ATTACHMENT_WRITE;
    }
    if access.contains(AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ) {
        flags |= vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ;
    }
    if access.contains(AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE) {
        flags |= vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE;
    }
    if access.contains(AccessFlags::TRANSFER_READ) {
        flags |= vk::AccessFlags::TRANSFER_READ;
    }
    if access.contains(AccessFlags::TRANSFER_WRITE) {
        flags |= vk::AccessFlags::TRANSFER_WRITE;
    }
    if access.contains(AccessFlags::HOST_READ) {
        flags |= vk::AccessFlags::HOST_READ;
    }
    if access.contains(AccessFlags::HOST_WRITE) {
        flags |= vk::AccessFlags::HOST_WRITE;
    }
    flags
}

/// Convert RHI subpass dependency flags to Vulkan dependency flags.
pub(crate) fn dependency_flags(
    flags: crate::command::pass::render::DependencyFlags,
) -> vk::DependencyFlags {
    use crate::command::pass::render::DependencyFlags;
    let mut out = vk::DependencyFlags::empty();
    if flags.contains(DependencyFlags::BY_REGION) {
        out |= vk::DependencyFlags::BY_REGION;
    }
    if flags.contains(DependencyFlags::DEVICE_GROUP) {
        out |= vk::DependencyFlags::DEVICE_GROUP;
    }
    if flags.contains(DependencyFlags::VIEW_LOCAL) {
        out |= vk::DependencyFlags::VIEW_LOCAL;
    }
    out
}

/// Convert an RHI stencil op state to a Vulkan stencil op state.
pub(crate) fn stencil_op_state(
    state: &crate::pipeline::StencilOpState,
) -> vk::StencilOpState {
    vk::StencilOpState {
        fail_op: stencil_op(state.fail_op),
        pass_op: stencil_op(state.pass_op),
        depth_fail_op: stencil_op(state.depth_fail_op),
        compare_op: compare_op(state.compare_op),
        compare_mask: state.compare_mask,
        write_mask: state.write_mask,
        reference: state.reference,
    }
}