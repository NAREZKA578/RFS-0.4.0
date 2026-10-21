//! Does the resolve pass put the lit image on the screen?
//!
//! The deferred chain was complete and invisible: the GBuffer filled four
//! targets, the lighting pass resolved them into a lit image, and the lit image
//! sat in a texture that no pass read and no screen showed. A pipeline that
//! cannot be looked at is a pipeline nobody can check.
//!
//! So the test does not ask the pass whether it recorded commands — it asks
//! whether the *pixels* arrived. The chain is run over a real cube, the resolve
//! pass is pinned to an offscreen target (a presentation image ends in
//! `PresentSrc` and cannot be copied out), and the result is read back and
//! compared with the lit image the lighting pass produced.
//!
//! The comparison is not "the image is not empty". It is that the two images
//! agree about *where* the cube is, pixel for pixel. That is what catches the
//! failure this pass is for: a copy that lands in the wrong place, or a copy
//! that never happened and leaves the clear value behind, produces a different
//! picture even when both images are "not empty".

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::lighting::light::{Light, LightType};
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::gbuffer::GBufferPass;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::lighting::LightingPass;
use rfs_client::render::passes::resolve::ResolvePass;
use rfs_client::render::scene::components::LightComponent;
use rfs_client::render::scene::entity::Entity;
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{
    Format, FramebufferAttachment, FramebufferDesc, RhiConfig, TextureUsage,
};

const EXTENT: u32 = 128;
const GBUFFER_SHADER: &[u8] = include_bytes!("../assets/shaders/gbuffer.spv");
const LIGHTING_SHADER: &[u8] = include_bytes!("../assets/shaders/lighting.spv");
const RESOLVE_SHADER: &[u8] = include_bytes!("../assets/shaders/resolve.spv");

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
                &DeviceDesc {
                    features: Features::default(),
                    ..Default::default()
                },
            )
            .expect("create device"),
    )
}

#[test]
fn the_resolve_pass_copies_the_lit_image_where_it_belongs() {
    let device = open_device();

    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(&device).expect("cube buffers");

    let (gbuffer, gbuffer_state) = GBufferPass::create(
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
        GBUFFER_SHADER,
    )
    .expect("gbuffer pass");

    let (lighting, lighting_state) =
        LightingPass::create(&device, Default::default(), LIGHTING_SHADER).expect("lighting pass");

    // The resolve pass names both ends of its edge: the resource it copies and
    // the pass that produces it. Naming only the resource would copy a texture
    // nobody has written.
    let (resolve, resolve_state) = ResolvePass::create(
        &device,
        Format::RGBA8_UNORM,
        RESOLVE_SHADER,
        "lighting",
        "lighting",
    )
    .expect("resolve pass");

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("gbuffer", Box::new(gbuffer)));
    graph.add_pass(RenderPassNode::new("lighting", Box::new(lighting)));
    graph.add_pass(RenderPassNode::new("resolve", Box::new(resolve)));

    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    context.presentation_extent = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("resolve");

    let mut light_entity = Entity::new("sun");
    light_entity.add_component(rfs_client::render::scene::Transform::with_position(Vec3::new(
        0.0, 0.0, 5.0,
    )));
    light_entity.add_component(LightComponent::new(
        Light::new("sun", LightType::Point)
            .with_color(Vec3::ONE)
            .with_intensity(1.0)
            .with_radius(100.0),
    ));
    scene.add_entity(light_entity);

    // The graph creates the GBuffer's targets, the lit image, and hands them to
    // the passes.
    graph.initialize(&device, &mut context, &mut scene);

    // The offscreen target the copy lands in. The resolve pass's own render
    // pass is used for the framebuffer, because a framebuffer's attachments
    // must match its render pass's formats.
    let target = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        Format::RGBA8_UNORM,
        TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
        1,
    );
    let target_view = device
        .create_texture_view(&target, target.create_default_view().desc())
        .expect("colour attachment view");
    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: resolve_state
                .lock()
                .expect("resolve state")
                .render_pass
                .clone(),
            attachments: vec![FramebufferAttachment {
                texture_view: target_view,
                layer: 0,
                mip_level: 0,
            }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");
    ResolvePass::set_framebuffer(&resolve_state, Some(framebuffer), EXTENT, EXTENT);

    let camera = Vec3::new(0.0, 0.0, 3.0);
    let view = Mat4::look_at_rh(camera, Vec3::ZERO, Vec3::Y);
    let projection = Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0);

    assert!(
        GBufferPass::update_uniforms(&gbuffer_state, &device, view, projection),
        "the gbuffer's uniform upload must succeed"
    );
    let light_count = LightingPass::update_lights(&lighting_state, &device, &scene);
    assert_eq!(light_count, 1, "the scene's one light must reach the shader");
    assert!(
        LightingPass::update_uniforms(
            &lighting_state,
            &device,
            projection * view,
            camera,
            light_count as u32,
        ),
        "the lighting uniform upload must succeed"
    );

    let validation_before = rhi::validation_error_count();
    graph.execute(&device, &mut context, &mut scene);
    let validation_after = rhi::validation_error_count();
    assert_eq!(
        validation_after, validation_before,
        "the validator reported {} error(s) while producing a frame this test is \
         willing to call correct — correct output does not mean correct API use",
        validation_after - validation_before
    );

    device
        .wait_idle()
        .expect("wait for the submitted frame before reading it back");

    // The lit image the resolve pass was supposed to copy.
    let lighting_texture = graph
        .get_resource("lighting")
        .expect("the graph declares a lighting target")
        .texture()
        .expect("the lighting target was created")
        .clone();
    let lit_bytes = device
        .download_texture(&lighting_texture, 0, 0)
        .expect("the lit image could not be read back");

    // And what actually landed on the screen.
    let copied_bytes = device
        .download_texture(&target, 0, 0)
        .expect("the copy could not be read back");

    let lit = count_lit_halves(&lit_bytes);
    assert!(
        lit > 0,
        "the lit image holds nothing but the clear value: there is nothing for \
         the resolve pass to copy"
    );

    let copied = count_lit_bytes(&copied_bytes);
    assert!(
        copied > 0,
        "the copy is empty: the resolve pass recorded its commands and produced \
         a cleared image"
    );

    assert_eq!(
        copied, lit,
        "the copy and the lit image disagree about how many pixels the cube \
         covers: the lit image has {lit} and the copy has {copied}"
    );

    // The same pixels, not just the same number of them: a copy that was
    // shifted or flipped has the same count and a different picture.
    assert!(
        same_footprint(&lit_bytes, &copied_bytes),
        "the copy holds the cube in different places than the lit image: the \
         resolve pass copied the wrong pixels"
    );
}

