// Integration tests for the rhi::backend module.
//
// Covers: Backend trait object safety, per-API backend stubs, and
// create_backend dispatch.

use rhi::backend::{create_backend, Backend};
use rhi::backend::d3d11::D3D11Backend;
use rhi::backend::d3d12::D3D12Backend;
use rhi::backend::opengl::OpenGLBackend;
use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{DeviceDesc, PhysicalDevice, PhysicalDeviceType};
use rhi::types::GraphicsApi;
use rhi::types::{Features, Limits};
use rhi::{MemoryProperties, RhiError};

fn fake_physical_device() -> PhysicalDevice {
    PhysicalDevice {
        name: "Fake GPU".into(),
        vendor_id: 0,
        device_id: 0,
        device_type: PhysicalDeviceType::Cpu,
        features: Features::default(),
        limits: Limits::default(),
        memory_properties: MemoryProperties::default(),
        queue_families: vec![],
    }
}

#[test]
fn backend_creation_all_apis() {
    let cfg = RhiConfig::default();
    let b = create_backend(&cfg).unwrap();
    // Default api enum picks a backend; it must be usable as a trait object.
    let _: Box<dyn Backend> = b;
}

#[test]
fn vulkan_backend_identity() {
    let b = VulkanBackend::new(&RhiConfig::default()).unwrap();
    assert_eq!(b.name(), "Vulkan");
    assert_eq!(b.api(), GraphicsApi::Vulkan);
}

#[test]
fn d3d12_backend_identity() {
    let b = D3D12Backend::new(&RhiConfig::default()).unwrap();
    assert_eq!(b.name(), "Direct3D 12");
    assert_eq!(b.api(), GraphicsApi::Direct3D12);
}

#[test]
fn d3d11_backend_identity() {
    let b = D3D11Backend::new(&RhiConfig::default()).unwrap();
    assert_eq!(b.name(), "Direct3D 11");
    assert_eq!(b.api(), GraphicsApi::Direct3D11);
}

#[test]
fn opengl_backend_identity() {
    let b = OpenGLBackend::new(&RhiConfig::default()).unwrap();
    assert_eq!(b.name(), "OpenGL");
    assert_eq!(b.api(), GraphicsApi::OpenGL);
}

#[test]
fn backend_as_trait_object() {
    let b: Box<dyn Backend> = Box::new(VulkanBackend::new(&RhiConfig::default()).unwrap());
    assert!(!b.name().is_empty());
    let _ = b.api();
}

#[test]
fn stub_backends_return_not_supported() {
    let backends: Vec<Box<dyn Backend>> = vec![
        Box::new(D3D12Backend::new(&RhiConfig::default()).unwrap()),
        Box::new(D3D11Backend::new(&RhiConfig::default()).unwrap()),
        Box::new(OpenGLBackend::new(&RhiConfig::default()).unwrap()),
    ];
    let fake = fake_physical_device();
    let desc = DeviceDesc::default();
    for b in &backends {
        assert!(matches!(
            b.enumerate_physical_devices(),
            Err(RhiError::NotSupported(_))
        ));
        assert!(matches!(
            b.create_device(&fake, &desc),
            Err(RhiError::NotSupported(_))
        ));
    }
}

#[test]
fn vulkan_backend_enumerates_devices() {
    let b = VulkanBackend::new(&RhiConfig::default()).unwrap();
    let devices = b.enumerate_physical_devices().unwrap();
    assert!(!devices.is_empty());
    let first = &devices[0];
    assert!(!first.name.is_empty());
    assert!(!first.queue_families.is_empty());
    assert!(first.limits.max_texture_size > 0);
    assert!(first.limits.max_buffer_size > 0);
    assert!(!first.memory_properties.memory_types.is_empty());
    assert!(!first.memory_properties.memory_heaps.is_empty());
}

#[test]
fn vulkan_backend_creates_logical_device() {
    let b = VulkanBackend::new(&RhiConfig::default()).unwrap();
    let devices = b.enumerate_physical_devices().unwrap();
    let device = b.create_device(&devices[0], &DeviceDesc::default());
    assert!(device.is_ok());
    let device = device.unwrap();
    assert!(!device.queues().is_empty());
    assert!(device.graphics_queue().is_some());
    device.wait_idle().unwrap();
    assert_eq!(device.backend_resource().unwrap().backend_type(), GraphicsApi::Vulkan);
}

#[test]
fn rhi_creation_requires_working_backend() {
    // The Vulkan backend drives Rhi::new on this machine. On systems without
    // a Vulkan loader a proper error is reported instead of panicking.
    let cfg = RhiConfig::default();
    let result = rhi::core::instance::Rhi::new(cfg);
    if result.is_err() {
        assert!(matches!(result, Err(RhiError::NoBackendForApi(_))));
    }
}