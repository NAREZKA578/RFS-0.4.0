// Integration tests for the Vulkan resource layer.
//
// These tests require a real Vulkan driver and create actual GPU buffers,
// textures, image views and samplers through `VulkanBackend`.

use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::resource::{Sampler, SamplerDesc, TextureViewDesc, TextureViewType};
use rhi::types::{Features, Format, TextureUsage};
use rhi::{AddressMode, Backend, BufferUsage, CompareOp, FilterMode, TextureAspectFlags};

fn open_vulkan_device(features: Features) -> Device {
    let backend = VulkanBackend::new(&RhiConfig::default()).unwrap();
    let devices = backend.enumerate_physical_devices().unwrap();
    let desc = DeviceDesc {
        features,
        ..Default::default()
    };
    backend.create_device(&devices[0], &desc).unwrap()
}

#[test]
fn vulkan_buffer_host_upload_roundtrip() {
    let device = open_vulkan_device(Features::default());
    let mut buffer = device.create_buffer(
        64,
        BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
        true,
    );
    assert!(buffer.has_gpu_backing());
    assert!(buffer.is_storage());
    assert!(buffer.is_host_visible());

    device.upload_buffer(&buffer, &[1u32, 2, 3, 4, 5, 6, 7, 8]);

    let data = device.buffer_data(&buffer).unwrap();
    assert_eq!(data.len(), 64);
    let words = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u32, 8) };
    assert_eq!(words, &[1, 2, 3, 4, 5, 6, 7, 8]);

    device.upload_buffer(&buffer, &[9u32, 9, 9, 9]);
    let data = device.buffer_data(&buffer).unwrap();
    let words = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u32, 4) };
    assert_eq!(words, &[9, 9, 9, 9]);

    buffer.set_device_address(0xABCD_DEAD);
    assert_eq!(buffer.device_address(), Some(0xABCD_DEAD));
}

#[test]
fn vulkan_device_local_buffer() {
    let device = open_vulkan_device(Features::default());
    let buffer = device.create_buffer(1 << 20, BufferUsage::VERTEX, false);
    assert!(buffer.has_gpu_backing());
    assert!(buffer.is_vertex());
    assert!(!buffer.is_host_visible());
    assert_eq!(buffer.size(), 1 << 20);
}

#[test]
fn vulkan_buffer_device_address() {
    let features = Features {
        buffer_device_address: true,
        ..Default::default()
    };
    let device = open_vulkan_device(features);
    let buffer = device.create_buffer(
        256,
        BufferUsage::STORAGE | BufferUsage::SHADER_DEVICE_ADDRESS,
        false,
    );
    assert!(buffer.has_gpu_backing());
    assert!(buffer.device_address().is_some());
}

#[test]
fn vulkan_texture_image_creation() {
    let device = open_vulkan_device(Features::default());
    let texture = device.create_texture(
        256,
        256,
        1,
        Format::RGBA8_UNORM,
        TextureUsage::SAMPLED,
        1,
    );
    assert!(texture.has_gpu_backing());
    assert!(texture.is_sampled());
    assert_eq!(texture.width(), 256);
    assert_eq!(texture.height(), 256);
    assert_eq!(texture.format(), Format::RGBA8_UNORM);
}

#[test]
fn vulkan_texture_view_and_sampler() {
    let features = Features {
        sampler_anisotropy: true,
        ..Default::default()
    };
    let device = open_vulkan_device(features);

    let texture = device.create_texture(
        128,
        128,
        1,
        Format::RGBA8_UNORM,
        TextureUsage::SAMPLED | TextureUsage::COLOR_ATTACHMENT,
        1,
    );
    assert!(texture.has_gpu_backing());

    let view_desc = TextureViewDesc {
        texture: texture.clone(),
        format: None,
        view_type: TextureViewType::D2,
        aspects: TextureAspectFlags::COLOR,
        base_mip_level: 0,
        mip_level_count: 1,
        base_array_layer: 0,
        array_layer_count: 1,
    };
    let view = device.create_texture_view(&texture, &view_desc).unwrap();
    assert!(view.has_gpu_backing());
    assert_eq!(view.view_type(), TextureViewType::D2);

    let sampler = device.create_sampler(AddressMode::ClampToEdge, FilterMode::Linear, 8.0);
    assert!(sampler.has_gpu_backing());
    assert!(sampler.is_anisotropic());
    assert!(sampler.is_linear());
}

#[test]
fn vulkan_sampler_desc_helpers() {
    let device = open_vulkan_device(Features::default());
    let _ = device;
    let shadow = SamplerDesc::shadow_compare(CompareOp::Less);
    assert!(shadow.is_valid());
    assert!(shadow.compare_enable);
    assert!(SamplerDesc::linear_clamp().is_valid());
    let anisotropic = Sampler::new(SamplerDesc::anisotropic(4.0));
    assert!(anisotropic.is_anisotropic());
}

#[test]
fn vulkan_buffer_upload_through_backend_trait() {
    let device = open_vulkan_device(Features::default());
    let buffer = device.create_buffer(16, BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST, true);
    let backend = device.backend_resource().unwrap();
    backend
        .upload_buffer(&buffer, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66])
        .unwrap();
    let data = device.buffer_data(&buffer).unwrap();
    assert_eq!(&data[..6], &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
}

#[test]
fn vulkan_buffer_upload_to_device_local_buffer_via_staging() {
    let device = open_vulkan_device(Features::default());
    let buffer =
        device.create_buffer(32, BufferUsage::STORAGE | BufferUsage::TRANSFER_DST, false);
    assert!(buffer.has_gpu_backing());
    assert!(!buffer.is_host_visible());
    assert!(buffer.supports_transfer());
    device.upload_buffer(&buffer, &[1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn vulkan_texture_upload_with_mip_chain() {
    let device = open_vulkan_device(Features::default());
    let texture = device.create_texture(64, 64, 1, Format::RGBA8_UNORM, TextureUsage::SAMPLED, 4);
    assert!(texture.has_gpu_backing());

    let total = (64 * 64 + 32 * 32 + 16 * 16 + 8 * 8) * 4;
    assert_eq!(texture.mip_size(0), (64, 64, 1));
    assert_eq!(texture.mip_size(1), (32, 32, 1));
    assert_eq!(texture.mip_size(3), (8, 8, 1));
    let data: Vec<u8> = (0..total).map(|i| (i & 0xff) as u8).collect();

    device.upload_texture(&texture, &data);
    let view_desc = TextureViewDesc {
        texture: texture.clone(),
        format: None,
        view_type: TextureViewType::D2,
        aspects: TextureAspectFlags::COLOR,
        base_mip_level: 0,
        mip_level_count: texture.mip_levels(),
        base_array_layer: 0,
        array_layer_count: 1,
    };
    let view = device.create_texture_view(&texture, &view_desc).unwrap();
    assert!(view.has_gpu_backing());
}