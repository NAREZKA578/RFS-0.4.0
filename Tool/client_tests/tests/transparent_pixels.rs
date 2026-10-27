//! Does the transparent pass blend, or does it just draw?
//!
//! The pass used to be a `pipelines` map created empty and a loop over the
//! objects whose body said "Placeholder: actual rendering would go here". It
//! opened a render pass, drew nothing, and reported that it had recorded
//! work.
//!
//! So the test draws a half-transparent red quad and reads the result back.
//! The check that matters is the *blend*: with alpha blending the pixel is
//! the source's colour times its alpha over the cleared background, and
//! without blending it is the source's colour unchanged. Half-transparent red
//! over black is (0.5, 0, 0) with blending and (1, 0, 0) without, so the
//! test cannot be passed by a pass that draws but does not blend.

use std::sync::Arc;

use glam::{Mat4, Vec3, Vec4};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::meshes::{Mesh, Vertex};
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::transparent::{BlendMode, TransparentConfig, TransparentItem, TransparentPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{Format, FramebufferAttachment, FramebufferDesc, RhiConfig, TextureUsage};

const EXTENT: u32 = 64;
const SHADER: &[u8] = include_bytes!("../assets/shaders/transparent.spv");

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

/// A quad in the XY plane, half transparent and red.
fn quad(device: &rhi::Device) -> Mesh {
    let colour = Vec4::new(1.0, 0.0, 0.0, 0.5);
    let vertices = vec![
        Vertex::new(Vec3::new(-0.5, -0.5, 0.0)).with_color(colour),
        Vertex::new(Vec3::new(0.5, -0.5, 0.0)).with_color(colour),
        Vertex::new(Vec3::new(0.5, 0.5, 0.0)).with_color(colour),
        Vertex::new(Vec3::new(-0.5, 0.5, 0.0)).with_color(colour),
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];
    let mut mesh = Mesh::new("quad").with_vertices(vertices).with_indices(indices);
    mesh.create_buffers(device).expect("quad buffers");
    mesh
}

#[test]
fn the_transparent_pass_blends_what_it_draws() {
    let device = open_device();
    let mesh = quad(&device);

    let (pass, state) = TransparentPass::create(
        &device,
        vec![TransparentItem {
            item: GeometryItem {
                name: "quad".into(),
                vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
                index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
                index_type: mesh.index_type(),
                index_count: mesh.index_count(),
                model: Mat4::IDENTITY,
                entity_id: None,
            },
            blend_mode: BlendMode::Alpha,
        }],
        TransparentConfig::default(),
        Format::RGBA8_UNORM,
        SHADER,
    )
    .expect("transparent pass");

    // The offscreen target: a colour image and a depth image, both sized to
    // the frame. The depth is there because the pass's render pass has a depth
    // attachment, and a framebuffer's attachment count must match its render
    // pass's exactly.
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
            TextureUsage::DEPTH_STENCIL_ATTACHMENT,
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
    // The framebuffer has to be built with the pass's *own* render pass: a
    // framebuffer and a render pass that are not compatible with each other
    // is a validation error, and the pass's render pass is the one that
    // declares the depth format.
    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: state.lock().expect("state").render_pass.clone(),
            attachments: vec![
                FramebufferAttachment { texture_view: colour_view, layer: 0, mip_level: 0 },
                FramebufferAttachment { texture_view: depth_view, layer: 0, mip_level: 0 },
            ],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");
    TransparentPass::set_framebuffer(&state, Some(framebuffer), EXTENT, EXTENT);

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("transparent", Box::new(pass)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("transparent");

    assert!(
        TransparentPass::update_uniforms(
            &state,
            &device,
            Mat4::look_at_rh(Vec3::new(0.0, 0.0, 2.0), Vec3::ZERO, Vec3::Y),
            Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
        ),
        "the uniform upload must succeed"
    );

    let validation_before = rhi::validation_error_count();
    graph.execute(&device, &mut context, &mut scene);
    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while drawing a transparent object"
    );

    device.wait_idle().expect("wait for the frame");

    let bytes = device
        .download_texture(&colour, 0, 0)
        .expect("the target could not be read back");
    let centre = (EXTENT / 2) as usize;
    let pixel = &bytes[(centre * EXTENT as usize + centre) * 4..][..4];
    let red = pixel[0] as f32 / 255.0;
    let alpha = pixel[3] as f32 / 255.0;

    assert!(
        red > 0.3 && red < 0.7,
        "the centre pixel's red is {red}: half-transparent red over black is \
         about 0.5 with alpha blending and 1.0 without it, so the pass drew but \
         did not blend"
    );
    assert!(
        alpha > 0.3 && alpha < 0.7,
        "the centre pixel's alpha is {alpha}: the standard alpha blend keeps the \
         source's alpha, and the alpha factors must not be the colour's factors \
         (that squares the alpha)"
    );
}
