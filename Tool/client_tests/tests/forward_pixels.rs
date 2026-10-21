//! Does the forward pass light the scene, and does it use the shadow map?
//!
//! The forward pass is the consumer of the shadow map: without it the shadow
//! pass writes a picture nobody looks at, and the repository has a name for
//! that class of defect.
//!
//! So the test renders the same scene twice, with the light in front of the
//! cube and with the light behind it, and requires the visible face to be lit
//! in the first case and in shadow in the second. The shadow map is rendered
//! from the light each time, so the two renders differ in exactly one thing —
//! where the light is — and the difference in the picture can only come from
//! the map.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::forward::{ForwardLight, ForwardPass};
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::shadow::{light_view_projection, ShadowConfig, ShadowPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::{Format, RhiConfig};

const EXTENT: u32 = 128;
const FORWARD_SHADER: &[u8] = include_bytes!("../assets/shaders/forward.spv");
const SHADOW_SHADER: &[u8] = include_bytes!("../assets/shaders/shadow.spv");

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

/// The scene: a cube, and the camera that looks at it.
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

/// Render the scene with the light at `light_position` and read the centre
/// pixel's brightness.
fn render_with_light(
    device: &Arc<rhi::Device>,
    light_position: Vec3,
) -> f32 {
    let items = scene_items(device);

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
    let light_space = light_view_projection(light_position, Vec3::ZERO, 50.0);
    assert!(
        ShadowPass::update_light(&shadow_state, device, &[light_space]),
        "the light's uniform upload must succeed"
    );

    let (forward, forward_state) =
        ForwardPass::create(device, items, Format::RGBA8_UNORM, FORWARD_SHADER).expect("forward pass");

    // The shadow map is the forward pass's input.
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
    ForwardPass::set_shadow_map(&forward_state, Some(shadow_view));

    // The target the scene is drawn into.
    let colour = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        Format::RGBA8_UNORM,
        rhi::TextureUsage::COLOR_ATTACHMENT | rhi::TextureUsage::TRANSFER_SRC,
        1,
    );
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
    let depth = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        Format::D32_SFLOAT,
        rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT,
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

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("shadow", Box::new(shadow)));
    graph.add_pass(RenderPassNode::new("forward", Box::new(forward)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("forward");

    let light = ForwardLight {
        position: light_position,
        color: Vec3::ONE,
        intensity: 4.0,
        radius: 50.0,
    };
    assert!(
        ForwardPass::update_uniforms(
            &forward_state,
            device,
            Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y),
            Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
            light,
            light_space,
            Vec3::new(0.02, 0.02, 0.02),
        ),
        "the uniform upload must succeed"
    );

    graph.execute(device, &mut context, &mut scene);
    device.wait_idle().expect("wait for the frame");

    let bytes = device
        .download_texture(&colour, 0, 0)
        .expect("the target could not be read back");
    let centre = (EXTENT / 2) as usize;
    let pixel = &bytes[(centre * EXTENT as usize + centre) * 4..][..4];
    (pixel[0] as f32 + pixel[1] as f32 + pixel[2] as f32) / (3.0 * 255.0)
}

#[test]
fn the_forward_pass_lights_the_scene_and_reads_the_shadow_map() {
    let device = open_device();

    // The light in front of the cube, where the camera is: the face the camera
    // sees is the lit one.
    let lit = render_with_light(&device, Vec3::new(0.0, 0.0, 5.0));
    // The light behind it: the face the camera sees is the one in shadow.
    let shadowed = render_with_light(&device, Vec3::new(0.0, 0.0, -5.0));

    assert!(
        lit > 0.2,
        "the centre of the lit render is {lit}: the light in front of the cube \
         has to light the face the camera sees"
    );
    assert!(
        lit > shadowed * 3.0,
        "the lit render's centre is {lit} and the shadowed one's is {shadowed}: \
         the shadow map has to be read, or the two renders would be the same \
         picture"
    );
}
