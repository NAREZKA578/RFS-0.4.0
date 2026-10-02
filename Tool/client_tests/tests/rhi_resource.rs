use rhi::resource::buffer::*;
use rhi::resource::texture::*;
use rhi::resource::sampler::*;
use rhi::resource::acceleration::blas::*;
use rhi::resource::acceleration::build::*;
use rhi::resource::acceleration::query::*;
use rhi::resource::acceleration::tlas::*;
use rhi::types::primitives::*;
use rhi::types::flags::*;

#[test]
fn buffer_desc_creation() {
    let desc = BufferDesc {
        size: 1024,
        usage: BufferUsage::VERTEX | BufferUsage::INDEX,
        memory_flags: MemoryPropertyFlags::DEVICE_LOCAL,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: vec![0, 1],
    };
    assert_eq!(desc.size, 1024);
    assert!(desc.usage.contains(BufferUsage::VERTEX));
    assert!(desc.usage.contains(BufferUsage::INDEX));
}

#[test]
fn buffer_desc_default() {
    let desc = BufferDesc::default();
    assert_eq!(desc.size, 0);
    assert!(desc.usage.is_empty());
}

#[test]
fn buffer_new_and_desc() {
    let desc = BufferDesc { size: 4096, ..Default::default() };
    let buf = Buffer::new(desc);
    assert_eq!(buf.size(), 4096);
    assert_eq!(buf.desc().size, 4096);
}

#[test]
fn buffer_clone() {
    let desc = BufferDesc { size: 256, ..Default::default() };
    let buf = Buffer::new(desc);
    let buf2 = buf.clone();
    assert_eq!(buf2.size(), 256);
}

#[test]
fn texture_desc_creation() {
    let desc = TextureDesc {
        width: 512,
        height: 512,
        depth: 1,
        mip_levels: 10,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: vec![],
    };
    assert_eq!(desc.width, 512);
    assert_eq!(desc.height, 512);
    assert_eq!(desc.mip_levels, 10);
    assert_eq!(desc.format, Format::RGBA8_UNORM);
}

#[test]
fn texture_desc_default() {
    let desc = TextureDesc::default();
    assert_eq!(desc.width, 0);
    assert_eq!(desc.height, 0);
    assert_eq!(desc.format, Format::default());
}

#[test]
fn texture_new_and_accessors() {
    let desc = TextureDesc {
        width: 256,
        height: 128,
        format: Format::D32_SFLOAT,
        ..Default::default()
    };
    let tex = Texture::new(desc);
    assert_eq!(tex.width(), 256);
    assert_eq!(tex.height(), 128);
    assert_eq!(tex.format(), Format::D32_SFLOAT);
    assert_eq!(tex.desc().width, 256);
}

#[test]
fn texture_create_view() {
    let desc = TextureDesc {
        width: 64,
        height: 64,
        format: Format::RGBA8_UNORM,
        ..Default::default()
    };
    let tex = Texture::new(desc);
    let view_desc = TextureViewDesc {
        texture: tex.clone(),
        format: None,
        view_type: TextureViewType::D2,
        aspects: TextureAspectFlags::COLOR,
        base_mip_level: 0,
        mip_level_count: 1,
        base_array_layer: 0,
        array_layer_count: 1,
    };
    let view = tex.create_view(view_desc);
    assert_eq!(tex.width(), 64);
    let _ = view;
}

#[test]
fn texture_view_desc_fields() {
    let tex = Texture::default();
    let vd = TextureViewDesc {
        texture: tex,
        format: Some(Format::R32_SFLOAT),
        view_type: TextureViewType::Cube,
        aspects: TextureAspectFlags::DEPTH,
        base_mip_level: 2,
        mip_level_count: 5,
        base_array_layer: 1,
        array_layer_count: 6,
    };
    assert_eq!(vd.view_type, TextureViewType::Cube);
    assert_eq!(vd.base_mip_level, 2);
    assert_eq!(vd.mip_level_count, 5);
}

#[test]
fn texture_view_type_variants() {
    let _ = TextureViewType::D1;
    let _ = TextureViewType::D2;
    let _ = TextureViewType::D3;
    let _ = TextureViewType::Cube;
    let _ = TextureViewType::CubeArray;
    let _ = TextureViewType::D1Array;
    let _ = TextureViewType::D2Array;
}

