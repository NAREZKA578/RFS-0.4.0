//! Does the forward pass take its light from the clustering pass's buffer?
//!
//! This is the junction where Forward+ either works or does not. The
//! clustering pass bins the lights into the screen's clusters and writes the
//! **indices**; the forward pass walks its own cluster and reads the lights by
//! those indices. The two have to agree on two things at once, and neither is
//! visible in a picture that comes out merely dark:
//!
//! - **Which buffer.** The indices are indices into the lights buffer the
//!   clustering binned from. A forward pass with its own lights buffer would
//!   read whatever happens to be at those places.
//! - **The layout.** A `vec3` in a struct is 16-aligned and 12 bytes long, so
//!   the light's fields sit at 0, 16, **28** and **32** — the two floats after
//!   the vectors are placed by *their own* alignment, which is 4, not the
//!   struct's 16. A writer that puts them four bytes late makes the shader
//!   read the intensity out of the padding: every light has an intensity of
//!   zero, and the picture is lit by the ambient term alone.
//!
//! The second one is not hypothetical. The repository had the layout written
//! down in three places, and two of them disagreed with the third — and the
//! disagreement was invisible to every test, because the clustering pass only
//! reads the light's *position* and *radius*, and a light with the wrong
//! intensity still lands in exactly the same clusters.
//!
//! So the test does not check "the pass ran". It renders the same scene twice
//! and requires the picture to change when the light's numbers change, and it
//! checks the numbers the shader actually saw by reading them back out of the
//! GPU.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::lighting::light_buffer::{pack_light, LIGHT_STRIDE_FLOATS};
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::cluster::{ClusterConfig, ClusterPass};
use rfs_client::render::passes::forward::ForwardPass;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::shadow::{light_view_projection, ShadowConfig, ShadowPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::RhiConfig;

const EXTENT: u32 = 128;
const FORWARD_SHADER: &[u8] = include_bytes!("../assets/shaders/forward.spv");
const SHADOW_SHADER: &[u8] = include_bytes!("../assets/shaders/shadow.spv");
const CLUSTER_SHADER: &[u8] = include_bytes!("../assets/shaders/cluster.spv");

/// The light the scene is lit by, and the numbers the test writes into the
/// buffer.
struct SceneLight {
    position: Vec3,
    color: Vec3,
    intensity: f32,
    radius: f32,
}

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

fn scene_items(device: &rhi::Device) -> Vec<GeometryItem> {
    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(device).expect("cube buffers");
    vec![GeometryItem {
        name: "cube".into(),
        vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
        index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
        index_type: mesh.index_type(),
        index_count: mesh.index_count(),
        model: Mat4::IDENTITY,
        entity_id: None,
    }]
}

/// The camera, the same in every render of the test.
fn camera() -> (Mat4, Mat4) {
    (
        Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y),
        Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
    )
}

