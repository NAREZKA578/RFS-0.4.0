// Integration tests for the rhi::backend module.
//
// Covers: Backend trait object safety, per-API backend stubs, and
// create_backend dispatch.

use rhi::backend::d3d11::compile::compile_hlsl;
use rhi::backend::{create_backend, Backend};
use rhi::backend::common::ReleaseKind;
use rhi::backend::d3d11::D3D11Backend;
use rhi::backend::d3d12::D3D12Backend;
use rhi::backend::opengl::OpenGLBackend;
use rhi::backend::vulkan::VulkanBackend;
use rhi::command::commands::Command;
use rhi::command::pass::render::{
    AttachmentDescription, AttachmentReference, LoadOp, RenderPassBeginInfo, RenderPassDesc,
    StoreOp, SubpassDescription,
};
use rhi::config::RhiConfig;
use rhi::core::device::{DeviceDesc, PhysicalDevice, PhysicalDeviceType};
use rhi::descriptor::{
    DescriptorBinding, DescriptorInfo, DescriptorSetLayoutDesc, DescriptorType, DescriptorWrite,
};
use rhi::pipeline::compute::{ComputePipeline, ComputePipelineDesc};
use rhi::pipeline::graphics::{
    GraphicsPipelineDesc, InputAssemblyState, PipelineShaderStage, PrimitiveTopology,
    VertexInputState, ViewportState,
};
use rhi::resource::{BufferDesc, TextureDesc, TextureLayout, TextureViewDesc, TextureViewType};
use rhi::shader::module::ShaderModuleDesc;
use rhi::shader::ShaderFormat;
use rhi::sync::barrier::{BufferCopy, BufferTextureCopy};
use rhi::types::GraphicsApi;
use rhi::types::{
    BufferUsage, Format, MemoryPropertyFlags, SampleCount, SharingMode, TextureAspectFlags,
    ShaderStage,
};
use rhi::types::{Features, Limits, TextureDimensions, TextureUsage};
use rhi::types::{ClearValue, Extent2D, IndexType, Offset2D, Rect2D, Viewport};
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

// Bug №706: `submit_recorded` on the D3D12 device inherited a
// NotSupported default — the recorded command surface did not exist. The M2
// contract: a recorded `CopyBuffer` replays on a real graphics command list
// (reset → record → close → `ExecuteCommandLists` → fence wait), and the
// destination bytes are readable back unchanged.
#[test]
fn d3d12_submit_recorded_copy_round_trip() {
    let device = d3d12_test_device();

    // The source is an upload-heap buffer the host wrote through the managed
    // path; the copy itself must travel through the recorded list.
    let src = device
        .create_buffer(512, BufferUsage::TRANSFER_SRC, true)
        .unwrap();
    let payload: Vec<u32> = (0..128).map(|i| 0x500_000 + i as u32).collect();
    device.upload_buffer::<u32>(&src, &payload).unwrap();

    let dst_desc = BufferDesc {
        size: 512,
        usage: BufferUsage::TRANSFER_DST,
        memory_flags: MemoryPropertyFlags::DEVICE_LOCAL,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    };
    let dst = device.create_buffer_from_desc(&dst_desc).unwrap();

    let commands = vec![Command::CopyBuffer {
        src: src.clone(),
        dst: dst.clone(),
        regions: vec![BufferCopy {
            src_offset: 0,
            dst_offset: 0,
            size: 512,
        }],
    }];
    device.submit_recorded(&commands).unwrap();

    // The device-local destination only reads through a staging download.
    let out = device.download_buffer(&dst, 0, 512).unwrap();
    let back: Vec<u32> = out
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if back != payload {
        let first = back.iter().zip(payload.iter()).position(|(a, b)| a != b);
        eprintln!(
            "recorded copy mismatch: first at u32 index {:?}, saw {:?}, expected {:?}",
            first,
            back.iter().take(8).collect::<Vec<_>>(),
            payload.iter().take(8).collect::<Vec<_>>()
        );
    }
    assert_eq!(back, payload);
    device.wait_idle().unwrap();
}

