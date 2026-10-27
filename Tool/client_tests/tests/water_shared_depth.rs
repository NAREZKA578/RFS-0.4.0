//! Is the water tested against the **opaque** geometry's depth?
//!
//! The water pass used to have a depth buffer of its own, cleared to the far
//! plane before it drew. Every water surface was therefore in front of every
//! other one — and, worse, in front of the walls: a lake behind a building
//! was drawn over the building.
//!
//! The fix is the same one the transparent pass got: a depth buffer the frame
//! shares. The forward pass writes the opaque geometry's depth into it, and
//! the water pass attaches it read-only.
//!
//! So the test draws an opaque cube and a water surface **behind** it, and
//! requires the cube to be visible where it is. If the water surface were
//! drawn over the cube, the cube's colour would be blended towards the
//! water's.
//!
//! The test also checks the other half — that the water surface is drawn at
//! all where nothing is in front of it. A pass that drew nothing would
//! satisfy the first check perfectly, and the second is what rules it out.
//!
//! And it renders the same scene a third time, with the water surface moved
//! **in front** of the cube. There the water has to cover the cube, and the
//! first check has to fail. That render is what gives the first check its
//! teeth: if the water were not tested against the opaque depth at all, the
//! two renders would be the same picture and the first check would pass for
//! the wrong reason.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::lighting::light_buffer::pack_light;
use rfs_client::render::meshes::{Mesh, Vertex};
use rfs_client::render::passes::depth_target::DepthTarget;
use rfs_client::render::passes::forward::ForwardPass;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::shadow::{light_view_projection, ShadowConfig, ShadowPass};
use rfs_client::render::passes::water::{WaterConfig, WaterPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{Format, FramebufferAttachment, FramebufferDesc, RhiConfig, TextureUsage};

const EXTENT: u32 = 64;
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

/// A cube of the size 1, at the origin.
fn cube(device: &rhi::Device) -> Mesh {
    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(device).expect("cube buffers");
    mesh
}

/// A quad in the XY plane, at `z`, facing the camera.
fn quad(device: &rhi::Device, z: f32) -> Mesh {
    let vertices = vec![
        Vertex::new(Vec3::new(-2.0, -2.0, z)).with_normal(Vec3::Z),
        Vertex::new(Vec3::new(2.0, -2.0, z)).with_normal(Vec3::Z),
        Vertex::new(Vec3::new(2.0, 2.0, z)).with_normal(Vec3::Z),
        Vertex::new(Vec3::new(-2.0, 2.0, z)).with_normal(Vec3::Z),
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];
    let mut mesh = Mesh::new("quad")
        .with_vertices(vertices)
        .with_indices(indices);
    mesh.create_buffers(device).expect("quad buffers");
    mesh
}

fn item(name: &str, mesh: &Mesh) -> GeometryItem {
    GeometryItem {
        name: name.into(),
        vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
        index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
        index_type: mesh.index_type(),
        index_count: mesh.index_count(),
        model: Mat4::IDENTITY,
        entity_id: None,
    }
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

fn colour_view(device: &rhi::Device, texture: &rhi::Texture) -> rhi::TextureView {
    device
        .create_texture_view(
            texture,
            &rhi::TextureViewDesc {
                texture: texture.clone(),
                format: None,
                view_type: rhi::TextureViewType::D2,
                aspects: rhi::TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("colour view")
}

/// The picture the scene made: the centre's green and the corner's red.
///
/// The green is the one that says whether the water was drawn over the cube
/// (the water is red, the cube is white), and the corner's red is the one
/// that says whether the water was drawn at all.
type Picture = (f32, f32);

/// Render the scene with the water surface at `water_z` and read the picture
/// back.
fn render(device: &Arc<rhi::Device>, water_z: f32) -> Picture {
    let cube = cube(device);
    // The water surface is at `water_z`: behind the cube when it is negative,
    // in front of it when it is positive. The camera is at z = 3 looking at
    // the origin, and the cube is at z = 0.
    let water = quad(device, water_z);

    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y);
    let projection = Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0);

    // The frame's shared depth: the forward pass writes it, the water pass
    // reads it.
    let depth = DepthTarget::new();

    // The forward pass: the opaque cube.
    let (forward, forward_state) = ForwardPass::create(
        device,
        vec![item("cube", &cube)],
        Format::RGBA8_UNORM,
        FORWARD_SHADER,
    )
    .expect("forward pass");
    ForwardPass::set_depth_target(&forward_state, depth.clone());

    // The shadow pass: the cube's shadow.
    let (shadow, shadow_state) = ShadowPass::create(
        device,
        vec![item("cube", &cube)],
        ShadowConfig {
            resolution: 128,
            cascade_count: 1,
            ..Default::default()
        },
        SHADOW_SHADER,
    )
    .expect("shadow pass");
    let light_space = light_view_projection(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, 50.0);
    assert!(
        ShadowPass::update_light(&shadow_state, device, &[light_space]),
        "the light's uniform upload must succeed"
    );
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

    // The clustering pass: the light, in the buffer the forward pass reads.
    let config = rfs_client::render::passes::cluster::ClusterConfig {
        tiles_x: 4,
        tiles_y: 4,
        depth_slices: 8,
    };
    let (cluster, cluster_state) =
        rfs_client::render::passes::cluster::ClusterPass::create(device, config, CLUSTER_SHADER)
            .expect("cluster pass");
    {
        let state = cluster_state.lock().expect("state");
        let mut lights = vec![0.0f32; 12];
        pack_light(
            &mut lights,
            0,
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::ONE,
            4.0,
            50.0,
        );
        device
            .upload_buffer(&state.lights_buffer, &lights)
            .expect("the light's upload must succeed");
    }
    {
        let state = cluster_state.lock().expect("state");
        ForwardPass::set_cluster_buffers(
            &forward_state,
            state.lights_buffer.clone(),
            state.cluster_indices_buffer.clone(),
            config,
        );
    }

    // The water pass: the surface, and the frame's shared depth.
    let (water_pass, water_state) = WaterPass::create(
        device,
        vec![item("water", &water)],
        WaterConfig::default(),
        Format::RGBA8_UNORM,
        WATER_SHADER,
    )
    .expect("water pass");
    WaterPass::set_depth_target(&water_state, depth.clone());

    // The reflection and the refraction. Both are red, so the water is red
    // wherever it is drawn: the checks below are about *where* it is drawn,
    // not about what colour the shader mixes.
    let reflection = flat_texture(device, [255, 0, 0, 255]);
    let refraction = flat_texture(device, [255, 0, 0, 255]);
    WaterPass::set_textures(
        &water_state,
        device,
        Some(colour_view(device, &reflection)),
        Some(colour_view(device, &refraction)),
    );

    // The target: a colour image and the shared depth.
    let colour = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the shared colour target must be created");
    let colour_image_view = colour_view(device, &colour);
    let depth_view = depth
        .view(device, EXTENT, EXTENT)
        .expect("the shared depth's view");

    // The two passes draw into the same colour image and the same depth, and
    // each has its own render pass: the forward pass's writes the depth, the
    // water pass's reads it.
    let forward_framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: forward_state.lock().expect("state").render_pass.clone(),
            attachments: vec![
                FramebufferAttachment {
                    texture_view: colour_image_view.clone(),
                    layer: 0,
                    mip_level: 0,
                },
                FramebufferAttachment {
                    texture_view: depth_view.clone(),
                    layer: 0,
                    mip_level: 0,
                },
            ],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("the forward framebuffer");
    ForwardPass::set_framebuffer(&forward_state, Some(forward_framebuffer));

    // The water pass's framebuffer for the shared-depth mode: the same colour
    // image and the same depth, but the shared-depth render pass — the colour
    // is loaded, not cleared, and the depth is not written.
    let water_framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: water_state
                .lock()
                .expect("state")
                .shared_render_pass
                .clone()
                .expect("the shared-depth render pass"),
            attachments: vec![
                FramebufferAttachment {
                    texture_view: colour_image_view.clone(),
                    layer: 0,
                    mip_level: 0,
                },
                FramebufferAttachment {
                    texture_view: depth_view.clone(),
                    layer: 0,
                    mip_level: 0,
                },
            ],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("the water framebuffer");
    WaterPass::set_framebuffer(&water_state, Some(water_framebuffer));

    assert!(
        ForwardPass::update_uniforms(
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
            Vec3::new(0.1, 0.1, 0.1),
        ),
        "the forward uniform upload must succeed"
    );
    assert!(
        rfs_client::render::passes::cluster::ClusterPass::update_uniforms(
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
    assert!(
        WaterPass::update_uniforms(&water_state, device, view, projection, 0.0),
        "the water uniform upload must succeed"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("cluster", Box::new(cluster)));
    graph.add_pass(RenderPassNode::new("shadow", Box::new(shadow)));
    graph.add_pass(RenderPassNode::new("forward", Box::new(forward)));
    graph.add_pass(RenderPassNode::new("water", Box::new(water_pass)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("water depth");

    let validation_before = rhi::validation_error_count();
    graph.execute(device, &mut context, &mut scene);
    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while drawing the scene"
    );

    device.wait_idle().expect("wait for the frame");

    let bytes = device
        .download_texture(&colour, 0, 0)
        .expect("the target could not be read back");
    let centre = (EXTENT / 2) as usize;
    let pixel = &bytes[(centre * EXTENT as usize + centre) * 4..][..4];
    let centre_green = pixel[1] as f32 / 255.0;
    // The corner of the screen is outside the cube, and there the water is
    // all that is there.
    let corner = 4usize;
    let corner_pixel = &bytes[(corner * EXTENT as usize + corner) * 4..][..4];
    let corner_red = corner_pixel[0] as f32 / 255.0;
    (centre_green, corner_red)
}

#[test]
fn the_water_is_behind_the_opaque_geometry() {
    let device = open_device();

    // The water surface behind the cube.
    let (behind_green, behind_corner_red) = render(&device, -1.0);

    // The cube is white and the light is white, so the cube's colour is
    // saturated: every channel is 1.0.
    //
    // The **green** channel is the one that says whether the water was drawn
    // over the cube. The water is red, so a water surface over the cube would
    // take the green away (0.85 * red + 0.15 * white). The cube alone leaves
    // the green at 1.0, and the red at 1.0 either way — which is why the red
    // alone cannot answer the question.
    assert!(
        behind_green > 0.8,
        "the centre pixel's green is {behind_green} and the cube's is 1.0: the \
         water surface was drawn over the cube, so the water pass was not \
         tested against the opaque depth"
    );

    // And the water **was** drawn where nothing is in front of it: the corner
    // of the screen is outside the cube, and there the water is all that is
    // there. A pass that drew nothing would satisfy the check above
    // perfectly, and this is what rules it out.
    assert!(
        behind_corner_red > 0.3,
        "the corner pixel's red is {behind_corner_red}: the water surface was \
         not drawn at all, so the check above proves nothing"
    );
}

#[test]
fn the_water_covers_the_geometry_when_it_is_in_front_of_it() {
    let device = open_device();

    // The same scene, with the water surface in front of the cube. The water
    // is translucent, so the cube shows through a little — but the green has
    // to go: the water takes 85% of the colour.
    let (front_green, _) = render(&device, 1.0);

    assert!(
        front_green < 0.4,
        "the centre pixel's green is {front_green} and the water covers 85% of \
         the cube: the water in front of the cube was not drawn at all, so the \
         check in the other test proves nothing"
    );
}