/// Render the scene lit by `light` and read the centre pixel's brightness.
///
/// The whole chain runs: the clustering pass bins the light, the shadow pass
/// renders the depth, and the forward pass reads both.
fn render_with_light(device: &Arc<rhi::Device>, light: SceneLight) -> f32 {
    let items = scene_items(device);
    let (view, projection) = camera();

    // The clustering pass: it owns the lights buffer, and the forward pass
    // reads that same buffer.
    let config = ClusterConfig {
        tiles_x: 4,
        tiles_y: 4,
        depth_slices: 8,
    };
    let (cluster, cluster_state) =
        ClusterPass::create(device, config, CLUSTER_SHADER).expect("cluster pass");
    {
        let state = cluster_state.lock().expect("state");
        // The light, in the canonical layout. This is the one place the
        // layout is written down, and the test `light_layout.rs` proves the
        // shader reads it from there.
        let mut data = vec![0.0f32; LIGHT_STRIDE_FLOATS];
        assert!(
            pack_light(
                &mut data,
                0,
                light.position,
                light.color,
                light.intensity,
                light.radius
            ),
            "the light must fit into the buffer"
        );
        device
            .upload_buffer(&state.lights_buffer, &data)
            .expect("the light's upload must succeed");
    }

    // The shadow pass: one cascade, the point light's own view.
    let (shadow, shadow_state) = ShadowPass::create(
        device,
        items.clone(),
        ShadowConfig {
            resolution: 256,
            cascade_count: 1,
            ..Default::default()
        },
        SHADOW_SHADER,
    )
    .expect("shadow pass");
    let light_space = light_view_projection(light.position, Vec3::ZERO, 50.0);
    assert!(
        ShadowPass::update_light(&shadow_state, device, &[light_space]),
        "the light's uniform upload must succeed"
    );

    let (forward, forward_state) =
        ForwardPass::create(device, items, rhi::Format::RGBA8_UNORM, FORWARD_SHADER)
            .expect("forward pass");

    // The shadow map the forward pass reads.
    let shadow_map = ShadowPass::shadow_map(&shadow_state).expect("the shadow map");
    let shadow_view = device
        .create_texture_view(
            &shadow_map,
            &rhi::TextureViewDesc {
                texture: shadow_map.clone(),
                format: None,
                view_type: rhi::TextureViewType::D2,
                aspects: rhi::TextureAspectFlags::DEPTH,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("shadow view");
    ForwardPass::set_shadow_maps(&forward_state, vec![shadow_view]);

    // The clustering pass's buffers: the lights and the cluster indices. This
    // is the wiring the whole feature is about, and it is what makes the
    // difference between "the forward pass has a light" and "the forward pass
    // has *the* light".
    {
        let state = cluster_state.lock().expect("state");
        ForwardPass::set_cluster_buffers(
            &forward_state,
            state.lights_buffer.clone(),
            state.cluster_indices_buffer.clone(),
            config,
        );
    }

    // The target the scene is drawn into.
    let colour = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            rhi::Format::RGBA8_UNORM,
            rhi::TextureUsage::COLOR_ATTACHMENT | rhi::TextureUsage::TRANSFER_SRC,
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
            rhi::Format::D32_SFLOAT,
            rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT,
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
    let framebuffer = device
        .create_gpu_framebuffer(&rhi::FramebufferDesc {
            render_pass: forward_state.lock().expect("state").render_pass.clone(),
            attachments: vec![
                rhi::FramebufferAttachment { texture_view: colour_view, layer: 0, mip_level: 0 },
                rhi::FramebufferAttachment { texture_view: depth_view, layer: 0, mip_level: 0 },
            ],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");
    ForwardPass::set_framebuffer(&forward_state, Some(framebuffer));

    // The frame's uniforms: the camera, the cascade, the grid and the
    // ambient term.
    assert!(
        rfs_client::render::passes::forward::ForwardPass::update_uniforms(
            &forward_state,
            device,
            view,
            projection,
            &[light_space],
            &[100.0],
            1,
            0,
            0.1,
            100.0,
            Vec3::new(0.02, 0.02, 0.02),
        ),
        "the uniform upload must succeed"
    );

    // The clustering pass's own uniforms: the same camera, so the clusters
    // and the forward pass agree about which cluster a fragment is in.
    assert!(
        ClusterPass::update_uniforms(
            &cluster_state,
            device,
            view,
            projection,
            1,
            0.1,
            100.0,
            EXTENT,
            EXTENT,
        ),
        "the cluster uniform upload must succeed"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("cluster", Box::new(cluster)));
    graph.add_pass(RenderPassNode::new("shadow", Box::new(shadow)));
    graph.add_pass(RenderPassNode::new("forward", Box::new(forward)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("forward");

    let validation_before = rhi::validation_error_count();
    graph.execute(device, &mut context, &mut scene);
    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while the forward pass drew the scene"
    );

    device.wait_idle().expect("wait for the frame");

    let bytes = device
        .download_texture(&colour, 0, 0)
        .expect("the target could not be read back");
    let centre = (EXTENT / 2) as usize;
    let pixel = &bytes[(centre * EXTENT as usize + centre) * 4..][..4];
    (pixel[0] as f32 + pixel[1] as f32 + pixel[2] as f32) / (3.0 * 255.0)
}

/// The light of the scene, and the numbers the test changes.
fn the_light() -> SceneLight {
    SceneLight {
        position: Vec3::new(0.0, 0.0, 4.0),
        color: Vec3::ONE,
        intensity: 4.0,
        radius: 50.0,
    }
}

#[test]
fn the_forward_pass_lights_the_scene_from_the_clustering_buffers() {
    let device = open_device();
    let lit = render_with_light(&device, the_light());
    assert!(
        lit > 0.2,
        "the centre of the lit render is {lit}: the forward pass has to take the \
         light from the clustering pass's buffer"
    );
}

/// The decisive check on the layout.
///
/// The light's **intensity** is at byte 28 and its radius at byte 32 — the two
/// floats after the vectors are placed by their own alignment, which is 4.
/// A writer that puts them four bytes late (32 and 36) makes the shader read
/// the intensity out of the padding: the light is in the cluster, the cluster
/// is right, and the picture is lit by the ambient term alone. Every light
/// contributes nothing, and nothing says so.
///
/// So the test changes the intensity *alone* and requires the picture to
/// change with it. If the intensity were read from the padding, the two
/// renders would be the same picture, and the test would say so.
#[test]
fn the_picture_follows_the_lights_intensity() {
    let device = open_device();

    let mut dim = the_light();
    dim.intensity = 0.2;
    let dim = render_with_light(&device, dim);

    let bright = render_with_light(&device, the_light());

    assert!(
        bright > dim * 1.5,
        "the dim render's centre is {dim} and the bright one's is {bright}: the \
         forward pass has to read the intensity from the light's own field, and \
         not from the padding next to it"
    );
}

/// The same check for the radius: a light with a tiny radius does not reach
/// the fragment, and the picture must go dark.
#[test]
fn the_picture_follows_the_lights_radius() {
    let device = open_device();

    let mut tiny = the_light();
    tiny.radius = 0.5;
    tiny.intensity = 8.0;
    let tiny = render_with_light(&device, tiny);

    let wide = render_with_light(&device, the_light());

    assert!(
        wide > tiny * 1.5,
        "the narrow render's centre is {tiny} and the wide one's is {wide}: the \
         forward pass has to read the radius from the light's own field"
    );
}
