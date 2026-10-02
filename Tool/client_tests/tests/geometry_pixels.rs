//! Does the geometry pass actually put pixels in the framebuffer?
//!
//! Recording commands correctly is not the same as drawing: the pipeline, the
//! vertex layout, the descriptor set and the rasteriser state can all be wrong
//! in ways that record perfectly and produce an untouched framebuffer. This
//! test submits the real thing to an offscreen target and reads the pixels
//! back, so the answer is observed rather than inferred.
//!
//! It is separate from the command-list test on purpose — that one asserts
//! which commands were emitted, this one asserts what came out.

use std::sync::Arc;

use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::{GeometryItem, GeometryPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::Features;
use rhi::{
    AttachmentDescription, AttachmentReference, Format, FramebufferAttachment, FramebufferDesc,
    LoadOp, PipelineBindPoint, RhiConfig, SampleCount, StoreOp, SubpassDescription, TextureUsage,
};

const EXTENT: u32 = 256;
const SHADER: &[u8] = include_bytes!("../assets/shaders/geometry.spv");

/// The colour the pass clears to, in the same order `ClearValue::color` takes.
const CLEAR: [f32; 3] = [0.05, 0.06, 0.08];

fn open_device() -> Arc<rhi::Device> {
    let backend = rhi::backend::create_backend(&RhiConfig::default()).expect("backend");
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
fn a_geometry_framebuffer_accepts_a_device_created_depth_view() {
    let device = open_device();

    // The bug this guards: the depth view was built with
    // `Texture::create_view`, which returns a CPU-side handle with no GPU
    // backing. Nothing complained at the point of the mistake — the frame that
    // used it failed with "framebuffer attachment has no GPU backing", reported
    // from the draw rather than from the creation.
    let depth = device.create_texture(
        64,
        64,
        1,
        Format::D32_SFLOAT,
        TextureUsage::DEPTH_STENCIL_ATTACHMENT,
        1,
    );
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
    assert!(
        depth_view.has_gpu_backing(),
        "a depth view created through the device must have GPU backing"
    );

    // And the render pass `create` builds must actually declare depth, since
    // the pipeline's depth state is derived from it.
    let (_pass, state) = GeometryPass::create(
        &device,
        Vec::new(),
        Format::RGBA8_UNORM,
        SHADER,
    )
    .expect("geometry pass");
    let render_pass = state.lock().expect("state lock").render_pass.clone();
    assert!(
        render_pass
            .desc()
            .subpasses
            .first()
            .is_some_and(|s| s.depth_stencil_attachment.is_some()),
        "the presentation render pass must declare a depth attachment"
    );
}

#[test]
fn the_geometry_pass_actually_lights_pixels() {
    let device = open_device();

    // A readable render pass: a presentation pass ends in `PresentSrc`, which
    // cannot be copied out, so it cannot be used to check what was drawn.
    let render_pass = device
        .create_gpu_render_pass(&rhi::RenderPassDesc {
            attachments: vec![AttachmentDescription {
                format: Format::RGBA8_UNORM,
                samples: SampleCount::X1,
                load_op: LoadOp::Clear,
                store_op: StoreOp::Store,
                stencil_load_op: LoadOp::DontCare,
                stencil_store_op: StoreOp::DontCare,
                initial_layout: rhi::TextureLayout::Undefined,
                final_layout: rhi::TextureLayout::TransferSrcOptimal,
            }],
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: rhi::TextureLayout::ColorAttachmentOptimal,
                }],
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");

    let texture = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        Format::RGBA8_UNORM,
        TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
        1,
    );
    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("colour attachment view");
    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![FramebufferAttachment { texture_view: view, layer: 0, mip_level: 0 }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(&device);
    let (pass, state) = GeometryPass::create_with_render_pass(
        &device,
        vec![GeometryItem {
            name: "cube".into(),
            vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
            index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
            index_type: mesh.index_type(),
            index_count: mesh.index_count(),
            model: glam::Mat4::IDENTITY,
        }],
        render_pass,
        SHADER,
    )
    .expect("geometry pass");

    // Pinned to the offscreen target, and sized to match it.
    GeometryPass::set_framebuffer(&state, Some(framebuffer), EXTENT, EXTENT);

    // Straight down the barrel at the cube, so the geometry must be inside the
    // frustum. A camera pointed elsewhere would produce a correct, empty frame.
    GeometryPass::update_uniforms(
        &state,
        &device,
        glam::Mat4::look_at_rh(
            glam::Vec3::new(0.0, 0.0, 3.0),
            glam::Vec3::ZERO,
            glam::Vec3::Y,
        ),
        glam::Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode {
        name: "geometry".into(),
        pass: Box::new(pass),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });

    let mut scene = Scene::new("pixels");
    let mut context = RenderContext::headless(device.clone());
    graph.execute(&device, &mut context, &mut scene);

    let bytes = device.download_texture(&texture, 0, 0).expect("readback");
    assert_eq!(
        bytes.len(),
        (EXTENT * EXTENT * 4) as usize,
        "the readback is not one RGBA pixel per texel"
    );

    // Count pixels that are neither the clear colour nor black. A frame that is
    // uniformly the clear colour means nothing was rasterised; a frame that is
    // uniformly black means the shader wrote nothing.
    let mut clear_pixels = 0u32;
    let mut black_pixels = 0u32;
    let mut lit_pixels = 0u32;
    for px in bytes.chunks_exact(4) {
        let rgb = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
        if rgb.iter().all(|c| *c < 0.02) {
            black_pixels += 1;
        } else if rgb
            .iter()
            .zip(CLEAR)
            .all(|(actual, clear)| (actual - clear).abs() < 0.02)
        {
            clear_pixels += 1;
        } else {
            lit_pixels += 1;
        }
    }
    let total = EXTENT * EXTENT;
    println!(
        "lit={lit_pixels} clear={clear_pixels} black={black_pixels} of {total}"
    );

    assert_ne!(
        black_pixels, total,
        "every pixel is black: the render pass never ran on the target"
    );
    assert!(
        lit_pixels > 0,
        "no pixel is lit: {clear_pixels} are the clear colour and {black_pixels} are black, \
         so the pass recorded but rasterised nothing"
    );
    // The cube is small in frame, so most of the image is still the clear
    // colour. A pass that covered everything would mean the cube filled the
    // screen, which is not what a 1-unit cube at 3 units with a 90-degree
    // vertical field of view looks like.
    assert!(
        lit_pixels < total / 2,
        "{lit_pixels} of {total} pixels are lit: the cube is covering the whole target, \
         which suggests the projection or the vertex positions are wrong"
    );
}
