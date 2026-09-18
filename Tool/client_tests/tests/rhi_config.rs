use rhi::config::settings::*;
use rhi::types::primitives::GraphicsApi;

#[test]
fn rhi_config_default() {
    let c = RhiConfig::default();
    assert_eq!(c.api, GraphicsApi::Vulkan);
    assert!(!c.validation);
    assert!(!c.debug_markers);
    assert!(!c.enable_ray_tracing);
    assert!(!c.enable_mesh_shading);
    assert!(!c.enable_vrs);
    assert_eq!(c.max_frames_in_flight, 2);
    assert!(c.preferred_gpu.is_none());
}

#[test]
fn memory_allocator_config_default() {
    let m = MemoryAllocatorConfig::default();
    assert!(m.use_buddy_for_buffers);
    assert!(m.use_linear_for_transient);
    assert!(m.use_pool_for_small);
    assert_eq!(m.small_resource_threshold, 1024 * 1024);
    assert_eq!(m.min_alignment, 16);
}

#[test]
fn validation_config_default() {
    let v = ValidationConfig::default();
    assert!(!v.gpu_assisted);
    assert!(v.shader_validation);
    assert!(v.sync_validation);
    assert_eq!(v.min_severity, ValidationSeverity::Error);
}

#[test]
fn validation_severity_variants() {
    let _ = ValidationSeverity::Info;
    let _ = ValidationSeverity::Warning;
    let _ = ValidationSeverity::Error;
    let _ = ValidationSeverity::Verbose;
    assert_eq!(ValidationSeverity::default(), ValidationSeverity::Error);
}

#[test]
fn rhi_config_serde_roundtrip() {
    let c = RhiConfig {
        api: GraphicsApi::Direct3D12,
        validation: true,
        debug_markers: true,
        enable_ray_tracing: true,
        enable_mesh_shading: false,
        enable_vrs: false,
        max_frames_in_flight: 3,
        preferred_gpu: Some("RTX 4090".to_string()),
        memory_allocator: MemoryAllocatorConfig::default(),
    };
    let json = serde_json::to_string(&c).unwrap();
    let c2: RhiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(c.api, c2.api);
    assert_eq!(c.validation, c2.validation);
    assert_eq!(c.debug_markers, c2.debug_markers);
    assert_eq!(c.enable_ray_tracing, c2.enable_ray_tracing);
    assert_eq!(c.max_frames_in_flight, c2.max_frames_in_flight);
    assert_eq!(c.preferred_gpu, c2.preferred_gpu);
}

#[test]
fn validation_config_serde_roundtrip() {
    let v = ValidationConfig {
        gpu_assisted: true,
        shader_validation: false,
        sync_validation: true,
        min_severity: ValidationSeverity::Warning,
    };
    let json = serde_json::to_string(&v).unwrap();
    let v2: ValidationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(v.gpu_assisted, v2.gpu_assisted);
    assert_eq!(v.shader_validation, v2.shader_validation);
    assert_eq!(v.sync_validation, v2.sync_validation);
    assert_eq!(v.min_severity, v2.min_severity);
}

#[test]
fn rhi_config_clone() {
    let c = RhiConfig::default();
    let c2 = c.clone();
    assert_eq!(c.api, c2.api);
    assert_eq!(c.max_frames_in_flight, c2.max_frames_in_flight);
}
