//! Is the transparent object tested against the **opaque** geometry's depth?
//!
//! The transparent pass used to have a depth buffer of its own, cleared to the
//! far plane before it drew. Every transparent object was therefore in front
//! of every other one — including the ones behind a wall, which were drawn
//! over the wall.
//!
//! The fix is a depth buffer the frame shares: the forward pass writes the
//! opaque geometry's depth into it, and the transparent pass attaches it and
//! draws over the picture that is already in the target.
//!
//! So the test draws an opaque cube and a transparent quad **behind** it, and
//! requires the cube to be visible where it is: if the transparent quad were
//! drawn over the cube, the cube's colour would be blended towards the quad's
//! red.
//!
//! The test also checks the other half — that the transparent object is drawn
//! at all where nothing is in front of it. A pass that drew nothing would
//! satisfy the first check perfectly, and the second is what rules it out.

use std::sync::Arc;

use glam::{Mat4, Vec3, Vec4};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::lighting::light_buffer::pack_light;
use rfs_client::render::meshes::{Mesh, Vertex};
use rfs_client::render::passes::depth_target::DepthTarget;
use rfs_client::render::passes::forward::ForwardPass;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::shadow::{light_view_projection, ShadowConfig, ShadowPass};
use rfs_client::render::passes::transparent::{BlendMode, TransparentConfig, TransparentItem, TransparentPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{Format, FramebufferAttachment, FramebufferDesc, RhiConfig, TextureUsage};

const EXTENT: u32 = 64;
const FORWARD_SHADER: &[u8] = include_bytes!("../assets/shaders/forward.spv");
const SHADOW_SHADER: &[u8] = include_bytes!("../assets/shaders/shadow.spv");
const CLUSTER_SHADER: &[u8] = include_bytes!("../assets/shaders/cluster.spv");
const TRANSPARENT_SHADER: &[u8] = include_bytes!("../assets/shaders/transparent.spv");

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

/// A cube of the size 1, at the origin.
fn cube(device: &rhi::Device) -> Mesh {
    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(device).expect("cube buffers");
    mesh
}

/// A quad in the XY plane, half transparent and red, at `z`.
fn red_quad(device: &rhi::Device, z: f32) -> Mesh {
    let colour = Vec4::new(1.0, 0.0, 0.0, 0.5);
    let vertices = vec![
        Vertex::new(Vec3::new(-2.0, -2.0, z)).with_color(colour),
        Vertex::new(Vec3::new(2.0, -2.0, z)).with_color(colour),
        Vertex::new(Vec3::new(2.0, 2.0, z)).with_color(colour),
        Vertex::new(Vec3::new(-2.0, 2.0, z)).with_color(colour),
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];
    let mut mesh = Mesh::new("quad").with_vertices(vertices).with_indices(indices);
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

#[test]
fn the_transparent_object_is_behind_the_opaque_one() {
    let device = open_device();
    let cube = cube(&device);
    // The quad is *behind* the cube: the camera is at z = 3 looking at the
    // origin, and the cube is at z = 0, so the quad has to be further away.
    let quad = red_quad(&device, -1.0);

    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y);
    let projection = Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0);

    // The frame's shared depth: the forward pass writes it, the transparent
    // pass reads it.
    let depth = DepthTarget::new();

    // The forward pass: the opaque cube.
    let (forward, forward_state) = ForwardPass::create(
        &device,
        vec![item("cube", &cube)],
        Format::RGBA8_UNORM,
        FORWARD_SHADER,
    )
    .expect("forward pass");
    ForwardPass::set_depth_target(&forward_state, depth.clone());

    // The shadow pass: the cube's shadow.
    let (shadow, shadow_state) = ShadowPass::create(
        &device,
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
        ShadowPass::update_light(&shadow_state, &device, &[light_space]),
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
        rfs_client::render::passes::cluster::ClusterPass::create(&device, config, CLUSTER_SHADER)
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

    // The transparent pass: the red quad, and the frame's shared depth.
    let (transparent, transparent_state) = TransparentPass::create(
        &device,
        vec![TransparentItem {
            item: item("quad", &quad),
            blend_mode: BlendMode::Alpha,
        }],
        TransparentConfig::default(),
        Format::RGBA8_UNORM,
        TRANSPARENT_SHADER,
    )
    .expect("transparent pass");
    TransparentPass::set_depth_target(&transparent_state, depth.clone());

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
    let depth_view = depth
        .view(&device, EXTENT, EXTENT)
        .expect("the shared depth's view");

    // The two passes draw into the same colour image and the same depth, and
    // each has its own render pass: the forward pass's writes the depth, the
    // transparent pass's reads it.
    let forward_framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: forward_state.lock().expect("state").render_pass.clone(),
            attachments: vec![
                FramebufferAttachment { texture_view: colour_view.clone(), layer: 0, mip_level: 0 },
                FramebufferAttachment { texture_view: depth_view.clone(), layer: 0, mip_level: 0 },
            ],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("the forward framebuffer");
    ForwardPass::set_framebuffer(&forward_state, Some(forward_framebuffer));

    let transparent_framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: transparent_state
                .lock()
                .expect("state")
                .shared_render_pass
                .clone()
                .expect("the shared-depth render pass"),
            attachments: vec![
                FramebufferAttachment { texture_view: colour_view.clone(), layer: 0, mip_level: 0 },
                FramebufferAttachment { texture_view: depth_view.clone(), layer: 0, mip_level: 0 },
            ],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("the transparent framebuffer");
    TransparentPass::set_framebuffer(&transparent_state, Some(transparent_framebuffer), EXTENT, EXTENT);

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
            Vec3::new(0.1, 0.1, 0.1),
        ),
        "the forward uniform upload must succeed"
    );
    assert!(
        rfs_client::render::passes::cluster::ClusterPass::update_uniforms(
            &cluster_state,
            &device,
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
        TransparentPass::update_uniforms(&transparent_state, &device, view, projection),
        "the transparent uniform upload must succeed"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("cluster", Box::new(cluster)));
    graph.add_pass(RenderPassNode::new("shadow", Box::new(shadow)));
    graph.add_pass(RenderPassNode::new("forward", Box::new(forward)));
    graph.add_pass(RenderPassNode::new("transparent", Box::new(transparent)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("depth");

    let validation_before = rhi::validation_error_count();
    graph.execute(&device, &mut context, &mut scene);
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
    let green = pixel[1] as f32 / 255.0;

    // The cube is white and the light is white, so the cube's colour is
    // saturated: every channel is 1.0.
    //
    // The **green** channel is the one that says whether the transparent quad
    // was drawn over the cube. The quad is red, so a quad over the cube would
    // halve the green (0.5 * red + 0.5 * white). The cube alone leaves the
    // green at 1.0, and the red at 1.0 either way — which is why the red
    // alone cannot answer the question.
    assert!(
        green > 0.8,
        "the centre pixel's green is {green} and the cube's is 1.0: the \
         transparent quad was drawn over the cube, so the transparent pass was \
         not tested against the opaque depth"
    );

    // And the quad **was** drawn where nothing is in front of it: the corner
    // of the screen is outside the cube, and there the quad is all that is
    // there. A pass that drew nothing would satisfy the check above
    // perfectly, and this is what rules it out.
    let corner = 4usize;
    let corner_pixel = &bytes[(corner * EXTENT as usize + corner) * 4..][..4];
    let corner_red = corner_pixel[0] as f32 / 255.0;
    assert!(
        corner_red > 0.3,
        "the corner pixel's red is {corner_red}: the transparent quad was not \
         drawn at all, so the check above proves nothing"
    );
}
