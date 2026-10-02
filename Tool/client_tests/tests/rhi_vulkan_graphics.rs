use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
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
@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-0.5, -0.5),
        vec2<f32>( 0.5, -0.5),
        vec2<f32>( 0.0,  0.5),
    );
    return vec4<f32>(pos[vid], 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 1.0, 0.0, 1.0);
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
fn vulkan_offscreen_graphics_roundtrip() {
    let device = open_vulkan_device(Features::default());

    const EXTENT: u32 = 64;
    let texture = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        rhi::Format::RGBA8_UNORM,
        TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
        1,
    );
    assert!(texture.has_gpu_backing());

    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("color attachment view");
    assert!(view.has_gpu_backing());

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
    assert!(render_pass.has_gpu_backing());

    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![FramebufferAttachment {
                texture_view: view,
                layer: 0,
                mip_level: 0,
            }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");
    assert!(framebuffer.has_gpu_backing());

    let vs = device.create_shader_module(&ShaderModuleDesc {
        code: wgsl_to_spirv(SHADER),
        format: ShaderFormat::SpirV,
        entry_point: Some("vs_main".into()),
        name: Some("graphics test".into()),
    });
    assert!(vs.has_gpu_backing());
    let fs = device.create_shader_module(&ShaderModuleDesc {
        code: wgsl_to_spirv(SHADER),
        format: ShaderFormat::SpirV,
        entry_point: Some("fs_main".into()),
        name: Some("graphics test".into()),
    });
    assert!(fs.has_gpu_backing());

    let pipeline = device
        .create_gpu_graphics_pipeline(
            &GraphicsPipelineDesc {
                shader_stages: vec![
                    PipelineShaderStage {
                        stage: ShaderStage::VERTEX,
                        module: vs,
                        entry_point: "vs_main".into(),
                    },
                    PipelineShaderStage {
                        stage: ShaderStage::FRAGMENT,
                        module: fs,
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
            &[],
        )
        .expect("graphics pipeline");
    assert!(pipeline.has_gpu_backing());

    device
        .draw_framebuffer(
            &framebuffer,
            &pipeline,
            &[],
            None,
            3,
            0,
            &[rhi::ClearValue::color(0.0, 0.0, 1.0, 1.0)],
        )
        .expect("draw framebuffer");

    let bytes = device.download_texture(&texture, 0, 0).expect("readback");
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
        "expected the triangle to paint the center green, got {center:?}"
    );

    let corner = pixel(0, 0);
    assert!(
        corner[2] > 200 && corner[0] < 50 && corner[1] < 50,
        "expected the clear color (blue) in the corner, got {corner:?}"
    );
}
