// Integration tests for the rhi::backend module.
//
// Covers: Backend trait object safety, per-API backend stubs, and
// create_backend dispatch.

use rhi::backend::{create_backend, Backend};
use rhi::backend::common::ReleaseKind;
use rhi::backend::d3d11::D3D11Backend;
use rhi::backend::d3d12::D3D12Backend;
use rhi::backend::opengl::OpenGLBackend;
use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{DeviceDesc, PhysicalDevice, PhysicalDeviceType};
use rhi::resource::{BufferDesc, TextureDesc};
use rhi::types::GraphicsApi;
use rhi::types::{BufferUsage, Format, MemoryPropertyFlags, SampleCount, SharingMode};
use rhi::types::{Features, Limits, TextureDimensions, TextureUsage};
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
fn d3d12_backend_enumerates_devices() {
    let b = D3D12Backend::new(&RhiConfig::default()).unwrap();
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
fn d3d12_backend_creates_logical_device() {
    let b = D3D12Backend::new(&RhiConfig::default()).unwrap();
    let devices = b.enumerate_physical_devices().unwrap();
    let device = b.create_device(&devices[0], &DeviceDesc::default());
    assert!(device.is_ok());
    let device = device.unwrap();
    assert!(!device.queues().is_empty());
    assert!(device.graphics_queue().is_some());
    device.wait_idle().unwrap();
    assert_eq!(device.backend_resource().unwrap().backend_type(), GraphicsApi::Direct3D12);
}

fn d3d12_test_device() -> rhi::core::device::Device {
    let b = D3D12Backend::new(&RhiConfig::default()).unwrap();
    let devices = b.enumerate_physical_devices().unwrap();
    b.create_device(&devices[0], &DeviceDesc::default()).unwrap()
}

#[test]
fn d3d12_device_buffer_round_trip() {
    // A device-only buffer in the DEFAULT heap: the upload goes through the
    // UPLOAD-heap staging buffer, a COPY_DEST barrier and CopyBufferRegion;
    // the download goes back through a READBACK staging buffer. The bytes
    // must survive the trip unchanged (BUGS №729).
    let device = d3d12_test_device();
    let desc = BufferDesc {
        size: 1024,
        usage: BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
        memory_flags: MemoryPropertyFlags::DEVICE_LOCAL,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    };
    let buffer = device.create_buffer_from_desc(&desc).unwrap();
    assert!(buffer.has_gpu_backing());
    let data: Vec<u32> = (0..256).map(|i| 1000 + i as u32).collect();
    device.upload_buffer::<u32>(&buffer, &data).unwrap();
    let out = device.download_buffer(&buffer, 0, 1024).unwrap();
    let back: Vec<u32> = out
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if back != data {
        let first = back.iter().zip(data.iter()).position(|(a, b)| a != b);
        eprintln!(
            "round trip mismatch: first at u32 index {:?}, saw {:?}, expected {:?}",
            first,
            back.iter().take(8).collect::<Vec<_>>(),
            data.iter().take(8).collect::<Vec<_>>()
        );
    }
    assert_eq!(back, data);
    device.wait_idle().unwrap();
}

#[test]
fn d3d12_device_mapped_buffer_view() {
    // A host-visible buffer lives in the UPLOAD heap and stays persistently
    // mapped: after the upload `buffer_data` must expose exactly the payload
    // without any backend call.
    let device = d3d12_test_device();
    let buffer = device.create_buffer(256, BufferUsage::UNIFORM, true).unwrap();
    assert!(buffer.has_gpu_backing());
    assert!(buffer.is_host_visible());
    let payload: Vec<u8> = (0..=255u8).collect();
    device.upload_buffer::<u8>(&buffer, &payload).unwrap();
    let view = device.buffer_data(&buffer).expect("the upload buffer maps");
    assert_eq!(view.len(), 256);
    assert_eq!(view, payload.as_slice());
}

#[test]
fn d3d12_device_texture_round_trip() {
    // A 2D texture in the DEFAULT heap: upload with the 256-byte-pitch
    // staging footprint, download the same mip back; the bytes must
    // survive the trip unchanged (BUGS №729).
    let device = d3d12_test_device();
    let desc = TextureDesc {
        width: 64,
        height: 64,
        depth: 1,
        mip_levels: 1,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::TRANSFER_SRC | TextureUsage::TRANSFER_DST,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    };
    let texture = device.create_texture_from_desc(&desc).unwrap();
    assert!(texture.has_gpu_backing());
    let data: Vec<u8> = (0..64 * 64 * 4).map(|i| (i * 31 % 251) as u8).collect();
    device.upload_texture(&texture, &data).unwrap();
    let back = device.download_texture(&texture, 0, 0).unwrap();
    assert_eq!(back.len(), data.len());
    assert_eq!(back, data);
}

// Bug №731: the D3D12 backend inherits the no-op defaults of `release`,
// `release_many` and `release_texture`, so an explicitly released texture
// sat in the handle table until device teardown — the D3D12 edition of the
// leak bug №289 fixed for Vulkan. The public contract: a release must
// remove the tracked entry, a repeated release must stay a no-op, and
// handles the backend never tracked must be ignored rather than panic.
#[test]
fn d3d12_released_texture_leaves_the_teardown_list() {
    let device = d3d12_test_device();
    let before = device.tracked_object_count();

    let desc = TextureDesc {
        width: 8,
        height: 8,
        depth: 1,
        mip_levels: 1,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::TRANSFER_SRC | TextureUsage::TRANSFER_DST,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    };
    let texture = device.create_texture_from_desc(&desc).unwrap();
    assert!(texture.has_gpu_backing());

    assert_eq!(
        device.tracked_object_count(),
        before + 1,
        "the created texture must be tracked"
    );

    device.release_texture(&texture);
    assert_eq!(
        device.tracked_object_count(),
        before,
        "the released texture must leave the handle table"
    );

    // A double release, a handle the backend never tracked (zero is the
    // invalid sentinel) and a kind the backend does not own are all plain
    // no-ops — most of all they must not panic.
    device.release_texture(&texture);
    assert_eq!(
        device.tracked_object_count(),
        before,
        "a repeated release must stay harmless"
    );
    let backend = device.backend_resource().unwrap();
    backend.release(ReleaseKind::Buffer, 0);
    backend.release(ReleaseKind::Sampler, 7);
    assert_eq!(device.tracked_object_count(), before);
}

// Bug №737: the upload path wrote only subresource 0 — mip 0 of layer 0 —
// with hand-computed geometry, so every texture beyond a single mip kept
// the whole mip chain as uninitialized committed memory; download of
// mip 1+ returned whatever the allocation happened to hold. The contract
// mirrors the Vulkan backend: the upload fills the mip chain of layer 0
// from tightly packed, 4-byte-aligned rows, each mip at its own offset,
// and wider layer counts stay an honest rejection.
#[test]
fn d3d12_device_texture_mip_chain_round_trip() {
    let device = d3d12_test_device();
    let desc = TextureDesc {
        width: 64,
        height: 64,
        depth: 1,
        mip_levels: 3,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::TRANSFER_SRC | TextureUsage::TRANSFER_DST,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    };
    let texture = device.create_texture_from_desc(&desc).unwrap();
    assert!(texture.has_gpu_backing());

    // Every mip carries its own pattern, so a stale subresource cannot pass
    // for the expected content by accident.
    let region = |mip: u32, base: u8, seed: u64| -> Vec<u8> {
        let side = 64u32 >> mip;
        (0..side * side * 4)
            .map(|i| ((i as u64 * seed + base as u64 * 9617) % 251) as u8)
            .collect()
    };
    let mip0 = region(0, 1, 31);
    let mip1 = region(1, 2, 127);
    let mip2 = region(2, 3, 89);
    assert_eq!(mip0.len(), 64 * 64 * 4);
    assert_eq!(mip1.len(), 32 * 32 * 4);
    assert_eq!(mip2.len(), 16 * 16 * 4);
    let mut data = mip0.clone();
    data.extend_from_slice(&mip1);
    data.extend_from_slice(&mip2);

    device.upload_texture(&texture, &data).unwrap();
    assert_eq!(device.download_texture(&texture, 0, 0).unwrap(), mip0);
    assert_eq!(
        device.download_texture(&texture, 1, 0).unwrap(),
        mip1,
        "mip 1 must carry its own uploaded bytes, not uninitialized memory"
    );
    assert_eq!(device.download_texture(&texture, 2, 0).unwrap(), mip2);

    // The layer contract is the same honest rejection the Vulkan backend
    // makes today: uploads cover one layer, wider arrays are refused rather
    // than silently writing layer 0 only.
    let layered_desc = TextureDesc {
        array_layers: 2,
        ..desc.clone()
    };
    let layered = device.create_texture_from_desc(&layered_desc).unwrap();
    let result = device.upload_texture(&layered, &mip0);
    assert!(
        matches!(result, Err(RhiError::NotSupported(_))),
        "a two-layer upload must be a stated refusal, saw {result:?}"
    );
}

#[test]
fn d3d11_backend_identity() {
    let b = D3D11Backend::new(&RhiConfig::default()).unwrap();
    assert_eq!(b.name(), "Direct3D 11");
    assert_eq!(b.api(), GraphicsApi::Direct3D11);
}

#[test]
fn d3d11_backend_enumerates_devices() {
    let b = D3D11Backend::new(&RhiConfig::default()).unwrap();
    let devices = b.enumerate_physical_devices().unwrap();
    assert!(!devices.is_empty());
    let first = &devices[0];
    assert!(!first.name.is_empty());
    assert!(!first.queue_families.is_empty());
    assert!(first.limits.max_texture_size > 0);
    assert!(first.limits.max_buffer_size > 0);
    assert!(!first.memory_properties.memory_types.is_empty());
    assert!(!first.memory_properties.memory_heaps.is_empty());
    let device = b.create_device(first, &DeviceDesc::default()).unwrap();
    assert!(!device.queues().is_empty());
    assert!(device.graphics_queue().is_some());
    assert!(
        device
            .backend_resource()
            .unwrap()
            .backend_type() == GraphicsApi::Direct3D11
    );
    device.wait_idle().unwrap();
    b.wait_idle(&device).unwrap();
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
    // D3D12 ships its real backend since its M1 (see .DOC/BUGS.md #705);
    // D3D11 ships its real backend with its M1 (see .DOC/BUGS.md #710),
    // covered by d3d11_backend_enumerates_devices below. OpenGL stays a stub.
    let backends: Vec<Box<dyn Backend>> = vec![Box::new(OpenGLBackend::new(
        &RhiConfig::default(),
    )
    .unwrap())];
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
    match result {
        Err(e) => {
            // Bug №187 — the err side was always checked: a proper error is
            // reported instead of panicking.
            assert!(
                matches!(e, RhiError::NoBackendForApi(_)),
                "no-backend failure must be NoBackendForApi, got {e:?}"
            );
        }
        Ok(rhi) => {
            // Bug №187 — the Ok branch used to be empty: on a machine with a
            // working Vulkan loader the test could not fail at all, so it
            // proved nothing there. A successful creation must actually see
            // hardware.
            assert!(
                !rhi.physical_devices().is_empty(),
                "Rhi::new succeeded but physical_devices() is empty"
            );
            assert!(
                rhi.default_physical_device().is_some(),
                "Rhi::new succeeded but default_physical_device() is none"
            );
        }
    }
}