// Integration tests for indexed graphics draws and depth-buffer attachment
// handling in the Vulkan backend.
//
// A single indexed draw submits a big far quad (red, z = 0.25) followed by a
// small near quad (green, z = -0.25) as separate triangles in one index
// buffer. The depth attachment must keep the near quad visible where the two
// overlap and let the far quad paint the surrounding ring, proving both the
// index path (`cmd_draw_indexed`) and the depth-buffer subpass wiring.

use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::pipeline::graphics::{DepthStencilState, PrimitiveTopology};
use rhi::shader::module::{ShaderFormat, ShaderModuleDesc};
use rhi::types::{Features, ShaderStage};
use rhi::{
    AttachmentDescription, AttachmentReference, Backend, ColorBlendAttachment, ColorBlendState,
    ColorComponentFlags, ColorTargetDesc, FramebufferAttachment, FramebufferDesc,
    GraphicsPipelineDesc, IndexType, InputAssemblyState, LoadOp, MultisampleState,
    PipelineBindPoint, PipelineShaderStage, RasterizerState, RenderPassDesc, SampleCount, StoreOp,
    SubpassDescription, TextureLayout, TextureUsage,
};

const SHADER: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// Positions 0..=3 are the big far quad, 4..=7 the small near quad.
@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var far = array<vec2<f32>, 4>(
        vec2<f32>(-0.8, -0.8),
        vec2<f32>( 0.8, -0.8),
        vec2<f32>( 0.8,  0.8),
        vec2<f32>(-0.8,  0.8),
    );
    var near = array<vec2<f32>, 4>(
        vec2<f32>(-0.3, -0.3),
        vec2<f32>( 0.3, -0.3),
        vec2<f32>( 0.3,  0.3),
        vec2<f32>(-0.3,  0.3),
    );
    var out: VsOut;
    if (vid < 4u) {
        out.pos = vec4<f32>(far[vid], 0.75, 1.0);
        out.color = vec4<f32>(1.0, 0.0, 0.0, 1.0);
    } else {
        out.pos = vec4<f32>(near[vid - 4u], 0.25, 1.0);
        out.color = vec4<f32>(0.0, 1.0, 0.0, 1.0);
    }
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

fn open_vulkan_device(features: Features) -> Device {
    let backend = VulkanBackend::new(&RhiConfig::default()).unwrap();
    let devices = backend.enumerate_physical_devices().unwrap();
    let desc = DeviceDesc {
        features,
        ..Default::default()
    };
    backend.create_device(&devices[0], &desc).unwrap()
}