// The recorded texture upload (Bug №706): a `CopyBufferToTexture` whose tight
// row happens to be a multiple of the 256-byte staging alignment replays
// directly — width 256 RGBA8 makes the row 1024 bytes. Deeper usage stays
// with the managed `upload_texture`, which stages the general case itself.
#[test]
fn d3d12_submit_recorded_texture_upload_round_trip() {
    let device = d3d12_test_device();
    let src = device
        .create_buffer(1024, BufferUsage::TRANSFER_SRC, true)
        .unwrap();
    let payload: Vec<u8> = (0..1024).map(|i| (i * 7 % 251) as u8).collect();
    device.upload_buffer::<u8>(&src, &payload).unwrap();

    let desc = TextureDesc {
        width: 256,
        height: 1,
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

    let commands = vec![Command::CopyBufferToTexture {
        src: src.clone(),
        dst: texture.clone(),
        regions: vec![BufferTextureCopy {
            buffer_offset: 0,
            size: 1024,
        }],
    }];
    device.submit_recorded(&commands).unwrap();

    let back = device.download_texture(&texture, 0, 0).unwrap();
    assert_eq!(back.len(), 1024);
    assert_eq!(back, payload);
}

// The recorded texture upload refuses what the D3D12 copy contract cannot
// take silently: a tight row narrower than the 256-byte staging alignment
// (width 32 RGBA8 is 128 bytes) must be refused as NotSupported rather than
// arriving as misaligned-at-the-source bytes (Bug №706).
#[test]
fn d3d12_submit_recorded_refuses_unaligned_texture_rows() {
    let device = d3d12_test_device();
    let src = device
        .create_buffer(128, BufferUsage::TRANSFER_SRC, true)
        .unwrap();
    let payload: Vec<u8> = (0..128).map(|i| i as u8).collect();
    device.upload_buffer::<u8>(&src, &payload).unwrap();

    let desc = TextureDesc {
        width: 32,
        height: 1,
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

    let commands = vec![Command::CopyBufferToTexture {
        src: src.clone(),
        dst: texture.clone(),
        regions: vec![BufferTextureCopy {
            buffer_offset: 0,
            size: 128,
        }],
    }];
    let result = device.submit_recorded(&commands);
    assert!(
        matches!(result, Err(RhiError::NotSupported(_))),
        "an unaligned recorded row must be refused, got {result:?}"
    );
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

// —————————————————————————————————————————————————————————————————————
// M3 render path (BUGS №707): the recorded commands replay as real D3D12
// draws, and the pixels report the truth. The shaders compile at test time
// with the shared runtime helper (`rhi::backend::d3d11::compile` — the
// comment there says "a helper everyone can share"): the SM 5.0 DXBC
// containers D3D11 binds are the same finished containers the D3D12 device
// accepts as `ShaderFormat::Dxil`.
// —————————————————————————————————————————————————————————————————————

const TRIANGLE_VS: &str = r#"struct VSIn
{
    float3 pos : TEXCOORD0;
    float4 col : TEXCOORD1;
};
struct PSIn
{
    float4 pos : SV_Position;
    float4 col : COLOR0;
};
PSIn main(VSIn input)
{
    PSIn output;
    output.pos = float4(input.pos, 1.0);
    output.col = input.col;
    return output;
}
"#;

// The PS mirrors the full VS output struct on purpose (BUGS №762): D3D12
// links shader signatures by exact hardware register indexes — a pixel
// shader that drops SV_Position shifts every following input down one
// register, and COLOR0 lands on 0 while the VS writes it to 1; the PSO
// creation then refuses with 0x80070057 and the debug layer names
// CREATEGRAPHICSPIPELINESTATE_SHADER_LINKAGE_REGISTERINDEX. D3D11 links by
// semantic name only, which is why the shape had to be written for the
// stricter of the two backends.
const VARIABLE_PS: &str = r#"struct PSIn
{
    float4 pos : SV_Position;
    float4 col : COLOR0;
};
float4 main(PSIn input) : SV_Target
{
    return input.col;
}
"#;

const VERTEX_ID_VS: &str = r#"struct PSIn
{
    float4 pos : SV_Position;
};
PSIn main(uint vertex_id : SV_VertexID)
{
    PSIn output;
    float2 xy = float2(vertex_id == 1 ? 3.0 : -1.0, vertex_id == 2 ? 3.0 : -1.0);
    output.pos = float4(xy, 0.0, 1.0);
    return output;
}
"#;

const TINT_PS: &str = r#"cbuffer tint : register(b0)
{
    float4 tint_color;
};
float4 main() : SV_Target
{
    return tint_color;
}
"#;

/// The single-attachment RGBA8 render pass the pixel draws share: cleared
/// on load, stored, one subpass.
fn d3d12_pixel_render_pass() -> RenderPassDesc {
    RenderPassDesc {
        attachments: vec![AttachmentDescription {
            format: Format::RGBA8_UNORM,
            samples: SampleCount::X1,
            load_op: LoadOp::Clear,
            store_op: StoreOp::Store,
            stencil_load_op: LoadOp::DontCare,
            stencil_store_op: StoreOp::DontCare,
            initial_layout: TextureLayout::Undefined,
            final_layout: TextureLayout::ColorAttachmentOptimal,
        }],
        subpasses: vec![SubpassDescription {
            color_attachments: vec![AttachmentReference {
                attachment: 0,
                layout: TextureLayout::ColorAttachmentOptimal,
            }],
            ..Default::default()
        }],
        dependencies: Vec::new(),
    }
}

/// A 64x64 RGBA8 colour texture, its full view, and the framebuffer that
/// sits the view into the pixel pass — the draw surface.
fn d3d12_pixel_framebuffer(
    device: &rhi::core::device::Device,
    pass: &rhi::command::pass::render::RenderPass,
) -> (rhi::resource::Texture, rhi::command::pass::render::Framebuffer) {
    let texture = device
        .create_texture_from_desc(&TextureDesc {
            width: 64,
            height: 64,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: Format::RGBA8_UNORM,
            usage: TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            sample_count: SampleCount::X1,
            dimensions: TextureDimensions::D2,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: Vec::new(),
        })
        .unwrap();
    assert!(texture.has_gpu_backing());
    let view = device
        .create_texture_view(
            &texture,
            &TextureViewDesc {
                texture: texture.clone(),
                format: None,
                view_type: TextureViewType::D2,
                aspects: TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 0,
                base_array_layer: 0,
                array_layer_count: 0,
            },
        )
        .unwrap();
    let framebuffer = device.create_framebuffer(pass, vec![view], 64, 64).unwrap();
    (texture, framebuffer)
}

fn d3d12_pixel_back(
    device: &rhi::core::device::Device,
    texture: &rhi::resource::Texture,
) -> Vec<[u8; 4]> {
    device
        .download_texture(texture, 0, 0)
        .unwrap()
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect()
}

// The pipeline draw: pass clears to black, one full-screen triangle paints
// pure green over the whole 64x64 target, the fixed-function stage binds
// what the pipeline declared, and the pixels report the whole trip: every
// one of the 4096 must be exactly green — the D3D12 recorded draw path
// (pass state, root signature, PSO, IA, OM and the tracked transits)
// produced its picture (BUGS №707).
#[test]
fn d3d12_render_full_screen_triangle_pixels() {
    let device = d3d12_test_device();
    let pass = device.create_render_pass(&d3d12_pixel_render_pass()).unwrap();
    let (texture, framebuffer) = d3d12_pixel_framebuffer(&device, &pass);

    let vs_code = compile_hlsl(TRIANGLE_VS.as_bytes(), "main", "vs_5_0").unwrap();
    let ps_code = compile_hlsl(VARIABLE_PS.as_bytes(), "main", "ps_5_0").unwrap();
    let vs = device
        .create_shader_module(&ShaderModuleDesc {
            code: vs_code,
            format: ShaderFormat::Dxil,
            entry_point: Some("main".into()),
            name: Some("d3d12 m3 test vs".into()),
        })
        .unwrap();
    let ps = device
        .create_shader_module(&ShaderModuleDesc {
            code: ps_code,
            format: ShaderFormat::Dxil,
            entry_point: Some("main".into()),
            name: Some("d3d12 m3 test ps".into()),
        })
        .unwrap();

    let pipeline = device
        .create_graphics_pipeline(
            &GraphicsPipelineDesc {
                shader_stages: vec![
                    PipelineShaderStage {
                        stage: ShaderStage::VERTEX,
                        module: vs,
                        entry_point: "main".into(),
                    },
                    PipelineShaderStage {
                        stage: ShaderStage::FRAGMENT,
                        module: ps,
                        entry_point: "main".into(),
                    },
                ],
                vertex_input_state: Some(VertexInputState::from_formats(&[
                    (0, Format::R32G32B32_SFLOAT),
                    (1, Format::RGBA32_SFLOAT),
                ])),
                input_assembly_state: InputAssemblyState {
                    topology: PrimitiveTopology::TriangleList,
                    primitive_restart_enable: false,
                },
                viewport_state: ViewportState {
                    viewports: vec![Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: 64.0,
                        height: 64.0,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }],
                    scissors: vec![Rect2D {
                        offset: Offset2D { x: 0, y: 0 },
                        extent: Extent2D {
                            width: 64,
                            height: 64,
                        },
                    }],
                },
                ..Default::default()
            },
            &pass,
            &[],
        )
        .unwrap();
    assert!(pipeline.has_gpu_backing());

    // The vertex layout mirrors VertexInputState::from_formats: position
    // first (12 bytes), colour second (16), the stride 28 bytes — a stride
    // 28 % 4 == 0 takes, and the D3D12 input assembler wants a four-byte
    // multiple.
    let positions = [
        [-1.0f32, -1.0, 0.0],
        [3.0, -1.0, 0.0],
        [-1.0, 3.0, 0.0],
    ];
    let colour = [0.0f32, 1.0, 0.0, 1.0];
    let mut vertex_bytes: Vec<u8> = Vec::new();
    for pos in positions {
        for v in pos {
            vertex_bytes.extend_from_slice(&v.to_le_bytes());
        }
        for v in colour {
            vertex_bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    let vertex_buffer = device
        .create_buffer(vertex_bytes.len() as u64, BufferUsage::VERTEX, true)
        .unwrap();
    device.upload_buffer::<u8>(&vertex_buffer, &vertex_bytes).unwrap();

    device
        .draw_framebuffer_indexed(
            &framebuffer,
            &pipeline,
            &[],
            Some((&vertex_buffer, 0)),
            None,
            IndexType::U32,
            3,
            0,
            0,
            0,
            0,
            &[ClearValue::color(0.0, 0.0, 0.0, 0.0)],
        )
        .unwrap();

    let back = d3d12_pixel_back(&device, &texture);
    assert_eq!(back.len(), 64 * 64);
    for (index, pixel) in back.iter().enumerate() {
        assert_eq!(pixel, &[0, 255, 0, 255], "pixel {index} must be green",);
    }
}

// The pass clear on its own: a begin whose only colour attachment loads
// Clear must paint every pixel with its positional clear value, and leave
// the target back in its home state the recorded EndRenderPass walks it
// to. The pixel truth of the clear load (BUGS №707).
#[test]
fn d3d12_render_pass_clear_only_pixels() {
    let device = d3d12_test_device();
    let pass_desc = d3d12_pixel_render_pass();
    let pass = device.create_render_pass(&pass_desc).unwrap();
    let (texture, framebuffer) = d3d12_pixel_framebuffer(&device, &pass);

    device
        .submit_recorded(&[
            Command::BeginRenderPass(RenderPassBeginInfo {
                render_pass: pass.clone(),
                framebuffer: framebuffer.clone(),
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: Extent2D {
                        width: 64,
                        height: 64,
                    },
                },
                clear_values: vec![ClearValue::color(1.0, 0.0, 0.0, 1.0)],
            }),
            Command::EndRenderPass,
        ])
        .unwrap();

    let back = d3d12_pixel_back(&device, &texture);
    assert_eq!(back.len(), 64 * 64);
    for (index, pixel) in back.iter().enumerate() {
        assert_eq!(pixel, &[255, 0, 0, 255], "pixel {index} must be clear red",);
    }
}

// The standalone clear: a view cleared through the recorder's temporary
// RTV slot, tracked in and out of RENDER_TARGET around the clear. No pass
// and no framebuffer — the pixels name the view's own truth.
#[test]
fn d3d12_standalone_clear_color_pixels() {
    let device = d3d12_test_device();
    let (texture, view) = {
        let texture = device
            .create_texture_from_desc(&TextureDesc {
                width: 64,
                height: 64,
                depth: 1,
                mip_levels: 1,
                array_layers: 1,
                format: Format::RGBA8_UNORM,
                usage: TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
                sample_count: SampleCount::X1,
                dimensions: TextureDimensions::D2,
                sharing_mode: SharingMode::Exclusive,
                queue_family_indices: Vec::new(),
            })
            .unwrap();
        let view = device
            .create_texture_view(
                &texture,
                &TextureViewDesc {
                    texture: texture.clone(),
                    format: None,
                    view_type: TextureViewType::D2,
                    aspects: TextureAspectFlags::COLOR,
                    base_mip_level: 0,
                    mip_level_count: 0,
                    base_array_layer: 0,
                    array_layer_count: 0,
                },
            )
            .unwrap();
        (texture, view)
    };

    device
        .submit_recorded(&[Command::ClearColor {
            texture: view,
            value: ClearValue::color(1.0, 0.0, 0.0, 1.0),
            rects: Vec::new(),
        }])
        .unwrap();

    let back = d3d12_pixel_back(&device, &texture);
    assert_eq!(back.len(), 64 * 64);
    for (index, pixel) in back.iter().enumerate() {
        assert_eq!(pixel, &[255, 0, 0, 255], "pixel {index} must be clear red",);
    }
}

// The descriptor table: a fragment uniform block binding (CBV at b0) reads
// its colour through a written descriptor set — SetDescriptorHeaps, then
// the root descriptor table pointing at the set's block in the
// shader-visible heap. The pixels prove the write reached the shader from
// the one heap the whole M3 binding model draws from (BUGS №707).
#[test]
fn d3d12_descriptor_table_uniform_block_pixels() {
    let device = d3d12_test_device();
    let pass_desc = d3d12_pixel_render_pass();
    let pass = device.create_render_pass(&pass_desc).unwrap();
    let (texture, framebuffer) = d3d12_pixel_framebuffer(&device, &pass);

    let layout = device
        .create_descriptor_set_layout(&DescriptorSetLayoutDesc {
            bindings: vec![DescriptorBinding {
                binding: 0,
                ty: DescriptorType::UniformBuffer,
                count: 1,
                stages: ShaderStage::FRAGMENT,
                immutable_samplers: None,
            }],
        })
        .unwrap();
    let set = device.create_descriptor_set(&layout).unwrap();

    // The tint the block carries: pure blue, so the f32-to-UNORM8 pixel
    // conversion is exact and no rounding half-tie can flip a byte.
    let mut tint: Vec<u8> = Vec::new();
    for value in [0.0f32, 0.0, 1.0, 1.0] {
        tint.extend_from_slice(&value.to_le_bytes());
    }
    let uniform = device
        .create_buffer(tint.len() as u64, BufferUsage::UNIFORM, true)
        .unwrap();
    device.upload_buffer::<u8>(&uniform, &tint).unwrap();
    device
        .write_descriptors(&set, &[DescriptorWrite {
            dst_set: set.clone(),
            dst_binding: 0,
            dst_array_element: 0,
            descriptors: vec![DescriptorInfo::Buffer(uniform, 0, 16)],
        }])
        .unwrap();

    let vs_code = compile_hlsl(VERTEX_ID_VS.as_bytes(), "main", "vs_5_0").unwrap();
    let ps_code = compile_hlsl(TINT_PS.as_bytes(), "main", "ps_5_0").unwrap();
    let vs = device
        .create_shader_module(&ShaderModuleDesc {
            code: vs_code,
            format: ShaderFormat::Dxil,
            entry_point: Some("main".into()),
            name: Some("d3d12 m3 test vid vs".into()),
        })
        .unwrap();
    let ps = device
        .create_shader_module(&ShaderModuleDesc {
            code: ps_code,
            format: ShaderFormat::Dxil,
            entry_point: Some("main".into()),
            name: Some("d3d12 m3 test tint ps".into()),
        })
        .unwrap();

    let pipeline = device
        .create_graphics_pipeline(
            &GraphicsPipelineDesc {
                shader_stages: vec![
                    PipelineShaderStage {
                        stage: ShaderStage::VERTEX,
                        module: vs,
                        entry_point: "main".into(),
                    },
                    PipelineShaderStage {
                        stage: ShaderStage::FRAGMENT,
                        module: ps,
                        entry_point: "main".into(),
                    },
                ],
                input_assembly_state: InputAssemblyState {
                    topology: PrimitiveTopology::TriangleList,
                    primitive_restart_enable: false,
                },
                viewport_state: ViewportState {
                    viewports: vec![Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: 64.0,
                        height: 64.0,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }],
                    scissors: vec![Rect2D {
                        offset: Offset2D { x: 0, y: 0 },
                        extent: Extent2D {
                            width: 64,
                            height: 64,
                        },
                    }],
                },
                ..Default::default()
            },
            &pass,
            &[&layout],
        )
        .unwrap();
    assert!(pipeline.has_gpu_backing());

    device
        .draw_framebuffer(
            &framebuffer,
            &pipeline,
            &[&set],
            None,
            3,
            0,
            &[ClearValue::color(0.0, 0.0, 0.0, 0.0)],
        )
        .unwrap();

    let back = d3d12_pixel_back(&device, &texture);
    assert_eq!(back.len(), 64 * 64);
    for (index, pixel) in back.iter().enumerate() {
        assert_eq!(pixel, &[0, 0, 255, 255], "pixel {index} must be tint blue",);
    }
}

// The pass contract holds at the command layer too: a draw outside a pass
// and vertex buffers without a bound pipeline are refused (a silent one
// draws a wrong picture), and the compute path plus subpass stepping stay
// honest refusals until their terrain (M4) and inside the single-subpass
// scope the backend supports (BUGS №707).
#[test]
fn d3d12_render_refusals_pin_the_pass_contract() {
    let device = d3d12_test_device();

    let result = device.submit_recorded(&[Command::Draw {
        vertex_count: 3,
        instance_count: 1,
        first_vertex: 0,
        first_instance: 0,
    }]);
    assert!(
        matches!(result, Err(RhiError::BackendError(_))),
        "a draw outside a pass must be refused, got {result:?}"
    );

    let buffer = device.create_buffer(64, BufferUsage::VERTEX, true).unwrap();
    let result = device.submit_recorded(&[Command::BindVertexBuffers {
        first_binding: 0,
        buffers: vec![(buffer, 0)],
    }]);
    assert!(
        matches!(result, Err(RhiError::BackendError(_))),
        "vertex buffers without a pipeline must be refused, got {result:?}"
    );

    let result = device.submit_recorded(&[Command::NextSubpass]);
    assert!(
        matches!(result, Err(RhiError::NotSupported(_))),
        "subpass stepping must stay a stated refusal, got {result:?}"
    );

    let result = device.submit_recorded(&[Command::BindComputePipeline(
        ComputePipeline::new(ComputePipelineDesc::default()),
    )]);
    assert!(
        matches!(result, Err(RhiError::NotSupported(_))),
        "the compute path must stay a stated refusal until M4, got {result:?}"
    );

    let result = device.submit_recorded(&[Command::Dispatch {
        x: 1,
        y: 1,
        z: 1,
    }]);
    assert!(
        matches!(result, Err(RhiError::NotSupported(_))),
        "Dispatch must stay a stated refusal until M4, got {result:?}"
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