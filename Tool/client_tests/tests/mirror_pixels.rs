//! The mirror camera: the scene, as the water's reflection sees it.
//!
//! The water pass takes the reflection as an **input** — it draws the
//! surface, it does not render what is reflected in it. Until now the only
//! pictures the water was ever tested with were flat stand-in colours
//! (`water_pixels.rs`, `water_shared_depth.rs`), and the pass that renders
//! the scene from the mirrored camera — `MirrorPass` — was constructed
//! nowhere, which is what the reachability check red-flagged.
//!
//! This test wires the real chain: the clustering, the shadow map, the
//! **mirror** pass, the forward pass, and the water that samples the mirror's
//! reflection. The scene is built so that the mirroring is the only possible
//! cause of what the picture shows:
//!
//! - the water surface is the plane `y = 0` (the mirror's level),
//! - the cube floats above it, at `(0, 1.5, -3)`,
//! - the camera looks **down** at the water from `(0, 2, 3)`, and every
//!   corner of the cube sits at least 27° off that view's axis — outside the
//!   22.5° half-field of the 45° lens: the **main** camera cannot see the
//!   cube at all,
//! - the mirror camera looks at the world reflected about `y = 0`, and the
//!   cube's reflection sits about 7° off the same axis: inside the lens,
//!   near the centre of the reflection's image.
//!
//! So the reflection's image must contain the lit cube, and the main pass's
//! image must not contain it. If the mirror pass drew with the plain camera's
//! view instead of the mirrored one, its picture would equal the main pass's
//! — empty — and the first assertion would fail. And if the mirror pass were
//! skipped entirely, its target would hold nothing but the clear colour, and
//! the same assertion would fail again.
//!
//! The water is drawn with the mirror's reflection bound, so the chain runs
//! end to end: no stand-in texture anywhere in it.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::lighting::light_buffer::pack_light;
use rfs_client::render::meshes::{Mesh, Vertex};
use rfs_client::render::passes::cluster::{ClusterConfig, ClusterPass};
use rfs_client::render::passes::forward::ForwardPass;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::mirror::{MirrorConfig, MirrorPass};
use rfs_client::render::passes::shadow::{light_view_projection, ShadowConfig, ShadowPass};
use rfs_client::render::passes::water::{WaterConfig, WaterPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{
    Format, FramebufferAttachment, FramebufferDesc, RhiConfig, TextureAspectFlags, TextureUsage,
    TextureViewDesc, TextureViewType,
};

const MAIN_EXTENT: u32 = 64;
const MIRROR_EXTENT: u32 = 256;
const FORWARD_SHADER: &[u8] = include_bytes!("../assets/shaders/forward.spv");
const SHADOW_SHADER: &[u8] = include_bytes!("../assets/shaders/shadow.spv");
const CLUSTER_SHADER: &[u8] = include_bytes!("../assets/shaders/cluster.spv");
const WATER_SHADER: &[u8] = include_bytes!("../assets/shaders/water.spv");

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

/// The cube that floats above the water: size 0.8, centred at `(0, 1.5, -3)`.
fn cube_item(device: &rhi::Device) -> GeometryItem {
    let mut mesh = Mesh::cube("cube", 0.8);
    mesh.create_buffers(device).expect("cube buffers");
    GeometryItem {
        name: "cube".into(),
        vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
        index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
        index_type: mesh.index_type(),
        index_count: mesh.index_count(),
        model: Mat4::from_translation(Vec3::new(0.0, 1.5, -3.0)),
        entity_id: None,
    }
}

/// The water's surface: a quad in the XZ plane at `y`, facing up.
///
/// The water shader displaces `position.y` by the waves, so the surface it
/// expects is horizontal — not the XY quad the shared-depth test draws.
fn water_item(device: &rhi::Device) -> GeometryItem {
    let vertices = vec![
        Vertex::new(Vec3::new(-2.0, 0.0, -2.0)).with_normal(Vec3::Y),
        Vertex::new(Vec3::new(2.0, 0.0, -2.0)).with_normal(Vec3::Y),
        Vertex::new(Vec3::new(2.0, 0.0, 2.0)).with_normal(Vec3::Y),
        Vertex::new(Vec3::new(-2.0, 0.0, 2.0)).with_normal(Vec3::Y),
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];
    let mut mesh = Mesh::new("water").with_vertices(vertices).with_indices(indices);
    mesh.create_buffers(device).expect("water buffers");
    GeometryItem {
        name: "water".into(),
        vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
        index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
        index_type: mesh.index_type(),
        index_count: mesh.index_count(),
        model: Mat4::IDENTITY,
        entity_id: None,
    }
}

fn colour_view(device: &rhi::Device, texture: &rhi::Texture) -> rhi::TextureView {
    device
        .create_texture_view(
            texture,
            &TextureViewDesc {
                texture: texture.clone(),
                format: None,
                view_type: TextureViewType::D2,
                aspects: TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("colour view")
}

fn depth_view(device: &rhi::Device, texture: &rhi::Texture) -> rhi::TextureView {
    device
        .create_texture_view(
            texture,
            &TextureViewDesc {
                texture: texture.clone(),
                format: None,
                view_type: TextureViewType::D2,
                aspects: TextureAspectFlags::DEPTH,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("depth view")
}

/// How many pixels of the image are brighter than `threshold` in any channel.
fn bright_pixels(bytes: &[u8], threshold: f32) -> usize {
    bytes
        .chunks_exact(4)
        .filter(|pixel| {
            pixel[..3]
                .iter()
                .any(|channel| *channel as f32 / 255.0 > threshold)
        })
        .count()
}

#[test]
fn the_reflection_shows_the_scene_the_main_camera_cannot_see() {
    let device = open_device();

    let view = Mat4::look_at_rh(Vec3::new(0.0, 2.0, 3.0), Vec3::new(0.0, -1.0, 0.0), Vec3::Y);
    let projection = Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0);
    let ambient = Vec3::new(0.1, 0.1, 0.1);

    // The light sits near the cube, above the water's plane. The lighting is
    // computed at the geometry's real positions, so the reflection shows the
    // object's own lighting — the same light both cameras read.
    let light_position = Vec3::new(0.0, 2.5, -1.0);
    let light_space = light_view_projection(light_position, Vec3::new(0.0, 1.5, -3.0), 50.0);
    let lights = {
        let mut lights = vec![0.0f32; 12];
        pack_light(&mut lights, 0, light_position, Vec3::ONE, 8.0, 50.0);
        lights
    };

    // The shadow map: the cube's depth in the light's space, shared by both
    // cameras, because the light does not move when the camera is mirrored.
    let (shadow, shadow_state) = ShadowPass::create(
        &device,
        vec![cube_item(&device)],
        ShadowConfig {
            resolution: 128,
            cascade_count: 1,
            ..Default::default()
        },
        SHADOW_SHADER,
    )
    .expect("shadow pass");
    assert!(
        ShadowPass::update_light(&shadow_state, &device, &[light_space]),
        "the light's shadow uniform upload must succeed"
    );
    let shadow_map = ShadowPass::shadow_map(&shadow_state).expect("the shadow map");
    let shadow_map_view = depth_view(&device, &shadow_map);

    // The clustering of the main camera, and the light it bins.
    let cluster_config = ClusterConfig {
        tiles_x: 4,
        tiles_y: 4,
        depth_slices: 8,
    };
    let (cluster, cluster_state) =
        ClusterPass::create(&device, cluster_config, CLUSTER_SHADER).expect("cluster pass");
    {
        let state = cluster_state.lock().expect("state");
        device
            .upload_buffer(&state.lights_buffer, &lights)
            .expect("the light's upload must succeed");
    }

    // The mirror camera: the same opaque items the main pass draws, the same
    // light, and its own clustering, cut with the reflected view.
    let (mirror, mirror_state) = MirrorPass::create(
        &device,
        vec![cube_item(&device)],
        MirrorConfig {
            level: 0.0,
            width: MIRROR_EXTENT,
            height: MIRROR_EXTENT,
        },
        Format::RGBA8_UNORM,
        CLUSTER_SHADER,
        FORWARD_SHADER,
    )
    .expect("mirror pass");
    if let Some(mirror_lights) = MirrorPass::lights_buffer(&mirror_state) {
        device
            .upload_buffer(&mirror_lights, &lights)
            .expect("the mirror light's upload must succeed");
    } else {
        panic!("the mirror's lights buffer");
    }
    MirrorPass::set_shadow_maps(&mirror_state, vec![shadow_map_view.clone()]);

    // The main forward pass, into a target of its own.
    let (forward, forward_state) =
        ForwardPass::create(&device, vec![cube_item(&device)], Format::RGBA8_UNORM, FORWARD_SHADER)
            .expect("forward pass");
    ForwardPass::set_shadow_maps(&forward_state, vec![shadow_map_view.clone()]);
    {
        let state = cluster_state.lock().expect("state");
        ForwardPass::set_cluster_buffers(
            &forward_state,
            state.lights_buffer.clone(),
            state.cluster_indices_buffer.clone(),
            cluster_config,
        );
    }
    let colour = device
        .create_texture(
            MAIN_EXTENT,
            MAIN_EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the main colour target must be created");
    let main_depth = device
        .create_texture(
            MAIN_EXTENT,
            MAIN_EXTENT,
            1,
            Format::D32_SFLOAT,
            TextureUsage::DEPTH_STENCIL_ATTACHMENT,
            1,
        )
        .expect("the main depth");
    let forward_framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: forward_state.lock().expect("state").render_pass.clone(),
            attachments: vec![
                FramebufferAttachment {
                    texture_view: colour_view(&device, &colour),
                    layer: 0,
                    mip_level: 0,
                },
                FramebufferAttachment {
                    texture_view: depth_view(&device, &main_depth),
                    layer: 0,
                    mip_level: 0,
                },
            ],
            width: MAIN_EXTENT,
            height: MAIN_EXTENT,
            layers: 1,
        })
        .expect("the forward framebuffer");
    ForwardPass::set_framebuffer(&forward_state, Some(forward_framebuffer));

    // The uniforms, the same for both cameras: the mirror mirrors the view
    // itself, the projection passes through unchanged.
    assert!(
        ForwardPass::update_uniforms(
            &forward_state,
            &device,
            view,
            projection,
            &[light_space],
            &[100.0],
            1,
            0,
            0.1,
            100.0,
            ambient,
        ),
        "the forward uniform upload must succeed"
    );
    assert!(
        ClusterPass::update_uniforms(
            &cluster_state,
            &device,
            view,
            projection,
            1,
            0.1,
            100.0,
            MAIN_EXTENT,
            MAIN_EXTENT,
        ),
        "the cluster uniform upload must succeed"
    );
    assert!(
        MirrorPass::update_uniforms(
            &mirror_state,
            &device,
            view,
            projection,
            &[light_space],
            &[100.0],
            1,
            0,
            1,
            0.1,
            100.0,
            ambient,
        ),
        "the mirror uniform upload must succeed"
    );

    // The water, drawing with the mirror's reflection: no flat stand-in
    // anywhere in the chain. The refraction is not wired yet — the mirror
    // pass produces the reflection only — so the shader falls back to it.
    let (water_pass, water_state) = WaterPass::create(
        &device,
        vec![water_item(&device)],
        WaterConfig::default(),
        Format::RGBA8_UNORM,
        WATER_SHADER,
    )
    .expect("water pass");
    let reflection = MirrorPass::reflection_view(&mirror_state).expect("the mirror's reflection");
    WaterPass::set_textures(&water_state, &device, Some(reflection), None);
    assert!(
        WaterPass::update_uniforms(&water_state, &device, view, projection, 0.0),
        "the water uniform upload must succeed"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("cluster", Box::new(cluster)));
    graph.add_pass(RenderPassNode::new("shadow", Box::new(shadow)));
    graph.add_pass(RenderPassNode::new("mirror", Box::new(mirror)));
    graph.add_pass(RenderPassNode::new("forward", Box::new(forward)));
    graph.add_pass(RenderPassNode::new("water", Box::new(water_pass)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(MAIN_EXTENT, MAIN_EXTENT);
    let mut scene = Scene::new("mirror");

    let validation_before = rhi::validation_error_count();
    graph.execute(&device, &mut context, &mut scene);
    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while drawing the scene"
    );
    device.wait_idle().expect("wait for the frame");

    // The main camera's picture: the cube is nowhere in its frustum, so
    // nothing but the clear colour may be there. This is the geometry the
    // mirror proof stands on — if the main camera *does* see the cube, the
    // assertion below about the reflection proves nothing.
    let main_bytes = device
        .download_texture(&colour, 0, 0)
        .expect("the main image could not be read back");
    assert_eq!(
        bright_pixels(&main_bytes, 0.4),
        0,
        "the main camera's image holds lit pixels: the cube is in its view, so \
         the reflection's content has an explanation other than the mirroring"
    );

    // The reflection's image: the lit cube, which only the mirrored camera
    // can see.
    let reflection_texture = mirror_state
        .lock()
        .expect("state")
        .reflection_texture
        .clone()
        .expect("the mirror's reflection texture");
    let reflection_bytes = device
        .download_texture(&reflection_texture, 0, 0)
        .expect("the reflection could not be read back");
    let reflection_bright = bright_pixels(&reflection_bytes, 0.35);
    assert!(
        reflection_bright >= 100,
        "the reflection's image holds {reflection_bright} lit pixels of {} — the \
         mirror camera did not render the scene into it",
        MIRROR_EXTENT * MIRROR_EXTENT
    );

    // And the water drew its surface with that reflection bound: it refuses
    // to draw without one, so a drawn surface is the chain running end to
    // end. The water's own image is cleared to a fully transparent black, and
    // the shader writes alpha 0.85.
    let water_output = water_state
        .lock()
        .expect("state")
        .output_texture
        .clone()
        .expect("the water's image");
    let water_bytes = device
        .download_texture(&water_output, 0, 0)
        .expect("the water's image could not be read back");
    let water_drawn = water_bytes
        .chunks_exact(4)
        .filter(|pixel| pixel[3] as f32 / 255.0 > 0.5)
        .count();
    assert!(
        water_drawn > 0,
        "the water drew nothing: with the mirror's reflection bound the surface \
         must still be drawn"
    );
}
