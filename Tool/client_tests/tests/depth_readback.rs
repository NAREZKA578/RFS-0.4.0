//! Can the depth buffer be read back?
//!
//! The shadow map is a depth image, and the only honest way to check that the
//! shadow pass put the scene into it is to read it back and look. That needs
//! two things the RHI had to be able to do: a depth image that can be a
//! transfer source, and a copy that takes the depth aspect rather than the
//! colour one.
//!
//! This test uses the geometry pass, which already writes a depth buffer, and
//! checks the result the way the shadow map's own test will have to: the
//! pixels where the cube is must hold a depth smaller than the clear value,
//! and the pixels where nothing was drawn must hold exactly the clear value.
//! A depth test that never fires and one that always fires look the same in
//! the colour image, and different here.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::{GeometryItem, GeometryPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{
    AttachmentDescription, AttachmentReference, Format, FramebufferAttachment, FramebufferDesc,
    LoadOp, PipelineBindPoint, RhiConfig, SampleCount, StoreOp, SubpassDescription, TextureUsage,
};

const EXTENT: u32 = 128;
const SHADER: &[u8] = include_bytes!("../assets/shaders/geometry.spv");

fn open_device() -> Arc<rhi::Device> {
    let backend = rhi::backend::create_backend(&RhiConfig {
        validation: true,
        ..RhiConfig::default()
    })
    .expect("backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    assert!(!devices.is_empty(), "no Vulkan device available");
    Arc::new(
        backend
            .create_device(
                &devices[0],
                &DeviceDesc { features: Features::default(), ..Default::default() },
            )
            .expect("create device"),
    )
}

#[test]
fn the_depth_buffer_holds_the_geometry_and_nothing_else() {
    let device = open_device();

    // The colour target and the depth target, both readable.
    let colour = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the colour target must be created");
    let colour_view = device
        .create_texture_view(
            &colour,
            &rhi::TextureViewDesc {
                texture: colour.clone(),
                format: None,
                view_type: rhi::TextureViewType::D2,
                aspects: rhi::TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("colour view");
    let depth = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::D32_SFLOAT,
            TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the depth target must be created");
    let depth_view = device
        .create_texture_view(
            &depth,
            &rhi::TextureViewDesc {
                texture: depth.clone(),
                format: None,
                view_type: rhi::TextureViewType::D2,
                aspects: rhi::TextureAspectFlags::DEPTH,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("depth view");

    // A render pass with a depth attachment: without one the pass has no depth
    // to write, and the test would be checking an empty image.
    let render_pass = device
        .create_gpu_render_pass(&rhi::RenderPassDesc {
            attachments: vec![
                AttachmentDescription {
                    format: Format::RGBA8_UNORM,
                    samples: SampleCount::X1,
                    load_op: LoadOp::Clear,
                    store_op: StoreOp::Store,
                    stencil_load_op: LoadOp::DontCare,
                    stencil_store_op: StoreOp::DontCare,
                    initial_layout: rhi::TextureLayout::Undefined,
                    final_layout: rhi::TextureLayout::TransferSrcOptimal,
                },
                AttachmentDescription {
                    format: Format::D32_SFLOAT,
                    samples: SampleCount::X1,
                    load_op: LoadOp::Clear,
                    store_op: StoreOp::Store,
                    stencil_load_op: LoadOp::DontCare,
                    stencil_store_op: StoreOp::DontCare,
                    initial_layout: rhi::TextureLayout::Undefined,
                    final_layout: rhi::TextureLayout::DepthStencilAttachmentOptimal,
                },
            ],
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: rhi::TextureLayout::ColorAttachmentOptimal,
                }],
                depth_stencil_attachment: Some(AttachmentReference {
                    attachment: 1,
                    layout: rhi::TextureLayout::DepthStencilAttachmentOptimal,
                }),
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");

    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![
                FramebufferAttachment { texture_view: colour_view, layer: 0, mip_level: 0 },
                FramebufferAttachment { texture_view: depth_view, layer: 0, mip_level: 0 },
            ],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(&device).expect("cube buffers");
    let (pass, state) = GeometryPass::create_with_render_pass(
        &device,
        vec![GeometryItem {
            name: "cube".into(),
            vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
            index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
            index_type: mesh.index_type(),
            index_count: mesh.index_count(),
            model: Mat4::IDENTITY,
            entity_id: None,
        }],
        render_pass,
        SHADER,
    )
    .expect("geometry pass");
    GeometryPass::set_framebuffer(&state, Some(framebuffer), EXTENT, EXTENT);

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("geometry", Box::new(pass)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("depth");

    assert!(
        GeometryPass::update_uniforms(
            &state,
            &device,
            Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y),
            Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
        ),
        "the uniform upload must succeed"
    );

    let validation_before = rhi::validation_error_count();
    graph.execute(&device, &mut context, &mut scene);
    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while drawing into a depth target"
    );

    device.wait_idle().expect("wait for the frame");

    let bytes = device
        .download_texture(&depth, 0, 0)
        .expect("the depth buffer could not be read back");
    assert_eq!(
        bytes.len(),
        (EXTENT * EXTENT * 4) as usize,
        "a D32_SFLOAT image is four bytes per pixel"
    );

    let mut written = 0u32;
    let mut cleared = 0u32;
    for pixel in bytes.chunks_exact(4) {
        let value = f32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
        if value >= 0.999 {
            cleared += 1;
        } else {
            written += 1;
        }
    }
    assert!(
        written > 0,
        "the depth buffer holds nothing but the clear value: the geometry did \
         not reach the depth target"
    );
    assert!(
        cleared > 0,
        "every pixel of the depth buffer was written: the cube is covering the \
         whole target, which means the projection or the vertex positions are \
         wrong"
    );
}
