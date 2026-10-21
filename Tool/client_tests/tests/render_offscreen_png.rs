//! End-to-end offscreen render: shader -> rasterisation -> readback -> PNG.
//!
//! This is the milestone that produces an actual image. It exercises the whole
//! stack that a window would otherwise sit on top of:
//!
//!   committed .spv -> `create_shader_module` -> `create_gpu_graphics_pipeline`
//!   -> `draw_framebuffer` -> `download_texture` -> PNG on disk
//!
//! No vertex buffer is involved: the shader derives a fullscreen triangle from
//! `@builtin(vertex_index)`. That keeps the test to the parts that are new.
//!
//! Nothing here waits on a window or a swapchain, so it is deterministic and
//! runnable on a headless machine.

mod common;

use common::png::encode_rgba8;
// `PrimitiveTopology` is declared in both `rhi::types` and
// `rhi::pipeline::graphics`, so the glob re-exports make `rhi::PrimitiveTopology`
// ambiguous. Named explicitly, the same way the other Vulkan tests do it.
use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::pipeline::graphics::PrimitiveTopology;
use rhi::shader::module::{ShaderFormat, ShaderModuleDesc};
use rhi::types::{Features, ShaderStage};
use rhi::{
    AttachmentDescription, AttachmentReference, Backend, ClearValue, ColorBlendAttachment,
    ColorBlendState, ColorComponentFlags, ColorTargetDesc, Format, FramebufferAttachment,
    FramebufferDesc, GraphicsPipelineDesc, InputAssemblyState, LoadOp, MultisampleState,
    PipelineBindPoint, PipelineShaderStage, RasterizerState, RenderPassDesc, SampleCount,
    StoreOp, SubpassDescription, TextureLayout, TextureUsage,
};
use std::path::PathBuf;

const EXTENT: u32 = 128;

fn shader_spv() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("shaders")
        .join("fullscreen.spv");
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "missing {}: {e}\nRun: cargo test -p client_tests --test compile_shaders -- --ignored",
            path.display()
        )
    })
}

fn open_vulkan_device() -> Device {
    let config = RhiConfig::default();
    let backend = VulkanBackend::new(&config).expect("create Vulkan backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate devices");
    assert!(!devices.is_empty(), "no Vulkan device available");
    let desc = DeviceDesc { features: Features::default(), ..Default::default() };
    backend.create_device(&devices[0], &desc).expect("create device")
}

fn all_channels() -> ColorComponentFlags {
    ColorComponentFlags::R
        | ColorComponentFlags::G
        | ColorComponentFlags::B
        | ColorComponentFlags::A
}

#[test]
fn renders_a_frame_and_writes_a_png() {
    let device = open_vulkan_device();
    let spirv = shader_spv();

    let texture = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        Format::RGBA8_UNORM,
        TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
        1,
    );
    assert!(texture.has_gpu_backing(), "texture must be GPU backed");

    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("colour attachment view");

    let render_pass = device
        .create_gpu_render_pass(&RenderPassDesc {
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
            attachments: vec![FramebufferAttachment { texture_view: view, layer: 0, mip_level: 0 }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    // One module, two entry points.
    let module = device.create_shader_module(&ShaderModuleDesc {
        code: spirv,
        format: ShaderFormat::SpirV,
        entry_point: Some("vs_main".into()),
        name: Some("fullscreen".into()),
    });
    assert!(module.has_gpu_backing(), "shader module must be GPU backed");

    let pipeline = device
        .create_gpu_graphics_pipeline(
            &GraphicsPipelineDesc {
                shader_stages: vec![
                    PipelineShaderStage {
                        stage: ShaderStage::VERTEX,
                        module: module.clone(),
                        entry_point: "vs_main".into(),
                    },
                    PipelineShaderStage {
                        stage: ShaderStage::FRAGMENT,
                        module,
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

    // 3 vertices, no vertex buffer: the shader generates the triangle.
    // The 5th/6th arguments are `vertex_count` and `first_vertex` — there is
    // no instance count in this signature.
    device
        .draw_framebuffer(
            &framebuffer,
            &pipeline,
            &[],
            None,
            3,
            0,
            &[ClearValue::color(0.0, 1.0, 0.0, 1.0)],
        )
        .expect("draw framebuffer");

    let bytes = device.download_texture(&texture, 0, 0).expect("readback");
    assert_eq!(bytes.len(), (EXTENT * EXTENT * 4) as usize, "readback size");

    let pixel = |x: u32, y: u32| -> [u8; 4] {
        let o = ((y * EXTENT + x) * 4) as usize;
        [bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]
    };

    // The clear colour is green, the shader must have painted over the centre.
    // If the draw had not happened, the centre would still be green.
    let centre = pixel(EXTENT / 2, EXTENT / 2);
    assert!(
        centre[0] > 200 && centre[1] < 60 && centre[2] < 60,
        "centre should be the red disc, got {centre:?}"
    );

    // Outside the disc: red must stay 0, so this cannot be the clear colour.
    let corner = pixel(0, 0);
    assert!(
        corner[0] < 20 && corner[2] > 200,
        "corner should be the blue gradient, got {corner:?}"
    );

    // And a pixel at the bottom edge must differ from the top one, proving the
    // gradient is actually varying rather than a flat fill.
    let top = pixel(4, 4);
    let bottom = pixel(4, EXTENT - 5);
    assert!(
        top[2] != bottom[2],
        "gradient is flat: top {top:?} bottom {bottom:?}"
    );

    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("render_offscreen.png");
    let png = encode_rgba8(EXTENT, EXTENT, &bytes);
    std::fs::write(&out, &png).expect("write PNG");
    println!("wrote {} ({} bytes)", out.display(), png.len());
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A], "PNG signature");
}