#[test]
fn sampler_desc_creation() {
    let desc = SamplerDesc {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_mode: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mip_lod_bias: 0.0,
        max_anisotropy: 16.0,
        compare_enable: false,
        compare_op: CompareOp::Less,
        min_lod: 0.0,
        max_lod: 10.0,
        border_color: BorderColor::FloatOpaqueBlack,
        unnormalized_coordinates: false,
    };
    assert_eq!(desc.mag_filter, FilterMode::Linear);
    assert_eq!(desc.max_anisotropy, 16.0);
}

#[test]
fn sampler_desc_default() {
    let desc = SamplerDesc::default();
    assert_eq!(desc.mag_filter, FilterMode::Nearest);
    assert_eq!(desc.address_mode_u, AddressMode::Repeat);
    assert_eq!(desc.compare_op, CompareOp::Never);
    assert_eq!(desc.border_color, BorderColor::FloatTransparentBlack);
}

#[test]
fn sampler_new_and_desc() {
    let desc = SamplerDesc { max_anisotropy: 8.0, ..Default::default() };
    let sampler = Sampler::new(desc);
    assert_eq!(sampler.desc().max_anisotropy, 8.0);
}

#[test]
fn sampler_clone() {
    let desc = SamplerDesc::default();
    let s = Sampler::new(desc);
    let s2 = s.clone();
    assert_eq!(s2.desc().mag_filter, FilterMode::Nearest);
}

#[test]
fn blas_desc_creation() {
    let desc = BlasDesc {
        geometries: vec![],
        flags: AccelerationStructureFlags::PREFER_FAST_TRACE,
    };
    assert!(desc.geometries.is_empty());
    assert!(desc.flags.contains(AccelerationStructureFlags::PREFER_FAST_TRACE));
}

#[test]
fn blas_desc_flags() {
    let f = AccelerationStructureFlags::ALLOW_UPDATE | AccelerationStructureFlags::ALLOW_COMPACTION;
    assert!(f.contains(AccelerationStructureFlags::ALLOW_UPDATE));
    assert!(f.contains(AccelerationStructureFlags::ALLOW_COMPACTION));
    assert!(!f.contains(AccelerationStructureFlags::PREFER_FAST_BUILD));
}

#[test]
fn blas_new() {
    let desc = BlasDesc { geometries: vec![], flags: AccelerationStructureFlags::empty() };
    let blas = Blas::new(desc);
    let _ = blas;
}

#[test]
fn geometry_type_variants() {
    let _ = GeometryType::Triangles;
    let _ = GeometryType::AABBs;
    let _ = GeometryType::Instances;
}

#[test]
fn geometry_flags_ops() {
    let f = GeometryFlags::OPAQUE;
    assert!(f.contains(GeometryFlags::OPAQUE));
    assert!(!f.contains(GeometryFlags::NO_DUPLICATE_ANY_HIT_INVOCATION));
}

#[test]
fn tlas_desc_creation() {
    let desc = TlasDesc {
        instance_count: 10,
        flags: AccelerationStructureFlags::PREFER_FAST_BUILD,
        dynamic: true,
    };
    assert_eq!(desc.instance_count, 10);
    assert!(desc.dynamic);
}

#[test]
fn instance_flags_ops() {
    let f = InstanceFlags::OPAQUE | InstanceFlags::TRIANGLE_FACING_CULL_DISABLE;
    assert!(f.contains(InstanceFlags::OPAQUE));
    assert!(f.contains(InstanceFlags::TRIANGLE_FACING_CULL_DISABLE));
    assert!(!f.contains(InstanceFlags::NO_OPAQUE));
}

#[test]
fn tlas_new() {
    let desc = TlasDesc { instance_count: 0, flags: AccelerationStructureFlags::empty(), dynamic: false };
    let tlas = Tlas::new(desc, vec![]);
    let _ = format!("{:?}", tlas);
}

#[test]
fn acceleration_structure_sizes_default() {
    let s = AccelerationStructureSizes::default();
    assert_eq!(s.result_data_size, 0);
    assert_eq!(s.scratch_data_size, 0);
}

#[test]
fn acceleration_structure_type_variants() {
    let _ = AccelerationStructureType::Blas;
    let _ = AccelerationStructureType::Tlas;
}

#[test]
fn build_mode_variants() {
    let _ = BuildMode::Build;
    let _ = BuildMode::Update;
}

