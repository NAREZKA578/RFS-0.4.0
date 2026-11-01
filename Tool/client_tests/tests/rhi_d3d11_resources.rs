// Integration tests for the D3D11 resource layer.
//
// These tests require a real D3D11 driver and create actual GPU buffers,
// textures, views and samplers through `D3D11Backend`.
//
// D3D11 has no message-based validator the RHI can count (the Vulkan
// `validation_error_count` accessor has no DX equivalent here), so the proof in
// these tests is bytes and counts, not the absence of warnings: every claim is
// backed by a runtime readback against data the test wrote itself, or by a
// tracked-object count taken while the device is alive. The backend's debug
// layer is not what makes these pass.

use rhi::backend::d3d11::compile::compile_hlsl;
use rhi::backend::d3d11::D3D11Backend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::resource::texture::TextureDesc;
use rhi::resource::Texture;
use rhi::shader::module::{ShaderFormat, ShaderModuleDesc};
use rhi::types::{Format, SampleCount, SharingMode, TextureDimensions, TextureUsage};
use rhi::{
    AddressMode, Backend, BufferUsage, FilterMode, TextureAspectFlags, TextureViewDesc,
    TextureViewType,
};

fn open_d3d11_device() -> Device {
    let backend = D3D11Backend::new(&RhiConfig::default()).unwrap();
    let devices = backend.enumerate_physical_devices().unwrap();
    backend.create_device(&devices[0], &DeviceDesc::default()).unwrap()
}

