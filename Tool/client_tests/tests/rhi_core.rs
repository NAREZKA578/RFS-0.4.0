use rhi::core::caps::{DeviceCaps, MissingFeature};
use rhi::core::device::{
    DeviceDesc, MemoryProperties, PhysicalDevice, PhysicalDeviceType, Queue, QueueFamily,
};
use rhi::types::{Features, Limits, QueueFlags, SampleCount};

fn sample_features() -> Features {
    Features {
        geometry_shader: true,
        tessellation_shader: false,
        mesh_shader: false,
        shader_float64: true,
        shader_int64: true,
        multi_draw_indirect: true,
        depth_bounds: false,
        depth_clamp: true,
        texture_compression_bc: true,
        texture_compression_astc: false,
        texture_compression_etc2: true,
        sampler_anisotropy: true,
        storage_buffer: true,
        storage_image: true,
        compute: true,
        indirect_compute: false,
        ray_tracing: false,
        ray_query: false,
        variable_rate_shading: false,
        conservative_raster: false,
        sparse_binding: false,
        memory_budget: false,
        descriptor_indexing: true,
        buffer_device_address: true,
    }
}

fn sample_limits() -> Limits {
    Limits {
        max_texture_size: 16384,
        max_texture_layers: 2048,
        max_texture_mips: 15,
        max_buffer_size: 1 << 30,
        max_uniform_buffer_size: 65536,
        max_storage_buffer_size: 1 << 30,
        max_push_constants_size: 128,
        max_bound_descriptor_sets: 8,
        max_per_stage_descriptors: 16,
        max_color_attachments: 8,
        max_sample_count: SampleCount::X4,
        max_viewports: 16,
        max_compute_work_group_count: [65535, 65535, 65535],
        max_compute_work_group_size: [1024, 1024, 64],
    }
}

#[test]
fn caps_supports_identical_features() {
    let caps = DeviceCaps {
        features: sample_features(),
        limits: sample_limits(),
        memory_properties: MemoryProperties::default(),
    };
    let required = sample_features();
    assert!(caps.supports(&required));
}

#[test]
fn caps_missing_features_partial() {
    let caps = DeviceCaps {
        features: sample_features(),
        limits: sample_limits(),
        memory_properties: MemoryProperties::default(),
    };

    let mut required = sample_features();
    required.ray_tracing = true;
    required.variable_rate_shading = true;

    assert!(!caps.supports(&required));

    let missing = caps.missing_features(&required);
    assert_eq!(missing.len(), 2);
    assert!(missing.iter().any(|m| m.name == "ray_tracing"));
    assert!(missing.iter().any(|m| m.name == "variable_rate_shading"));
}

#[test]
fn caps_missing_features_ignores_supported() {
    let caps = DeviceCaps {
        features: sample_features(),
        limits: sample_limits(),
        memory_properties: MemoryProperties::default(),
    };

    let missing = caps.missing_features(&sample_features());
    assert!(missing.is_empty());
}

#[test]
fn caps_all_required_missing() {
    let caps = DeviceCaps::default();
    let missing = caps.missing_features(&sample_features());
    assert!(!missing.is_empty());
    assert!(missing.iter().all(|m| !m.description.is_empty()));
}

#[test]
fn caps_max_sample_count() {
    let mut caps = DeviceCaps {
        features: Features::default(),
        limits: sample_limits(),
        memory_properties: MemoryProperties::default(),
    };

    assert_eq!(caps.max_sample_count(), SampleCount::X4);
    assert!(caps.supports_sample_count(SampleCount::X1));
    assert!(caps.supports_sample_count(SampleCount::X2));
    assert!(caps.supports_sample_count(SampleCount::X4));
    assert!(!caps.supports_sample_count(SampleCount::X8));

    caps.limits.max_sample_count = SampleCount::X1;
    assert_eq!(caps.max_sample_count(), SampleCount::X1);
    assert!(!caps.supports_sample_count(SampleCount::X2));
}

#[test]
fn caps_limits_queries() {
    let caps = DeviceCaps {
        features: Features::default(),
        limits: sample_limits(),
        memory_properties: MemoryProperties::default(),
    };
    assert_eq!(caps.max_texture_size(), 16384);
    assert_eq!(caps.max_buffer_size(), 1 << 30);
}

#[test]
fn physical_device_queue_family_lookup() {
    let gfx_family = QueueFamily {
        index: 0,
        flags: QueueFlags::GRAPHICS,
        queue_count: 1,
        timestamp_valid_bits: 0,
        min_image_transfer_granularity: rhi::types::Extent3D {
            width: 1,
            height: 1,
            depth: 1,
        },
    };
    let compute_family = QueueFamily {
        index: 1,
        flags: QueueFlags::COMPUTE,
        queue_count: 1,
        timestamp_valid_bits: 0,
        min_image_transfer_granularity: rhi::types::Extent3D {
            width: 1,
            height: 1,
            depth: 1,
        },
    };

    let device = PhysicalDevice {
        name: "Test GPU".into(),
        vendor_id: 0x10DE,
        device_id: 0x1234,
        device_type: PhysicalDeviceType::Discrete,
        features: Features::default(),
        limits: Limits::default(),
        memory_properties: MemoryProperties::default(),
        queue_families: vec![gfx_family, compute_family],
    };

    assert!(device.get_queue_family(QueueFlags::GRAPHICS).is_some());
    assert!(device.get_queue_family(QueueFlags::COMPUTE).is_some());
    assert!(device.get_queue_family(QueueFlags::TRANSFER).is_none());

    assert_eq!(device.name, "Test GPU");
    assert_eq!(device.device_type, PhysicalDeviceType::Discrete);
}

#[test]
fn queue_supports_predicates() {
    let queue = Queue {
        family_index: 0,
        index: 0,
        flags: QueueFlags::GRAPHICS | QueueFlags::COMPUTE,
    };
    assert!(queue.supports_graphics());
    assert!(queue.supports_compute());
    assert!(!queue.supports_transfer());
}

#[test]
fn device_desc_default_no_features() {
    let desc = DeviceDesc::default();
    assert!(desc.queue_family_indices.is_empty());
    assert!(!desc.validation);
    assert!(!desc.features.ray_tracing);
}

#[test]
fn caps_struct_is_send_sync_clone_default() {
    let caps = DeviceCaps::default();
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DeviceCaps>();
    let cloned = caps.clone();
    let _missing: Vec<MissingFeature> = vec![];
    assert_eq!(cloned.max_sample_count(), SampleCount::X1);
}