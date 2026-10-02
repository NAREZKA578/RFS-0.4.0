// Integration test that samples a texture through a descriptor set bound to the
// Vulkan graphics pipeline draw path, then reads the framebuffer back.
//
// A fullscreen quad is drawn with a 2x2 texel texture magnified with a nearest
// sampler. Each quadrant of the framebuffer must show one distinct texel color,
// which exercises texture uploads, texture views, samplers, descriptor set
// layouts/writes, and descriptor binding during an offscreen draw.

use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::descriptor::{
    DescriptorBinding, DescriptorInfo, DescriptorSetLayoutDesc, DescriptorType, DescriptorWrite,
};
use rhi::pipeline::graphics::PrimitiveTopology;
use rhi::shader::module::{ShaderFormat, ShaderModuleDesc};
use rhi::types::{Features, ShaderStage};
use rhi::{
    AttachmentDescription, AttachmentReference, Backend, ColorBlendAttachment, ColorBlendState,
    ColorComponentFlags, ColorTargetDesc, FramebufferAttachment, FramebufferDesc,
    GraphicsPipelineDesc, InputAssemblyState, LoadOp, MultisampleState, PipelineBindPoint,
    PipelineShaderStage, RasterizerState, RenderPassDesc, SampleCount, StoreOp,
    SubpassDescription, TextureLayout, TextureUsage,
};

const SHADER: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var pos = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );
    var uv = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(pos[vid], 0.5, 1.0);
    out.uv = uv[vid];
    return out;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(tex, smp, in.uv, 0.0);
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
fn vulkan_offscreen_textured_quad_roundtrip() {
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
    let color_view = device
        .create_texture_view(&color, color.create_default_view().desc())
        .expect("color attachment view");
    assert!(color_view.has_gpu_backing());

    let render_pass = device
        .create_gpu_render_pass(&RenderPassDesc {
            attachments: vec![AttachmentDescription {
                format: rhi::Format::RGBA8_UNORM,
                samples: SampleCount::X1,
                load_op: LoadOp::Clear,
                store_op: StoreOp::Store,
                stencil_load_op: LoadOp::DontCare,
                stencil_store_op: StoreOp::DontCare,
                initial_layout: TextureLayout::Undefined,
                final_layout: TextureLayout::ColorAttachmentOptimal,
            }],
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: TextureLayout::ColorAttachmentOptimal,
                }],
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");

    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![FramebufferAttachment {
                texture_view: color_view,
                layer: 0,
                mip_level: 0,
            }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    let texel_data: Vec<u8> = vec![
        255, 0, 0, 255, // top-left red
        0, 255, 0, 255, // top-right green
        0, 0, 255, 255, // bottom-left blue
        255, 255, 255, 255, // bottom-right white
    ];
    let sampled = device.create_texture(
        2,
        2,
        1,
        rhi::Format::RGBA8_UNORM,
        TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        1,
    );
    assert!(sampled.has_gpu_backing());
    device.upload_texture(&sampled, &texel_data);

    let sampled_view = device
        .create_texture_view(&sampled, sampled.create_default_view().desc())
        .expect("sampled view");
    assert!(sampled_view.has_gpu_backing());

    let sampler = device.create_sampler(
        rhi::AddressMode::ClampToEdge,
        rhi::FilterMode::Nearest,
        0.0,
    );
    assert!(sampler.has_gpu_backing());

    let layout = device.create_descriptor_set_layout(&DescriptorSetLayoutDesc {
        bindings: vec![
            DescriptorBinding {
                binding: 0,
                ty: DescriptorType::SampledTexture,
                count: 1,
                stages: ShaderStage::FRAGMENT,
                immutable_samplers: None,
            },
            DescriptorBinding {
                binding: 1,
                ty: DescriptorType::Sampler,
                count: 1,
                stages: ShaderStage::FRAGMENT,
                immutable_samplers: None,
            },
        ],
    });
    assert!(layout.has_gpu_backing());
    assert_eq!(layout.total_descriptors(), 2);

    let set = device.create_descriptor_set(&layout).unwrap();
    assert!(set.has_gpu_backing());
    device
        .write_descriptors(
            &set,
            &[
                DescriptorWrite {
                    dst_set: set.clone(),
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Texture(sampled_view, None)],
                },
                DescriptorWrite {
                    dst_set: set.clone(),
                    dst_binding: 1,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Sampler(sampler)],
                },
            ],
        )
        .expect("write texture descriptor");

    let module = device.create_shader_module(&ShaderModuleDesc {
        code: wgsl_to_spirv(SHADER),
        format: ShaderFormat::SpirV,
        entry_point: Some("vs_main".into()),
        name: Some("texture sample test".into()),
    });
    let frag = device.create_shader_module(&ShaderModuleDesc {
        code: wgsl_to_spirv(SHADER),
        format: ShaderFormat::SpirV,
        entry_point: Some("fs_main".into()),
        name: Some("texture sample test".into()),
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
            &[&layout],
        )
        .expect("graphics pipeline");
    assert!(pipeline.has_gpu_backing());

    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
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
            &[&set],
            None,
            Some((&index_buffer, 0)),
            rhi::IndexType::U16,
            4,
            0,
            6,
            0,
            0,
            &[rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0)],
        )
        .expect("textured draw framebuffer");

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

    // naga's SPIR-V output flips the sampled V axis relative to the upload
    // buffer, so the framebuffer top row (v=0) sees the last uploaded row.
    let quadrants = [
        // (screen top-left,   upload row index 2)
        (EXTENT / 4, EXTENT / 4, [0, 0, 255, 255]),
        // (screen top-right,  upload row index 3)
        (3 * EXTENT / 4, EXTENT / 4, [255, 255, 255, 255]),
        // (screen bottom-left, upload row index 0)
        (EXTENT / 4, 3 * EXTENT / 4, [255, 0, 0, 255]),
        // (screen bottom-right, upload row index 1)
        (3 * EXTENT / 4, 3 * EXTENT / 4, [0, 255, 0, 255]),
    ];
    for (x, y, expected) in quadrants {
        let p = pixel(x, y);
        assert_eq!(
            p, expected,
            "expected {expected:?} at ({x},{y}) from sampling the 2x2 texel texture, got {p:?}"
        );
    }
}