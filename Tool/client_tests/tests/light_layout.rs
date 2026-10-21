//! Do the shader and the writer agree on where a light's fields are?
//!
//! The light's layout is the kind of thing that is obvious once and then
//! forgotten. A `vec3` inside a struct is 16-aligned and takes 12 bytes, and
//! the member after it is aligned by **its own** alignment, not by the
//! struct's - so `position` is at 0, `color` at 16, `intensity` at 28 and
//! `radius` at 32. A writer that puts the two floats four bytes late makes
//! the shader read the intensity where the radius is, and the light's reach
//! collapses to a point: the light reaches a few clusters, the fragment
//! looks in a cluster the light is not in, and the scene comes out black
//! with no message anywhere.
//!
//! So the numbers are read back **out of the shader**: a light is written
//! into the buffer with known values, a shader that reads the buffer draws
//! those values into the colour, and the test checks that the colour holds
//! what was written. Nothing about the layout is assumed by the test - the
//! layout is what is being checked.

use std::sync::Arc;

use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::descriptor::layout::{DescriptorBinding, DescriptorSetLayoutDesc};
use rhi::pipeline::graphics::{
    ColorBlendAttachment, ColorBlendState, ColorTargetDesc, GraphicsPipelineDesc,
    PipelineShaderStage, PrimitiveTopology, RasterizerState,
};
use rhi::resource::texture::TextureViewDesc;
use rhi::shader::module::{ShaderFormat, ShaderModuleDesc};
use rhi::types::{Format, SampleCount, ShaderStage, TextureUsage};
use rhi::{
    AttachmentDescription, AttachmentReference, Backend, BufferUsage, ClearValue, DescriptorInfo,
    DescriptorType, FramebufferAttachment, FramebufferDesc, InputAssemblyState, LoadOp,
    MultisampleState, PipelineBindPoint, RenderPassDesc, StoreOp, SubpassDescription,
    TextureAspectFlags, TextureLayout, TextureViewType,
};

const EXTENT: u32 = 16;
const SHADER: &[u8] = include_bytes!("../assets/shaders/lightprobe.spv");

fn open_device() -> Device {
    let backend = VulkanBackend::new(&RhiConfig {
        validation: true,
        ..Default::default()
    })
    .unwrap();
    let devices = backend.enumerate_physical_devices().unwrap();
    backend
        .create_device(
            &devices[0],
            &DeviceDesc {
                features: Default::default(),
                ..Default::default()
            },
        )
        .unwrap()
}

/// The light, in the layout the scene's light buffer uses: the position at 0,
/// the colour at 16, the intensity at 28 and the radius at 32.
fn a_light() -> [f32; 12] {
    let mut light = [0f32; 12];
    // The position's x is 0.5, so that the shader's remap into [0, 1] keeps
    // it inside the range: a value above 1 would be clamped by the target and
    // the test would measure the clamp, not the layout.
    light[0] = 0.5;
    light[1] = 0.0;
    light[2] = 0.0;
    light[4] = 0.25;
    light[5] = 0.5;
    light[6] = 0.75;
    light[7] = 4.0;
    light[8] = 30.0;
    light
}

#[test]
fn the_shader_reads_the_light_where_it_was_written() {
    let device = open_device();

    // The buffer the shader reads: the clustering pass's own lights buffer,
    // because that is the one the forward shader reads. Reading a buffer this
    // test made itself would prove the layout and not the path: the forward
    // pass gets its lights from the clustering pass, and if that buffer is
    // not what the forward shader ends up reading, the numbers arrive wrong
    // with no message anywhere.
    let (_cluster, cluster_state) =
        rfs_client::render::passes::cluster::ClusterPass::create(
            &device,
            rfs_client::render::passes::cluster::ClusterConfig {
                tiles_x: 4,
                tiles_y: 4,
                depth_slices: 8,
            },
            include_bytes!("../assets/shaders/cluster.spv"),
        )
        .expect("cluster pass");
    let lights = {
        let state = cluster_state.lock().expect("state");
        state.lights_buffer.clone()
    };
    device
        .upload_buffer(&lights, &a_light())
        .expect("the light's upload must succeed");

    // The target the shader draws into.
    let target = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        Format::RGBA8_UNORM,
        TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
        1,
    );
    let view = device
        .create_texture_view(
            &target,
            &TextureViewDesc {
                texture: target.clone(),
                format: None,
                view_type: TextureViewType::D2,
                aspects: TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("colour view");

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

    // The descriptor: the lights' buffer.
    let layout = device.create_descriptor_set_layout(&DescriptorSetLayoutDesc {
        bindings: vec![DescriptorBinding {
            binding: 0,
            ty: DescriptorType::StorageBuffer,
            count: 1,
            stages: ShaderStage::FRAGMENT,
            immutable_samplers: None,
        }],
    });
    let set = device.create_descriptor_set(&layout).expect("descriptor set");
    device
        .write_descriptors(
            &set,
            &[rhi::DescriptorWrite {
                dst_set: set.clone(),
                dst_binding: 0,
                dst_array_element: 0,
                descriptors: vec![DescriptorInfo::Buffer(lights.clone(), 0, lights.size())],
            }],
        )
        .expect("the descriptor write must succeed");

    let module = device.create_shader_module(&ShaderModuleDesc {
        code: SHADER.to_vec(),
        format: ShaderFormat::SpirV,
        entry_point: Some("vs_main".into()),
        name: Some("light probe".into()),
    });
    assert!(module.has_gpu_backing());

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
                // No vertex buffer: the triangle comes from the vertex index.
                vertex_input_state: None,
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
                        color_write_mask: rhi::ColorComponentFlags::R
                            | rhi::ColorComponentFlags::G
                            | rhi::ColorComponentFlags::B
                            | rhi::ColorComponentFlags::A,
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

    device
        .draw_framebuffer(
            &framebuffer,
            &pipeline,
            &[&set],
            None,
            3,
            0,
            &[ClearValue::color(0.0, 0.0, 0.0, 1.0)],
        )
        .expect("the draw must succeed");
    device.wait_idle().expect("wait for the frame");

    let bytes = device
        .download_texture(&target, 0, 0)
        .expect("the target could not be read back");
    let pixel = &bytes[..4];
    let (red, green, blue, alpha) = (
        pixel[0] as f32 / 255.0,
        pixel[1] as f32 / 255.0,
        pixel[2] as f32 / 255.0,
        pixel[3] as f32 / 255.0,
    );

    // The position's x: 0.5, remapped to [0, 1] by the shader.
    assert!(
        (red - 0.75).abs() < 0.05,
        "the position's x came back as {red}: the shader read the light's \
         position at {}",
        red * 2.0 - 1.0
    );
    // The intensity: 4.0, scaled by 8.
    assert!(
        (green - 0.5).abs() < 0.05,
        "the intensity came back as {}: the shader read it at {}",
        green * 8.0,
        green * 8.0
    );
    // The radius: 30.0, scaled by 64.
    assert!(
        (blue - 30.0 / 64.0).abs() < 0.02,
        "the radius came back as {}: the shader read it at {}",
        blue * 64.0,
        blue * 64.0
    );
    // The colour's red: 0.25.
    assert!(
        (alpha - 0.25).abs() < 0.05,
        "the colour's red came back as {alpha}"
    );
}