fn wgsl_to_spirv(source: &str) -> Vec<u8> {
    let module = naga::front::wgsl::parse_str(source).expect("WGSL parse");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator.validate(&module).expect("WGSL validation");
    let options = naga::back::spv::Options::default();
    let words = naga::back::spv::write_vec(&module, &info, &options, None).expect("SPIR-V emit");
    let mut bytes = Vec::with_capacity(words.len() * 4);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn all_channels() -> ColorComponentFlags {
    ColorComponentFlags::R
        | ColorComponentFlags::G
        | ColorComponentFlags::B
        | ColorComponentFlags::A
}

#[test]
fn vulkan_offscreen_indexed_depth_roundtrip() {
    let device = open_vulkan_device(Features::default());

    const EXTENT: u32 = 64;
    let color = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        rhi::Format::RGBA8_UNORM,
        TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
        1,
    );
    assert!(color.has_gpu_backing());
    let color_view = device
        .create_texture_view(&color, color.create_default_view().desc())
        .expect("color attachment view");
    assert!(color_view.has_gpu_backing());

    let depth = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        rhi::Format::D32_SFLOAT,
        TextureUsage::DEPTH_STENCIL_ATTACHMENT,
        1,
    );
    assert!(depth.has_gpu_backing());
    let depth_view = device
        .create_texture_view(&depth, depth.create_default_view().desc())
        .expect("depth attachment view");
    assert!(depth_view.has_gpu_backing());

    let render_pass = device
        .create_gpu_render_pass(&RenderPassDesc {
            attachments: vec![
                AttachmentDescription {
                    format: rhi::Format::RGBA8_UNORM,
                    samples: SampleCount::X1,
                    load_op: LoadOp::Clear,
                    store_op: StoreOp::Store,
                    stencil_load_op: LoadOp::DontCare,
                    stencil_store_op: StoreOp::DontCare,
                    initial_layout: TextureLayout::Undefined,
                    final_layout: TextureLayout::ColorAttachmentOptimal,
                },
                AttachmentDescription {
                    format: rhi::Format::D32_SFLOAT,
                    samples: SampleCount::X1,
                    load_op: LoadOp::Clear,
                    store_op: StoreOp::DontCare,
                    stencil_load_op: LoadOp::DontCare,
                    stencil_store_op: StoreOp::DontCare,
                    initial_layout: TextureLayout::Undefined,
                    final_layout: TextureLayout::DepthAttachmentOptimal,
                },
            ],
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: TextureLayout::ColorAttachmentOptimal,
                }],
                depth_stencil_attachment: Some(AttachmentReference {
                    attachment: 1,
                    layout: TextureLayout::DepthAttachmentOptimal,
                }),
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");
    assert!(render_pass.has_gpu_backing());

    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![
                FramebufferAttachment {
                    texture_view: color_view,
                    layer: 0,
                    mip_level: 0,
                },
                FramebufferAttachment {
                    texture_view: depth_view,
                    layer: 0,
                    mip_level: 0,
                },
            ],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");
    assert!(framebuffer.has_gpu_backing());

    let module = device.create_shader_module(&ShaderModuleDesc {
        code: wgsl_to_spirv(SHADER),
        format: ShaderFormat::SpirV,
        entry_point: Some("vs_main".into()),
        name: Some("indexed/depth test".into()),
    });
    let frag = device.create_shader_module(&ShaderModuleDesc {
        code: wgsl_to_spirv(SHADER),
        format: ShaderFormat::SpirV,
        entry_point: Some("fs_main".into()),
        name: Some("indexed/depth test".into()),
    });
    assert!(module.has_gpu_backing());
    assert!(frag.has_gpu_backing());

    let pipeline = device
        .create_gpu_graphics_pipeline(
            &GraphicsPipelineDesc {
                shader_stages: vec![
                    PipelineShaderStage {
                        stage: ShaderStage::VERTEX,
                        module,
                        entry_point: "vs_main".into(),
                    },
                    PipelineShaderStage {
                        stage: ShaderStage::FRAGMENT,
                        module: frag,
                        entry_point: "fs_main".into(),
                    },
                ],
                input_assembly_state: InputAssemblyState {
                    topology: PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                rasterizer_state: Some(RasterizerState::double_sided()),
                multisample_state: MultisampleState {
                    sample_count: SampleCount::X1,
                    ..Default::default()
                },
                depth_stencil_state: Some(DepthStencilState::enabled()),
                color_blend_state: Some(ColorBlendState {
                    attachments: vec![ColorBlendAttachment {
                        color_write_mask: all_channels(),
                        ..ColorTargetDesc::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            &render_pass,
            &[],
        )
        .expect("graphics pipeline");
    assert!(pipeline.has_gpu_backing());

    let indices: [u16; 12] = [0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4];
    let index_bytes: Vec<u8> = indices.iter().flat_map(|v| v.to_le_bytes()).collect();
    let index_buffer = device.create_buffer(
        index_bytes.len() as u64,
        rhi::BufferUsage::INDEX | rhi::BufferUsage::TRANSFER_DST,
        false,
    );
    device.upload_buffer(&index_buffer, &index_bytes);

    device
        .draw_framebuffer_indexed(
            &framebuffer,
            &pipeline,
            &[],
            None,
            Some((&index_buffer, 0)),
            IndexType::U16,
            8,
            0,
            12,
            0,
            0,
            &[
                rhi::ClearValue::color(0.0, 0.0, 1.0, 1.0),
                rhi::ClearValue::DepthStencil { depth: 1.0, stencil: 0 },
            ],
        )
        .expect("indexed draw framebuffer");

    let bytes = device.download_texture(&color, 0, 0).expect("readback");
    assert_eq!(bytes.len(), (EXTENT * EXTENT * 4) as usize);

    let pixel = |x: u32, y: u32| -> [u8; 4] {
        let offset = ((y * EXTENT + x) * 4) as usize;
        [
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]
    };

    let center = pixel(EXTENT / 2, EXTENT / 2);
    assert!(
        center[1] > 200 && center[0] < 50 && center[2] < 50,
        "expected the near quad to win the depth test at the center, got {center:?}"
    );

    let corner = pixel(10, 32);
    assert!(
        corner[0] > 200 && corner[1] < 50 && corner[2] < 50,
        "expected the far quad to paint the surrounding ring, got {corner:?}"
    );
}