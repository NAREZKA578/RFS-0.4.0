//! Does the water pass draw a water surface?
//!
//! The pass used to be a `water_pipeline` that was `None` forever, a normal
//! map that was `None` forever, texture views built without the device, and a
//! drawing loop that was a literal placeholder. It opened a render pass, drew
//! nothing, and reported that it had recorded work.
//!
//! So the test draws a real surface: a plane, a reflection, a refraction, and
//! the waves' uniform. The water's own image is read back, and the surface has
//! to be there - lit by the refraction, and translucent, because the shader
//! writes an alpha of 0.85.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::water::{ReflectionQuality, WaterConfig, WaterPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{Format, RhiConfig, TextureUsage};

const SHADER: &[u8] = include_bytes!("../assets/shaders/water.spv");

fn open_device() -> Arc<rhi::Device> {
    let backend = rhi::backend::create_backend(&RhiConfig {
        validation: true,
        ..RhiConfig::default()
    })
    .expect("backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    Arc::new(
        backend
            .create_device(
                &devices[0],
                &DeviceDesc { features: Features::default(), ..Default::default() },
            )
            .expect("create device"),
    )
}

/// A flat colour image: the reflection or the refraction.
fn flat_texture(device: &rhi::Device, colour: [u8; 4]) -> rhi::Texture {
    let size = 16usize;
    let mut pixels = Vec::with_capacity(size * size * 4);
    for _ in 0..size * size {
        pixels.extend_from_slice(&colour);
    }
    let texture = device
        .create_texture(
            size as u32,
            size as u32,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
            1,
        )
        .expect("the flat colour texture must be created");
    device
        .upload_texture(&texture, &pixels)
        .expect("the texture's upload must succeed");
    texture
}

#[test]
fn the_water_pass_draws_a_water_surface() {
    let device = open_device();

    let mut mesh = Mesh::plane("water", 4.0, 4.0);
    mesh.create_buffers(&device).expect("plane buffers");

    let config = WaterConfig {
        reflection_quality: ReflectionQuality::Low,
        ..Default::default()
    };
    let (pass, state) = WaterPass::create(
        &device,
        vec![GeometryItem {
            name: "water".into(),
            vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
            index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
            index_type: mesh.index_type(),
            index_count: mesh.index_count(),
            model: Mat4::IDENTITY,
            entity_id: None,
        }],
        config,
        Format::RGBA8_UNORM,
        SHADER,
    )
    .expect("water pass");

    // The reflection and the refraction: the scene from the mirrored camera,
    // and the scene under the water.
    let reflection = flat_texture(&device, [255, 0, 0, 255]);
    let refraction = flat_texture(&device, [0, 0, 255, 255]);
    let reflection_view = device
        .create_texture_view(
            &reflection,
            &rhi::TextureViewDesc {
                texture: reflection.clone(),
                format: None,
                view_type: rhi::TextureViewType::D2,
                aspects: rhi::TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("reflection view");
    let refraction_view = device
        .create_texture_view(
            &refraction,
            &rhi::TextureViewDesc {
                texture: refraction.clone(),
                format: None,
                view_type: rhi::TextureViewType::D2,
                aspects: rhi::TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("refraction view");
    WaterPass::set_textures(&state, &device, Some(reflection_view), Some(refraction_view));

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("water", Box::new(pass)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(512, 512);
    let mut scene = Scene::new("water");

    assert!(
        WaterPass::update_uniforms(
            &state,
            &device,
            Mat4::look_at_rh(Vec3::new(0.0, 2.0, 3.0), Vec3::ZERO, Vec3::Y),
            Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
            0.0,
        ),
        "the uniform upload must succeed"
    );

    let validation_before = rhi::validation_error_count();
    graph.execute(&device, &mut context, &mut scene);
    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while drawing the water"
    );

    device.wait_idle().expect("wait for the frame");

    // The water's own image.
    let output = state
        .lock()
        .expect("state")
        .output_texture
        .clone()
        .expect("the water's image");
    let bytes = device
        .download_texture(&output, 0, 0)
        .expect("the water's image could not be read back");

    // The centre of the image, where the surface is.
    let size = 512usize;
    let centre = size / 2;
    let pixel = &bytes[(centre * size + centre) * 4..][..4];
    let (red, green, blue) = (
        pixel[0] as f32 / 255.0,
        pixel[1] as f32 / 255.0,
        pixel[2] as f32 / 255.0,
    );
    let alpha = pixel[3] as f32 / 255.0;

    assert!(
        blue > 0.3,
        "the centre of the water is (r {red}, g {green}, b {blue}): looking almost \
         straight down, the surface shows the refraction, which is blue"
    );
    assert!(
        (alpha - 0.85).abs() < 0.15,
        "the water's alpha is {alpha}: the shader writes 0.85, so the water is \
         translucent and the scene shows through it"
    );
}