#[test]
fn d3d11_buffer_host_upload_roundtrip() {
    let device = open_d3d11_device();
    // D3D11 storage buffers are device-local by contract (a UAV cannot carry
    // DYNAMIC), so the mapped CPU path goes through a constant buffer; the
    // storage shape is refused at creation on purpose, that is proven by
    // `d3d11_device_only_buffer_contents_survive_a_download` and the refusal
    // test below.
    let buffer = device
        .create_buffer(
            64,
            BufferUsage::UNIFORM | BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
            true,
        )
        .expect("the host-visible uniform buffer must be created");
    assert!(buffer.has_gpu_backing());
    assert!(buffer.is_host_visible());

    device
        .upload_buffer(&buffer, &[1u32, 2, 3, 4, 5, 6, 7, 8])
        .expect("the first upload must succeed; without this a failed write is \
                 indistinguishable from a successful one, and the readback below \
                 would be asserting against memory nobody wrote");

    let words = device.download_buffer(&buffer, 0, 64).expect("readback");
    let mut seen = [0u32; 8];
    for (slot, word) in words.chunks_exact(4).take(8).enumerate() {
        seen[slot] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    assert_eq!(seen, [1, 2, 3, 4, 5, 6, 7, 8]);

    device
        .upload_buffer(&buffer, &[9u32, 9, 9, 9])
        .expect("the second upload must succeed");

    // The upload maps with WRITE_DISCARD, which by contract invalidates the
    // whole allocation: the upload replaces the buffer's contents rather than
    // patching them, so the assertion covers the window that was written and
    // nothing beyond it.
    let words = device.download_buffer(&buffer, 0, 16).expect("readback");
    let mut seen = [0u32; 4];
    for (slot, word) in words.chunks_exact(4).enumerate() {
        seen[slot] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    assert_eq!(seen, [9, 9, 9, 9]);
}

#[test]
fn d3d11_the_upload_refuses_a_device_only_buffer_instead_of_writing_nothing() {
    let device = open_d3d11_device();
    let buffer = device
        .create_buffer(64, BufferUsage::VERTEX, false)
        .expect("the device-only vertex buffer must be created");
    assert!(buffer.has_gpu_backing());
    assert!(!buffer.is_host_visible());

    let error = device
        .upload_buffer(&buffer, &[0u8; 16])
        .expect_err("a device-only buffer has no upload path; the refusal has \
                     to reach the caller rather than disappear into a \
                     success-shaped no-op");
    let text = format!("{error}");
    assert!(
        text.contains("no upload path"),
        "the refusal should say why the upload is impossible, got: {text}"
    );
}

#[test]
fn d3d11_device_only_buffer_contents_survive_a_download() {
    let device = open_d3d11_device();
    // A vertex buffer that a vertex stage pulled from cannot be fed by
    // `upload_buffer` (no upload path), but its backing store is still
    // readable through the staging download; zero fill is the content until
    // something draws into it.
    let buffer = device
        .create_buffer(256, BufferUsage::VERTEX, false)
        .expect("the download-test vertex buffer must be created");
    let bytes = device.download_buffer(&buffer, 0, 256).expect("readback");
    assert_eq!(bytes.len(), 256);
    assert!(bytes.iter().all(|byte| *byte == 0), "a fresh buffer is zeroed");
}

#[test]
fn d3d11_texture_creation_roundtrip() {
    let device = open_d3d11_device();
    let texture = device
        .create_texture(
            64,
            64,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the tracked texture must be created");
    assert!(texture.has_gpu_backing());
    assert_eq!(texture.width(), 64);
    assert_eq!(texture.height(), 64);
    assert_eq!(texture.format(), Format::RGBA8_UNORM);

    // Bug №711's audit fix: the view roles live on the texture entry, and the
    // view shares the texture's handle. A tracked "view" of its own would be
    // unreachable bookkeeping: every attempt to read RTV/DSV through a view
    // handle landed on the texture kind and reported a lost kind instead —
    // invisible to compile and unit tests, fatal on the first draw.
    let before = device.tracked_object_count();
    let texture2 = device
        .create_texture(
            32,
            32,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT,
            1,
        )
        .expect("the released texture must be created");
    assert_eq!(
        device.tracked_object_count(),
        before + 1,
        "a texture with its roles must be tracked once, not twice"
    );

    let view_desc = TextureViewDesc {
        texture: texture2.clone(),
        format: None,
        view_type: TextureViewType::D2,
        aspects: TextureAspectFlags::COLOR,
        base_mip_level: 0,
        mip_level_count: 1,
        base_array_layer: 0,
        array_layer_count: 1,
    };
    let view = device.create_texture_view(&texture2, &view_desc).unwrap();
    assert!(view.has_gpu_backing());

    // Sharing the texture's handle also means the view adds no tracked entry
    // of its own: registering one under a second handle is exactly the defect
    // shape the audit fixed.
    assert_eq!(
        device.tracked_object_count(),
        before + 1,
        "the view must not add a tracked entry - a view and its texture share \
         one handle in D3D11"
    );

    // Releasing the view must not touch the texture: in D3D11 the view owns no
    // object of its own, so freeing it cannot shorten the texture's life.
    device.release_texture_view(&view);
    assert_eq!(
        device.tracked_object_count(),
        before + 1,
        "release_texture_view must be a no-op: removing the shared entry would \
         kill a texture the caller still owns, and the wrong pixel would show up \
         much later"
    );
    device.release_texture(&texture2);
    assert_eq!(device.tracked_object_count(), before);
}

#[test]
fn d3d11_texture_upload_download_roundtrip() {
    let device = open_d3d11_device();
    // TRANSFER_* alone produce no D3D11 bind flag: D3D11 has no image-layout
    // barrier culture, copies are always legal, so the usage needs something
    // real to back the object (SAMPLED is the honest carrier here).
    let texture = device
        .create_texture(
            64,
            64,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the upload texture must be created");
    let data: Vec<u8> = (0..64 * 64 * 4).map(|i| (i & 0xff) as u8).collect();
    device
        .upload_texture(&texture, &data)
        .expect("the upload must succeed; a discarded error here would leave the \
                 readback below asserting against memory nobody wrote");

    let read_back = device.download_texture(&texture, 0, 0).expect("readback");
    assert_eq!(read_back.len(), data.len(), "the readback is a different size");
    assert_eq!(
        read_back, data,
        "the bytes that were uploaded must come back; a readback of zeros is a \
         successful-looking wrong answer"
    );
}

#[test]
fn d3d11_exact_width_forced_row_pitch_is_arbitrary() {
    let device = open_d3d11_device();
    // Odd width: nothing in the RHI declares the staging row pitch, so IF the
    // driver pads rows the readback must compact them back to width * 4. The
    // data is number-tagged per row so a silent stride slip cannot reproduce
    // the original pattern by luck.
    let width = 61u32;
    let height = 61u32;
    let texture = device
        .create_texture(
            width,
            height,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the pitch-test texture must be created");
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x + 97 * y + 3) % 251) as u8);
            data.push((((y * 5 + 1) % 251)) as u8);
            data.push((((x * 3 + 2) % 251)) as u8);
            data.push(0xff);
        }
    }
    device
        .upload_texture(&texture, &data)
        .expect("upload must succeed");
    let read_back = device.download_texture(&texture, 0, 0).expect("readback");
    assert_eq!(read_back.len(), data.len(), "the readback must be the texel grid, not the staging stride");
    assert_eq!(read_back, data, "row padding must be compacted away, not handed to the caller");
}

