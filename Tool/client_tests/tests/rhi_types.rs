use rhi::types::primitives::*;
use rhi::types::flags::*;
use rhi::types::features::Features;
use rhi::types::limits::Limits;
use rhi::pipeline::graphics::{ColorBlendState, BlendFactor, BlendOp};

#[test]
fn format_is_depth() {
    assert!(Format::D16_UNORM.is_depth());
    assert!(Format::D24_UNORM.is_depth());
    assert!(Format::D32_SFLOAT.is_depth());
    assert!(Format::D24_UNORM_S8_UINT.is_depth());
    assert!(Format::D32_SFLOAT_S8_UINT.is_depth());
    assert!(!Format::RGBA8_UNORM.is_depth());
    assert!(!Format::S8_UINT.is_depth());
    assert!(!Format::R32_SFLOAT.is_depth());
}

#[test]
fn format_is_stencil() {
    assert!(Format::S8_UINT.is_stencil());
    assert!(Format::D24_UNORM_S8_UINT.is_stencil());
    assert!(Format::D32_SFLOAT_S8_UINT.is_stencil());
    assert!(!Format::D16_UNORM.is_stencil());
    assert!(!Format::D24_UNORM.is_stencil());
    assert!(!Format::RGBA8_UNORM.is_stencil());
}

#[test]
fn format_is_compressed() {
    assert!(Format::BC1_RGB_UNORM.is_compressed());
    assert!(Format::BC7_SRGB.is_compressed());
    assert!(Format::ASTC_4x4_UNORM.is_compressed());
    assert!(Format::ASTC_8x8_SRGB.is_compressed());
    assert!(Format::ETC2_R8G8B8_UNORM.is_compressed());
    assert!(Format::EAC_R11_SNORM.is_compressed());
    assert!(!Format::RGBA8_UNORM.is_compressed());
    assert!(!Format::D32_SFLOAT.is_compressed());
}

#[test]
fn format_default() {
    assert_eq!(Format::default(), Format::RGBA8_UNORM);
}

#[test]
fn sample_count_as_count() {
    assert_eq!(SampleCount::X1.as_count(), 1);
    assert_eq!(SampleCount::X2.as_count(), 2);
    assert_eq!(SampleCount::X4.as_count(), 4);
    assert_eq!(SampleCount::X8.as_count(), 8);
    assert_eq!(SampleCount::X16.as_count(), 16);
    assert_eq!(SampleCount::X32.as_count(), 32);
    assert_eq!(SampleCount::X64.as_count(), 64);
}

#[test]
fn sample_count_default() {
    assert_eq!(SampleCount::default(), SampleCount::X1);
}

#[test]
fn clear_value_color() {
    let cv = ClearValue::color(0.1, 0.2, 0.3, 1.0);
    match cv {
        ClearValue::Color { r, g, b, a } => {
            assert!((r - 0.1).abs() < f32::EPSILON);
            assert!((g - 0.2).abs() < f32::EPSILON);
            assert!((b - 0.3).abs() < f32::EPSILON);
            assert!((a - 1.0).abs() < f32::EPSILON);
        }
        _ => panic!("Expected Color variant"),
    }
}

#[test]
fn clear_value_depth_stencil() {
    let cv = ClearValue::DepthStencil { depth: 1.0, stencil: 0 };
    match cv {
        ClearValue::DepthStencil { depth, stencil } => {
            assert!((depth - 1.0).abs() < f32::EPSILON);
            assert_eq!(stencil, 0);
        }
        _ => panic!("Expected DepthStencil variant"),
    }
}

#[test]
fn extent2d_new() {
    let e = Extent2D::new(1920, 1080);
    assert_eq!(e.width, 1920);
    assert_eq!(e.height, 1080);
}

#[test]
fn extent3d_fields() {
    let e = Extent3D { width: 256, height: 256, depth: 64 };
    assert_eq!(e.width, 256);
    assert_eq!(e.height, 256);
    assert_eq!(e.depth, 64);
}

#[test]
fn offset2d_fields() {
    let o = Offset2D { x: 10, y: 20 };
    assert_eq!(o.x, 10);
    assert_eq!(o.y, 20);
}

#[test]
fn viewport_fields() {
    let v = Viewport { x: 0.0, y: 0.0, width: 800.0, height: 600.0, min_depth: 0.0, max_depth: 1.0 };
    assert!((v.width - 800.0).abs() < f32::EPSILON);
    assert!((v.height - 600.0).abs() < f32::EPSILON);
    assert!((v.min_depth).abs() < f32::EPSILON);
    assert!((v.max_depth - 1.0).abs() < f32::EPSILON);
}