/// How many pixels of a half-float image are brighter than the clear value.
fn count_lit_halves(bytes: &[u8]) -> u32 {
    let mut lit = 0u32;
    for pixel in bytes.chunks_exact(8) {
        let red = half_to_f32(u16::from_le_bytes([pixel[0], pixel[1]]));
        if red > 0.01 {
            lit += 1;
        }
    }
    lit
}

/// How many pixels of an 8-bit image are brighter than the clear value.
fn count_lit_bytes(bytes: &[u8]) -> u32 {
    let mut lit = 0u32;
    for pixel in bytes.chunks_exact(4) {
        if pixel[0] > 2 {
            lit += 1;
        }
    }
    lit
}

/// Whether both images have the cube in exactly the same places.
///
/// The two images have different formats — one is half-float, the other is
/// eight-bit — so the comparison is of *where* the pixels are lit, not of
/// their values. A half-float lit pixel and an eight-bit lit pixel are the
/// same pixel.
fn same_footprint(lit_bytes: &[u8], copied_bytes: &[u8]) -> bool {
    let lit_pixels = lit_bytes.chunks_exact(8).count();
    let copied_pixels = copied_bytes.chunks_exact(4).count();
    if lit_pixels != copied_pixels {
        return false;
    }
    for (lit, copied) in lit_bytes
        .chunks_exact(8)
        .zip(copied_bytes.chunks_exact(4))
    {
        let lit_red = half_to_f32(u16::from_le_bytes([lit[0], lit[1]])) > 0.01;
        let copied_red = copied[0] > 2;
        if lit_red != copied_red {
            return false;
        }
    }
    true
}

/// Decode an IEEE 754 half-precision float.
fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0f32 } else { 1.0f32 };
    let exponent = (bits >> 10) & 0x1F;
    let mantissa = bits & 0x03FF;
    if exponent == 0 {
        sign * (mantissa as f32) * 2f32.powi(-24)
    } else if exponent == 0x1F {
        if mantissa == 0 {
            sign * f32::INFINITY
        } else {
            0.0
        }
    } else {
        sign * (1.0 + mantissa as f32 / 1024.0) * 2f32.powi(exponent as i32 - 15)
    }
}
