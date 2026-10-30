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

fn open_vulkan_device(features: Features) -> Option<Device> {
    let backend = VulkanBackend::new(&RhiConfig::default()).ok()?;
    let devices = backend.enumerate_physical_devices().ok()?;
    let desc = DeviceDesc {
        features,
        ..Default::default()
    };
    backend.create_device(devices.first()?, &desc).ok()
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
    // Bug №187: graceful skip instead of a panic — a machine without a Vulkan
    // loader/device falls out of the test as a skip, not as a red unit.
    let Some(device) = open_vulkan_device(Features::default()) else {
        eprintln!("skipped: no Vulkan backend/device on this machine");
        return;
    };

    const EXTENT: u32 = 64;
    let texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            rhi::Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the colour target must be created");
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

    let vs = device
        .create_shader_module(&ShaderModuleDesc {
            code: wgsl_to_spirv(SHADER),
            format: ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("graphics test".into()),
        })
        .expect("the vertex module must be created");
    assert!(vs.has_gpu_backing());
    let fs = device
        .create_shader_module(&ShaderModuleDesc {
            code: wgsl_to_spirv(SHADER),
            format: ShaderFormat::SpirV,
            entry_point: Some("fs_main".into()),
            name: Some("graphics test".into()),
        })
        .expect("the fragment module must be created");
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

/// Bug №274: the submit fence used to be created and destroyed inside
/// `submit_recorded`, once per frame. This is the regression test for turning
/// that into a single device-owned fence.
///
/// A single submit cannot tell you whether the fence is being *reset* correctly,
/// because a freshly created fence starts unsignalled, so the wait after the
/// first submit is a real block whether or not the reset exists. What a repeated
/// submit can catch is the failure mode the reuse introduces: if the fence were
/// not reset, it would still be signalled from the previous frame, the wait
/// would return immediately, and the second frame would be recorded into a
/// command buffer the GPU was still reading — which shows up as the second
/// frame's pixels being wrong or the readback racing.
///
/// So each iteration draws a *different* colour and then checks the texture
/// actually contains that iteration's colour. A stale or unreached frame shows up
/// as the previous colour still being there.
#[test]
fn repeated_submits_do_not_reuse_a_previously_signalled_fence() {
    let Some(device) = open_vulkan_device(Features::default()) else {
        eprintln!("skipped: no Vulkan backend/device on this machine");
        return;
    };

    const EXTENT: u32 = 64;
    const FRAMES: u32 = 8;

    let texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            rhi::Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the frame target must be created");
    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("color attachment view");

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
                texture_view: view,
                layer: 0,
                mip_level: 0,
            }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    let vs = device
        .create_shader_module(&ShaderModuleDesc {
            code: wgsl_to_spirv(SHADER),
            format: ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("repeated submit".into()),
        })
        .expect("the vertex module must be created");
    let fs = device
        .create_shader_module(&ShaderModuleDesc {
            code: wgsl_to_spirv(SHADER),
            format: ShaderFormat::SpirV,
            entry_point: Some("fs_main".into()),
            name: Some("repeated submit".into()),
        })
        .expect("the fragment module must be created");

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
                // The colour blend state has to declare the render pass's one
                // colour attachment, even with the blending off: a pipeline
                // that declares none against a render pass that has one is
                // invalid, and an invalid pipeline draws nothing while
                // `vkCreateGraphicsPipelines` reports success. The test used
                // to leave the state out, and it still passed - because what
                // it checks is the *clear*, and the clear is the framebuffer's
                // business, not the pipeline's.
                color_blend_state: Some(rhi::ColorBlendState {
                    attachments: vec![rhi::ColorBlendAttachment {
                        blend_enable: false,
                        color_write_mask: rhi::ColorComponentFlags::R
                            | rhi::ColorComponentFlags::G
                            | rhi::ColorComponentFlags::B
                            | rhi::ColorComponentFlags::A,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            &render_pass,
            &[],
        )
        .expect("graphics pipeline");

    // Every frame clears to its own colour and draws the same triangle over it,
    // so the clear is the part that has to change. If a frame's submit were lost
    // or raced, the texture would still hold an earlier frame's colour.
    for frame in 0..FRAMES {
        let level = (frame + 1) as f32 / FRAMES as f32;
        let clear = rhi::ClearValue::color(level, 0.0, 0.0, 1.0);

        device
            .draw_framebuffer(&framebuffer, &pipeline, &[], None, 3, 0, &[clear])
            .unwrap_or_else(|e| panic!("submit for frame {frame} failed: {e}"));

        let bytes = device
            .download_texture(&texture, 0, 0)
            .unwrap_or_else(|e| panic!("readback for frame {frame} failed: {e}"));
        assert_eq!(bytes.len(), (EXTENT * EXTENT * 4) as usize);

        // A corner is outside the triangle, so it shows the clear value and not
        // the shader output.
        let offset = 0usize;
        let red = bytes[offset];
        let expected = (level * 255.0).round() as u8;
        assert!(
            (red as i32 - expected as i32).abs() <= 2,
            "frame {frame} should have cleared to red ~= {expected}, found {red}; \
             a stale fence or a lost submit would leave an earlier frame's colour"
        );
    }
}

/// Bug №692: a pipeline whose colour blend state declares a different number
/// of attachments than the render pass has is invalid, and Vulkan does not
/// refuse it: `vkCreateGraphicsPipelines` returns it as a success. The
/// pipeline then exists, is invalid, and draws nothing — a dark frame with no
/// message anywhere.
///
/// The RHI has to refuse it instead. The test found a pipeline of its own that
/// was invalid in exactly this way and still passed, because what it checked
/// was the *clear* — and the clear is the framebuffer's business, not the
/// pipeline's.
#[test]
fn a_pipeline_with_the_wrong_attachment_count_is_refused() {
    let Some(device) = open_vulkan_device(Features::default()) else {
        eprintln!("skipped: no Vulkan backend/device on this machine");
        return;
    };

    const EXTENT: u32 = 64;
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

    let vs = device
        .create_shader_module(&ShaderModuleDesc {
            code: wgsl_to_spirv(SHADER),
            format: ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("mismatch".into()),
        })
        .expect("the vertex module must be created");
    let fs = device
        .create_shader_module(&ShaderModuleDesc {
            code: wgsl_to_spirv(SHADER),
            format: ShaderFormat::SpirV,
            entry_point: Some("fs_main".into()),
            name: Some("mismatch".into()),
        })
        .expect("the fragment module must be created");

    // No colour blend state at all: zero attachments against a render pass with
    // one.
    let result = device.create_gpu_graphics_pipeline(
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
            ..Default::default()
        },
        &render_pass,
        &[],
    );

    assert!(
        result.is_err(),
        "a pipeline that declares no colour attachment against a render pass \
         with one is invalid and draws nothing; the RHI has to refuse it rather \
         than hand back a pipeline that will not draw"
    );
    let message = result.err().expect("an error").to_string();
    assert!(
        message.contains("colour attachment"),
        "the refusal must name what did not match, got: {message}"
    );
}