#[test]
fn viewport_default() {
    let v = Viewport::default();
    assert!((v.x).abs() < f32::EPSILON);
    assert!((v.y).abs() < f32::EPSILON);
    assert!((v.width).abs() < f32::EPSILON);
    assert!((v.height).abs() < f32::EPSILON);
}

#[test]
fn features_default() {
    let f = Features::default();
    assert!(!f.geometry_shader);
    assert!(!f.tessellation_shader);
    assert!(!f.mesh_shader);
    assert!(!f.ray_tracing);
    assert!(!f.compute);
    assert!(!f.storage_buffer);
    assert!(!f.descriptor_indexing);
    assert!(!f.memory_budget);
}

#[test]
fn features_custom() {
    let f = Features {
        geometry_shader: true,
        compute: true,
        ray_tracing: true,
        ..Default::default()
    };
    assert!(f.geometry_shader);
    assert!(f.compute);
    assert!(f.ray_tracing);
    assert!(!f.tessellation_shader);
}

#[test]
fn limits_default() {
    let l = Limits::default();
    assert_eq!(l.max_texture_size, 0);
    assert_eq!(l.max_buffer_size, 0);
    assert_eq!(l.max_color_attachments, 0);
}

#[test]
fn features_serde_roundtrip() {
    let f = Features {
        geometry_shader: true,
        compute: true,
        ..Default::default()
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: Features = serde_json::from_str(&json).unwrap();
    assert_eq!(f.geometry_shader, f2.geometry_shader);
    assert_eq!(f.compute, f2.compute);
    assert_eq!(f.ray_tracing, f2.ray_tracing);
}

#[test]
fn limits_serde_roundtrip() {
    let l = Limits {
        max_texture_size: 16384,
        max_buffer_size: 1 << 30,
        ..Default::default()
    };
    let json = serde_json::to_string(&l).unwrap();
    let l2: Limits = serde_json::from_str(&json).unwrap();
    assert_eq!(l.max_texture_size, l2.max_texture_size);
    assert_eq!(l.max_buffer_size, l2.max_buffer_size);
}

#[test]
fn queue_flags_ops() {
    let g = QueueFlags::GRAPHICS;
    let c = QueueFlags::COMPUTE;
    let gc = g | c;
    assert!(gc.contains(QueueFlags::GRAPHICS));
    assert!(gc.contains(QueueFlags::COMPUTE));
    assert!(!gc.contains(QueueFlags::TRANSFER));
    assert!(gc.intersects(QueueFlags::GRAPHICS));
    let intersection = gc & QueueFlags::GRAPHICS;
    assert_eq!(intersection, QueueFlags::GRAPHICS);
}

#[test]
fn buffer_usage_ops() {
    let u = BufferUsage::UNIFORM | BufferUsage::STORAGE;
    assert!(u.contains(BufferUsage::UNIFORM));
    assert!(u.contains(BufferUsage::STORAGE));
    assert!(!u.contains(BufferUsage::INDEX));
    let i = u & BufferUsage::UNIFORM;
    assert_eq!(i, BufferUsage::UNIFORM);
}

#[test]
fn shader_stage_ops() {
    let vs = ShaderStage::VERTEX;
    let fs = ShaderStage::FRAGMENT;
    let both = vs | fs;
    assert!(both.contains(ShaderStage::VERTEX));
    assert!(both.contains(ShaderStage::FRAGMENT));
    assert!(!both.contains(ShaderStage::COMPUTE));
    assert_eq!(both, ShaderStage::ALL_GRAPHICS);
}

#[test]
fn pipeline_stage_ops() {
    let s = PipelineStage::VERTEX_SHADER | PipelineStage::FRAGMENT_SHADER;
    assert!(s.contains(PipelineStage::VERTEX_SHADER));
    assert!(s.contains(PipelineStage::FRAGMENT_SHADER));
    assert!(!s.contains(PipelineStage::COMPUTE_SHADER));
}

#[test]
fn access_flags_ops() {
    let a = AccessFlags::SHADER_READ | AccessFlags::SHADER_WRITE;
    assert!(a.contains(AccessFlags::SHADER_READ));
    assert!(a.contains(AccessFlags::SHADER_WRITE));
    assert!(!a.contains(AccessFlags::TRANSFER_READ));
}

#[test]
fn primitive_topology_variants() {
    let _ = rhi::types::primitives::PrimitiveTopology::PointList;
    let _ = rhi::types::primitives::PrimitiveTopology::LineList;
    let _ = rhi::types::primitives::PrimitiveTopology::LineStrip;
    let _ = rhi::types::primitives::PrimitiveTopology::TriangleList;
    let _ = rhi::types::primitives::PrimitiveTopology::TriangleStrip;
    let _ = rhi::types::primitives::PrimitiveTopology::TriangleFan;
}

#[test]
fn graphics_api_variants() {
    let _ = GraphicsApi::Vulkan;
    let _ = GraphicsApi::Direct3D12;
    let _ = GraphicsApi::Direct3D11;
    let _ = GraphicsApi::OpenGL;
}

#[test]
fn color_blend_state_disabled() {
    let s = ColorBlendState::disabled();
    assert!(!s.logic_op_enable);
    assert!(s.attachments.is_empty());
}

#[test]
fn color_blend_state_alpha() {
    let s = ColorBlendState::alpha();
    assert_eq!(s.attachments.len(), 1);
    let a = &s.attachments[0];
    assert!(a.blend_enable);
    assert_eq!(a.src_color_blend_factor, BlendFactor::SrcAlpha);
    assert_eq!(a.dst_color_blend_factor, BlendFactor::OneMinusSrcAlpha);
    assert_eq!(a.color_blend_op, BlendOp::Add);
}

#[test]
fn color_blend_state_additive() {
    let s = ColorBlendState::additive();
    assert_eq!(s.attachments.len(), 1);
    let a = &s.attachments[0];
    assert!(a.blend_enable);
    assert_eq!(a.src_color_blend_factor, BlendFactor::One);
    assert_eq!(a.dst_color_blend_factor, BlendFactor::One);
}

#[test]
fn color_blend_state_multiplicative() {
    let s = ColorBlendState::multiplicative();
    assert_eq!(s.attachments.len(), 1);
    let a = &s.attachments[0];
    assert!(a.blend_enable);
    assert_eq!(a.src_color_blend_factor, BlendFactor::DstColor);
    assert_eq!(a.dst_color_blend_factor, BlendFactor::Zero);
}

#[test]
fn color_blend_state_screen() {
    let s = ColorBlendState::screen();
    assert_eq!(s.attachments.len(), 1);
    let a = &s.attachments[0];
    assert!(a.blend_enable);
    assert_eq!(a.src_color_blend_factor, BlendFactor::One);
    assert_eq!(a.dst_color_blend_factor, BlendFactor::OneMinusSrcColor);
}

#[test]
fn color_component_flags_ops() {
    let all = ColorComponentFlags::R | ColorComponentFlags::G | ColorComponentFlags::B | ColorComponentFlags::A;
    assert!(all.contains(ColorComponentFlags::R));
    assert!(all.contains(ColorComponentFlags::G));
    assert!(all.contains(ColorComponentFlags::B));
    assert!(all.contains(ColorComponentFlags::A));
}

#[test]
fn texture_usage_ops() {
    let u = TextureUsage::SAMPLED | TextureUsage::COLOR_ATTACHMENT;
    assert!(u.contains(TextureUsage::SAMPLED));
    assert!(u.contains(TextureUsage::COLOR_ATTACHMENT));
    assert!(!u.contains(TextureUsage::STORAGE));
}

#[test]
fn index_type_variants() {
    let _ = IndexType::U16;
    let _ = IndexType::U32;
}

#[test]
fn texture_dimensions_variants() {
    let _ = TextureDimensions::D1;
    let _ = TextureDimensions::D2;
    let _ = TextureDimensions::D3;
    let _ = TextureDimensions::Cube;
}

#[test]
fn texture_dimensions_default() {
    assert_eq!(TextureDimensions::default(), TextureDimensions::D1);
}

#[test]
fn sharing_mode_variants() {
    assert_eq!(SharingMode::default(), SharingMode::Exclusive);
    let _ = SharingMode::Concurrent;
}

#[test]
fn memory_property_flags_ops() {
    let f = MemoryPropertyFlags::DEVICE_LOCAL | MemoryPropertyFlags::HOST_VISIBLE;
    assert!(f.contains(MemoryPropertyFlags::DEVICE_LOCAL));
    assert!(f.contains(MemoryPropertyFlags::HOST_VISIBLE));
    assert!(!f.contains(MemoryPropertyFlags::HOST_CACHED));
}

#[test]
fn dynamic_state_ops() {
    let d = DynamicState::VIEWPORT | DynamicState::SCISSOR;
    assert!(d.contains(DynamicState::VIEWPORT));
    assert!(d.contains(DynamicState::SCISSOR));
    assert!(!d.contains(DynamicState::BLEND_CONSTANTS));
}