// Bug №738: the upload built its staging with `MipLevels: 1, ArraySize: 1`
// and handed it to an ignored-result `CopyResource`, so every texture beyond
// a single mip both failed the copy (mismatched subresource counts) AND hid
// the failure — the upload reported success while nothing landed. The mips
// kept their uninitialized content and even mip 0 stayed a ghost. The
// contract mirrors the D3D12 backend: the upload fills the mip chain of
// layer 0 from tightly packed rows, layer counts beyond one are refused
// rather than half-written, and troubleshooting facts (upload geometry,
// Map results) travel to the caller instead of being assumed.
#[test]
fn d3d11_device_texture_mip_chain_round_trip() {
    let device = open_d3d11_device();
    let texture = device
        .create_texture(
            64,
            64,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_SRC,
            3,
        )
        .expect("the multi-mip texture must be created");

    // Every mip carries its own pattern, so a stale or lost subresource
    // cannot pass for the expected content by accident.
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

    device
        .upload_texture(&texture, &data)
        .expect("the mip-chain upload must succeed; a silently inert copy here \
                 would leave the readbacks asserting against memory nobody wrote");
    assert_eq!(device.download_texture(&texture, 0, 0).unwrap(), mip0);
    assert_eq!(
        device.download_texture(&texture, 1, 0).unwrap(),
        mip1,
        "mip 1 must carry its own uploaded bytes, not uninitialized memory"
    );
    assert_eq!(device.download_texture(&texture, 2, 0).unwrap(), mip2);

    // The layer contract is the same honest refusal the Vulkan and D3D12
    // backends make today: uploads cover one layer, wider arrays are refused
    // rather than silently writing layer 0 only.
    let layered = device
        .create_texture_from_desc(&TextureDesc {
            width: 16,
            height: 16,
            depth: 1,
            mip_levels: 1,
            array_layers: 2,
            format: Format::RGBA8_UNORM,
            usage: TextureUsage::SAMPLED | TextureUsage::TRANSFER_SRC,
            sample_count: SampleCount::X1,
            dimensions: TextureDimensions::D2,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: Vec::new(),
        })
        .expect("the layered texture must be created");
    let result = device.upload_texture(&layered, &mip0);
    assert!(
        matches!(result, Err(rhi::RhiError::NotSupported(_))),
        "a two-layer upload must be a stated refusal, saw {result:?}"
    );
}

/// Bug №740: `wait_idle` once was a bare `Flush` — the driver accepted the
/// batch and the call reported "idle" while the GPU could still be running
/// it. The honest wait is a device-level contract: submit real bytes on the
/// real context, wait, then read the whole subresource set back. The
/// readback is the proof the returned "idle" refers to finished work, not
/// to a batch the driver merely accepted.
#[test]
fn d3d11_device_wait_idle_drains_submitted_work() {
    let device = open_d3d11_device();
    let texture = device
        .create_texture(
            32,
            32,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_SRC,
            2,
        )
        .expect("the texture must be created");

    // Per-mip patterns, so a lost or stale subresource cannot pass for the
    // expected content by accident.
    let mip = |level: u32, base: u8| -> Vec<u8> {
        let side = 32u32 >> level;
        (0..side * side * 4)
            .map(|i| (i as u8).wrapping_add(base))
            .collect()
    };
    let mip0 = mip(0, 7);
    let mip1 = mip(1, 19);
    assert_eq!(mip0.len(), 32 * 32 * 4);
    assert_eq!(mip1.len(), 16 * 16 * 4);
    let mut data = mip0.clone();
    data.extend_from_slice(&mip1);

    device
        .upload_texture(&texture, &data)
        .expect("the upload must succeed before the wait");
    device
        .wait_idle()
        .expect("a healthy D3D11 device must answer the idle wait");
    // Once the idle claim is out, the whole submitted set is observable.
    assert_eq!(device.download_texture(&texture, 0, 0).unwrap(), mip0);
    assert_eq!(
        device.download_texture(&texture, 1, 0).unwrap(),
        mip1,
        "after the idle claim every submitted subresource must be there"
    );
}