#[test]
fn acceleration_structure_query_type_variants() {
    let _ = AccelerationStructureQueryType::CompactionSize;
    let _ = AccelerationStructureQueryType::SerializationSize;
    let _ = AccelerationStructureQueryType::CurrentSize;
}

#[test]
fn buffer_desc_validation() {
    let desc = BufferDesc::default();
    assert!(!desc.is_valid());
    assert!(!desc.is_host_mappable());

    let valid = BufferDesc {
        size: 4096,
        usage: BufferUsage::VERTEX,
        memory_flags: MemoryPropertyFlags::DEVICE_LOCAL,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    };
    assert!(valid.is_valid());

    let concurrent = BufferDesc {
        sharing_mode: SharingMode::Concurrent,
        ..valid.clone()
    };
    assert!(!concurrent.is_valid());

    let concurrent_ok = BufferDesc {
        sharing_mode: SharingMode::Concurrent,
        queue_family_indices: vec![0, 1],
        ..valid.clone()
    };
    assert!(concurrent_ok.is_valid());
}

#[test]
fn buffer_alignment_rules() {
    // Bug №222: this test asserted uniform = 64 and storage = 256, which is the
    // exact inversion of the declared constants (UNIFORM_BUFFER_ALIGNMENT = 256,
    // STORAGE_BUFFER_ALIGNMENT = 16). It was locking in undefined behaviour on
    // the device rather than a deliberate requirement.
    let uniform = BufferDesc {
        usage: BufferUsage::UNIFORM,
        ..Default::default()
    };
    assert_eq!(uniform.alignment(), 256);

    let storage = BufferDesc {
        usage: BufferUsage::STORAGE,
        ..Default::default()
    };
    assert_eq!(storage.alignment(), 16);

    // Bug №222: the old check was `contains(VERTEX | INDEX)`, so each of these
    // on its own used to miss and fall through to the 4-byte default.
    for usage in [BufferUsage::VERTEX, BufferUsage::INDEX] {
        let desc = BufferDesc {
            usage,
            ..Default::default()
        };
        assert!(
            desc.alignment() >= 4,
            "{usage:?} alone must still be handled, got {}",
            desc.alignment()
        );
    }
    let both = BufferDesc {
        usage: BufferUsage::VERTEX | BufferUsage::INDEX,
        ..Default::default()
    };
    assert_eq!(both.alignment(), 4);

    // A buffer that is both uniform and storage must satisfy the stricter one.
    let dual = BufferDesc {
        usage: BufferUsage::UNIFORM | BufferUsage::STORAGE,
        ..Default::default()
    };
    assert_eq!(dual.alignment(), 256);

    // No usage at all means nothing to satisfy.
    let generic = BufferDesc::default();
    assert_eq!(generic.alignment(), 1);
}

#[test]
fn buffer_usage_helpers() {
    let buffer = Buffer::new(BufferDesc {
        size: 2048,
        usage: BufferUsage::VERTEX | BufferUsage::INDEX,
        memory_flags: MemoryPropertyFlags::HOST_VISIBLE,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    });
    assert!(buffer.is_vertex());
    assert!(buffer.is_index());
    assert!(!buffer.is_uniform());
    assert!(!buffer.is_storage());
    assert!(!buffer.is_indirect());
    assert!(!buffer.supports_transfer());
    assert!(buffer.is_host_visible());
    assert!(buffer.device_address().is_none());

    let mut addressed = buffer.clone();
    addressed.set_device_address(0xABCD_0000);
    assert_eq!(addressed.device_address(), Some(0xABCD_0000));
}

#[test]
fn texture_desc_validation_and_mips() {
    assert!(!TextureDesc::default().is_valid());

    let valid = TextureDesc {
        width: 512,
        height: 512,
        depth: 1,
        mip_levels: 1,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::SAMPLED,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        ..Default::default()
    };
    assert!(valid.is_valid());

    assert_eq!(TextureDesc::max_mip_levels(512, 512), 10);
    assert_eq!(TextureDesc::max_mip_levels(2048, 1024), 12);
    assert_eq!(TextureDesc::max_mip_levels(1, 1), 1);
    assert_eq!(TextureDesc::max_mip_levels(0, 0), 1);
}

