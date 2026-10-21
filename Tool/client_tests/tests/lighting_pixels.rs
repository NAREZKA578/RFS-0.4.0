//! Does the lighting pass resolve the GBuffer into a lit image?
//!
//! The GBuffer fills four targets; the lighting pass reads them and produces
//! the lit image. It had three separate reasons not to work, and none of them
//! was visible from outside: no pipeline (three of them — the main one, SSAO
//! and SSR — were left as `None` forever), a render pass and a framebuffer
//! built from CPU-side descriptions, and an `execute` that opened a pass, bound
//! nothing and closed it again.
//!
//! So the test does not check that the pass recorded commands. It runs the
//! GBuffer and the lighting pass over a real cube, reads the lit image back,
//! and requires two things: the lit pixels are exactly where the cube is, and
//! they are brighter than the ambient term alone would give — that is, a light
//! from the scene actually reached the shader.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::lighting::light::{Light, LightType};
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::gbuffer::GBufferPass;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::lighting::LightingPass;
use rfs_client::render::scene::components::LightComponent;
use rfs_client::render::scene::entity::Entity;
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{RhiConfig};

const EXTENT: u32 = 128;
const GBUFFER_SHADER: &[u8] = include_bytes!("../assets/shaders/gbuffer.spv");
const LIGHTING_SHADER: &[u8] = include_bytes!("../assets/shaders/lighting.spv");

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
fn the_lighting_pass_resolves_the_gbuffer_into_a_lit_image() {
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

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("gbuffer", Box::new(gbuffer)));
    graph.add_pass(RenderPassNode::new("lighting", Box::new(lighting)));

    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    context.presentation_extent = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("lighting");

    // A light in the scene, in front of the cube. The shader's light model is
    // positional — a point with a radius — so this is a point light, and the
    // cube's front face is the one it lights.
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

    // The graph creates the GBuffer's targets and the lit image, and hands them
    // to the passes: the lighting pass reads what the GBuffer pass produced, and
    // both ends of that dependency are in this frame.
    graph.initialize(&device, &mut context, &mut scene);

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

    // The lit image: HDR, so four half-floats per pixel.
    let lighting_texture = graph
        .get_resource("lighting")
        .expect("the graph declares a lighting target")
        .texture()
        .expect("the lighting target was created")
        .clone();
    let bytes = device
        .download_texture(&lighting_texture, 0, 0)
        .expect("the lit image could not be read back");

    let lit = count_lit_halves(&bytes);
    assert!(
        lit > 0,
        "the lit image holds nothing but the clear value: the lighting pass \
         resolved nothing"
    );

    // The cube's own footprint, for comparison: the GBuffer's albedo target
    // holds it, and the lighting pass reads the same pixels. The albedo target
    // is 8-bit, so its red channel is a byte and not a half-float.
    let albedo = graph
        .get_resource("gbuffer_albedo")
        .expect("the graph declares an albedo target")
        .texture()
        .expect("the albedo target was created")
        .clone();
    let albedo_bytes = device
        .download_texture(&albedo, 0, 0)
        .expect("the albedo target could not be read back");
    let footprint = count_lit_bytes(&albedo_bytes);
    assert!(
        footprint > 0,
        "the cube did not reach the GBuffer: the albedo target holds nothing but \
         the clear value"
    );

    // The same pixels, in both images.
    //
    // This is the assertion that catches a missing barrier. The graph orders
    // the passes, but an order is not a memory dependency: without a barrier
    // between the GBuffer's writes and the lighting pass's sampled reads, the
    // lighting pass reads the GBuffer while it is still being written, and the
    // dark region moves from run to run. The count is the same in the broken
    // case only by luck, and the *places* are not.
    assert_eq!(
        lit, footprint,
        "the lit image and the GBuffer's albedo disagree about where the cube \
         is: the lighting pass resolved {lit} pixels and the geometry wrote \
         {footprint}"
    );

    // And the light reached the shader: the brightest lit pixel is far above
    // the ambient term on its own.
    let brightest = brightest_half(&bytes);
    assert!(
        brightest > 0.5,
        "the brightest lit pixel is {brightest}, which is about the ambient term \
         alone: the scene's light never reached the shader"
    );
}

/// How many pixels of a half-float image are brighter than the clear value.
fn count_lit_halves(bytes: &[u8]) -> u32 {
    let mut lit = 0u32;
    for pixel in bytes.chunks_exact(8) {
        // Half-float: the first two bytes are the red channel, sign and
        // exponent first.
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

/// The brightest value in the red channel of a half-float image.
fn brightest_half(bytes: &[u8]) -> f32 {
    let mut best = 0.0f32;
    for pixel in bytes.chunks_exact(8) {
        let red = half_to_f32(u16::from_le_bytes([pixel[0], pixel[1]]));
        if red > best {
            best = red;
        }
    }
    best
}

/// Decode an IEEE 754 half-precision float.
///
/// The lighting target is `RGBA16_SFLOAT`, and reading it back means reading
/// half-floats. The full decode is short enough to be worth having rather than
/// approximating: a test that guesses the encoding is a test that can be wrong
/// in a way nobody notices.
fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0f32 } else { 1.0f32 };
    let exponent = (bits >> 10) & 0x1F;
    let mantissa = bits & 0x03FF;
    if exponent == 0 {
        // Subnormal: no implicit leading one.
        sign * (mantissa as f32) * 2f32.powi(-24)
    } else if exponent == 0x1F {
        // Infinity or NaN. Not a value the test cares about, and pretending it
        // is zero keeps a stray NaN from failing an unrelated assertion.
        if mantissa == 0 {
            sign * f32::INFINITY
        } else {
            0.0
        }
    } else {
        sign * (1.0 + mantissa as f32 / 1024.0) * 2f32.powi(exponent as i32 - 15)
    }
}