#[test]
fn d3d11_the_invalid_texture_description_is_an_error_and_not_a_stand_in() {
    let device = open_d3d11_device();
    let desc = TextureDesc {
        width: 0,
        height: 8,
        depth: 1,
        mip_levels: 1,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::SAMPLED,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    };
    let result = device.create_texture_from_desc(&desc);
    assert!(
        result.is_err(),
        "a texture with a zero width cannot exist; a CPU stand-in would be \
         reported as a success and poison the draw far away from the cause"
    );
}

#[test]
fn d3d11_a_view_of_a_texture_without_an_image_is_an_error() {
    let device = open_d3d11_device();
    let cpu_only = Texture::new(TextureDesc {
        width: 8,
        height: 8,
        depth: 1,
        mip_levels: 1,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::SAMPLED,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    });
    let view_desc = TextureViewDesc {
        texture: cpu_only.clone(),
        format: None,
        view_type: TextureViewType::D2,
        aspects: TextureAspectFlags::COLOR,
        base_mip_level: 0,
        mip_level_count: 1,
        base_array_layer: 0,
        array_layer_count: 1,
    };
    let result = device.create_texture_view(&cpu_only, &view_desc);
    assert!(
        result.is_err(),
        "there is no image to make a view of, so this has to be an error; a CPU \
         stand-in view cannot be bound to anything"
    );
}

#[test]
fn d3d11_sampler_creation() {
    let device = open_d3d11_device();
    let sampler = device
        .create_sampler(AddressMode::ClampToEdge, FilterMode::Linear, 16.0)
        .expect("the anisotropy-unclamped sampler must be created");
    assert!(sampler.has_gpu_backing());
    assert!(sampler.is_linear());
}

#[test]
fn d3d11_shader_module_takes_dx_bytes_and_refuses_junk() {
    let device = open_d3d11_device();
    let backend = device
        .backend_resource()
        .expect("a D3D11 device has a backend resource");

    let code = compile_hlsl(
        b"struct VsOut { float4 pos : SV_POSITION; }; \
          VsOut main(float4 pos : POSITION) { VsOut o; o.pos = pos; return o; }",
        "main",
        "vs_5_0",
    )
    .expect("the HLSL compile must succeed for the module to exist");
    assert_eq!(&code[..4], b"DXBC", "the container must be a real one");

    let desc = ShaderModuleDesc {
        code,
        format: ShaderFormat::Dxil,
        entry_point: Some("main".into()),
        name: Some("d3d11/resources vs".into()),
    };
    let module = backend
        .create_shader_module(&desc)
        .expect("the finished container must be accepted");
    assert!(module.has_gpu_backing());

    let junk = ShaderModuleDesc {
        code: b"not a container at all".to_vec(),
        format: ShaderFormat::Dxil,
        entry_point: None,
        name: Some("d3d11/resources junk".into()),
    };
    let error = backend
        .create_shader_module(&junk)
        .expect_err("bytes that are not a shader container must be refused here, \
                     not by the driver much later and with a message about the \
                     shader");
    let text = format!("{error}");
    assert!(
        text.contains("DXBC"),
        "the refusal should say what the bytes fail to be, got: {text}"
    );
}

#[test]
fn d3d11_the_pipeline_formats_outside_dx_are_refused() {
    let device = open_d3d11_device();
    let backend = device.backend_resource().unwrap();
    let desc = ShaderModuleDesc {
        code: b"".to_vec(),
        format: ShaderFormat::SpirV,
        entry_point: None,
        name: Some("d3d11/resources spirv".into()),
    };
    let error = backend
        .create_shader_module(&desc)
        .expect_err("SPIR-V cannot be bound by D3D11");
    let text = format!("{error}");
    assert!(
        text.contains("DXBC"),
        "the refusal should point at the accepted container magic, got: {text}"
    );
}