#[test]
fn texture_mip_size_query() {
    let tex = Texture::new(TextureDesc {
        width: 1024,
        height: 512,
        depth: 8,
        mip_levels: 11,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::SAMPLED | TextureUsage::STORAGE,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D3,
        sharing_mode: SharingMode::Exclusive,
        ..Default::default()
    });
    assert_eq!(tex.mip_size(0), (1024, 512, 8));
    assert_eq!(tex.mip_size(3), (128, 64, 1));
    assert_eq!(tex.mip_size(99), (1, 1, 1));
    assert!(tex.is_sampled());
    assert!(tex.is_storage());
    assert!(!tex.is_multisampled());
}

#[test]
fn texture_multisampled_flag() {
    let tex = Texture::new(TextureDesc {
        width: 64,
        height: 64,
        depth: 1,
        mip_levels: 1,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::COLOR_ATTACHMENT,
        sample_count: SampleCount::X4,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    });
    assert!(tex.is_multisampled());
    assert!(tex.is_color_attachment());
    assert!(!tex.is_depth_stencil());
}

#[test]
fn texture_default_view_2d_and_cube() {
    let tex = Texture::new(TextureDesc {
        width: 256,
        height: 256,
        depth: 1,
        mip_levels: 9,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::SAMPLED,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    });
    let view = tex.create_default_view();
    assert_eq!(view.view_type(), TextureViewType::D2);
    assert_eq!(view.format(), Format::RGBA8_UNORM);
    assert_eq!(view.base_mip_level(), 0);
    assert_eq!(view.mip_level_count(), 9);
    assert_eq!(view.array_layers(), 1);

    let cube = Texture::new(TextureDesc {
        width: 64,
        height: 64,
        depth: 1,
        mip_levels: 1,
        array_layers: 6,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::SAMPLED,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::Cube,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    });
    assert_eq!(cube.create_default_view().view_type(), TextureViewType::Cube);

    let cube_array = Texture::new(TextureDesc {
        array_layers: 12,
        dimensions: TextureDimensions::Cube,
        ..cube.desc().clone()
    });
    assert_eq!(
        cube_array.create_default_view().view_type(),
        TextureViewType::CubeArray
    );

    let depth = Texture::new(TextureDesc {
        format: Format::D32_SFLOAT,
        usage: TextureUsage::DEPTH_STENCIL_ATTACHMENT,
        ..tex.desc().clone()
    });
    let depth_view = depth.create_default_view();
    assert!(depth_view.desc().aspects.contains(TextureAspectFlags::DEPTH));
}

#[test]
fn sampler_presets() {
    let linear = SamplerDesc::linear_clamp();
    assert_eq!(linear.mag_filter, FilterMode::Linear);
    assert_eq!(linear.address_mode_u, AddressMode::ClampToEdge);

    let repeat = SamplerDesc::linear_repeat();
    assert_eq!(repeat.address_mode_u, AddressMode::Repeat);

    let nearest = SamplerDesc::nearest_clamp();
    assert_eq!(nearest.mag_filter, FilterMode::Nearest);

    let aniso = SamplerDesc::anisotropic(8.0);
    assert_eq!(aniso.max_anisotropy, 8.0);

    let shadow = SamplerDesc::shadow_compare(CompareOp::Less);
    assert!(shadow.compare_enable);
    assert_eq!(shadow.compare_op, CompareOp::Less);
}

#[test]
fn sampler_helpers() {
    let s = Sampler::new(SamplerDesc::linear_clamp());
    assert!(s.is_linear());
    assert!(!s.is_comparison());
    assert!(!s.is_anisotropic());

    let shadow = Sampler::new(SamplerDesc::shadow_compare(CompareOp::Less));
    assert!(shadow.is_comparison());

    let aniso = Sampler::new(SamplerDesc::anisotropic(4.0));
    assert!(aniso.is_anisotropic());
}

#[test]
fn sampler_desc_validation_rules() {
    assert!(SamplerDesc::default().is_valid());

    let bad_lod = SamplerDesc {
        min_lod: 2.0,
        max_lod: 1.0,
        ..SamplerDesc::default()
    };
    assert!(!bad_lod.is_valid());

    let bad_unnorm = SamplerDesc {
        unnormalized_coordinates: true,
        address_mode_u: AddressMode::ClampToBorder,
        ..SamplerDesc::default()
    };
    assert!(!bad_unnorm.is_valid());
    let unnorm_edge = SamplerDesc {
        address_mode_u: AddressMode::ClampToEdge,
        ..bad_unnorm
    };
    assert!(unnorm_edge.is_valid());
}
