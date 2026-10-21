//! Does the GBuffer pass actually fill its four targets?
//!
//! The GBuffer is the first pass of the deferred chain: the geometry goes
//! into four targets and the lighting pass resolves them into a lit image.
//! For years the pass had no pipeline at all — `_pipeline: None` — and a
//! framebuffer built from CPU-side descriptions, so it cleared four textures
//! and drew nothing into them. Everything downstream read empty targets.
//!
//! So the test does not check that the pass recorded commands. It reads the
//! four targets back and requires the cube's footprint in each of them: the
//! world position, the normal, the albedo and the depth. A pass that clears
//! and draws nothing leaves all four exactly as it cleared them, and that is
//! the state this test exists to rule out.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::gbuffer::GBufferPass;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{Format, RhiConfig, TextureUsage};

const EXTENT: u32 = 128;
const GBUFFER_SHADER: &[u8] = include_bytes!("../assets/shaders/gbuffer.spv");

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

/// The bytes a cleared target holds, so "the cube was drawn here" can be told
/// from "this is the clear value".
///
/// The colour targets are cleared to **alpha zero** — the alpha channel is the
/// coverage mask: zero where no geometry landed, one where it did. So the
/// clear is four zeroes, and comparing against "all zero" would be right for
/// the colour targets and wrong for the depth one.
///
/// The float patterns are little-endian, which is what the GPU's memory holds:
/// 1.0f32 is the bit pattern 0x3F800000, and in memory that is the bytes
/// `00 00 80 3F`. Writing them the other way round compares against a value
/// that is not in memory, and then every pixel looks drawn.
fn clear_pattern(format: Format) -> Vec<u8> {
    match format {
        // 0.0, 0.0, 0.0, 0.0 as four f32.
        Format::RGBA32_SFLOAT => vec![
            0, 0, 0, 0, // r
            0, 0, 0, 0, // g
            0, 0, 0, 0, // b
            0, 0, 0, 0, // a = 0.0
        ],
        // 0.0, 0.0, 0.0, 0.0 as four unorm8.
        Format::RGBA8_UNORM => vec![0, 0, 0, 0],
        // 1.0 as one f32.
        Format::D32_SFLOAT => vec![0x00, 0x00, 0x80, 0x3F],
        other => panic!("no clear pattern for {other:?}"),
    }
}

/// How many pixels differ from the clear value.
fn drawn_pixels(bytes: &[u8], format: Format, extent: u32) -> u32 {
    let pattern = clear_pattern(format);
    let stride = pattern.len();
    let mut drawn = 0;
    for pixel in bytes.chunks_exact(stride) {
        if pixel != pattern.as_slice() {
            drawn += 1;
        }
    }
    let _ = extent;
    drawn
}

#[test]
fn the_gbuffer_pass_fills_its_four_targets_and_the_depth() {
    let device = open_device();

    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(&device).expect("cube buffers");

    let (pass, state) = GBufferPass::create(
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

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("gbuffer", Box::new(pass)));

    let mut context = RenderContext::headless(device.clone());
    // Both sizes have to be set. The context's resolution is a hard-coded
    // 1920×1080 until something says otherwise, and a headless context's
    // presentation extent is seeded from it — and the extent is the *first*
    // source the size rule consults, because it is the physical size.
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    context.presentation_extent = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("gbuffer");

    // The graph creates the targets - the pass asked for them by name and the
    // pass that produces them is in this frame - and hands them to the pass.
    graph.initialize(&device, &mut context, &mut scene);

    // The camera looks down the barrel at the cube, so the geometry is inside
    // the frustum. A camera pointed elsewhere would produce a correct, empty
    // frame.
    assert!(
        GBufferPass::update_uniforms(
            &state,
            &device,
            Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y),
            Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
        ),
        "the uniform upload must succeed before pixels are read back"
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

    // Every target the pass names, and the format it must be in.
    let targets: [(&str, Format); 5] = [
        ("gbuffer_position", Format::RGBA32_SFLOAT),
        ("gbuffer_normal", Format::RGBA32_SFLOAT),
        ("gbuffer_albedo", Format::RGBA8_UNORM),
        ("gbuffer_material", Format::RGBA8_UNORM),
        ("depth", Format::D32_SFLOAT),
    ];

    let mut footprint = 0u32;
    for (name, format) in targets {
        let texture = graph
            .get_resource(name)
            .unwrap_or_else(|| panic!("the graph declares {name:?}"))
            .texture()
            .unwrap_or_else(|| {
                panic!("{name:?} was never created: the graph creates a resource when a pass in the frame names it")
            })
            .clone();
        assert_eq!(
            texture.format(),
            format,
            "{name:?} has the wrong format: the render pass declares {format:?}"
        );
        assert!(
            texture.has_gpu_backing(),
            "{name:?} has no native handle, so the pass cannot have written to it"
        );
        let usage = texture.desc().usage;
        assert!(
            usage.contains(TextureUsage::COLOR_ATTACHMENT)
                || usage.contains(TextureUsage::DEPTH_STENCIL_ATTACHMENT),
            "{name:?} was not created as an attachment: {usage:?}"
        );

        let bytes = device
            .download_texture(&texture, 0, 0)
            .unwrap_or_else(|e| panic!("{name:?} could not be read back: {e}"));
        if std::env::var_os("GBUFFER_DUMP").is_some() {
            println!("--- {name:?} ({:?}) ---", format);
            for row in 0..3 {
                let start = (row * EXTENT as usize) * clear_pattern(format).len();
                println!("  row {row}: {:02x?}", &bytes[start..start + 16]);
            }
        }
        assert_eq!(
            bytes.len(),
            (EXTENT as usize) * (EXTENT as usize) * clear_pattern(format).len(),
            "{name:?} read back is not one pixel per texel"
        );

        let drawn = drawn_pixels(&bytes, format, EXTENT);
        assert!(
            drawn > 0,
            "{name:?} holds nothing but the clear value: the GBuffer pass cleared \
             four textures and drew nothing into them"
        );
        if footprint == 0 {
            footprint = drawn;
        } else {
            assert_eq!(
                drawn, footprint,
                "{name:?} has a different footprint from the other targets: the \
                 geometry did not land in all of them"
            );
        }
    }

    // The cube is small in frame, so most of each target is still the clear
    // value. A pass that covered everything would mean the cube filled the
    // screen, which is not what a one-unit cube at three units looks like.
    let total = EXTENT * EXTENT;
    assert!(
        footprint < total / 2,
        "{footprint} of {total} pixels are drawn: the cube is covering the whole \
         target, which suggests the projection or the vertex positions are wrong"
    );
}
