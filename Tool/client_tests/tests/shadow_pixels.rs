//! Does the shadow pass put the scene into the shadow map?
//!
//! The shadow pass used to be a set of CPU-side descriptions with no native
//! handle, a pipeline that was `None` "until a shader existed" — the shader
//! has existed all along — and an `execute` that opened a render pass and
//! closed it again. It reported itself as having recorded work, because
//! recording an empty pass is recording.
//!
//! So the test does not ask whether the pass recorded commands. It reads the
//! shadow map back and requires the cube to be in it: the pixels where the
//! cube is hold a depth below the clear value, and the pixels where nothing
//! was drawn hold exactly the clear value. Both halves are needed — the first
//! catches a pass that drew nothing, the second catches a light whose
//! frustum points away from the geometry.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::shadow::{light_view_projection, ShadowConfig, ShadowPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::RhiConfig;

const EXTENT: u32 = 256;
const SHADER: &[u8] = include_bytes!("../assets/shaders/shadow.spv");

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

#[test]
fn the_shadow_map_holds_the_geometry() {
    let device = open_device();

    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(&device).expect("cube buffers");

    let config = ShadowConfig {
        resolution: 128,
        cascade_count: 1,
        ..Default::default()
    };
    let (pass, state) = ShadowPass::create(
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
        config,
        SHADER,
    )
    .expect("shadow pass");

    // The light in front of the cube, looking at it.
    let light_space = light_view_projection(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, 50.0);
    assert!(
        ShadowPass::update_light(
            &state,
            &device,
            &[light_space],
        ),
        "the light's uniform upload must succeed"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("shadow", Box::new(pass)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("shadow");

    let validation_before = rhi::validation_error_count();
    graph.execute(&device, &mut context, &mut scene);
    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while rendering the shadow map"
    );

    device.wait_idle().expect("wait for the frame");

    let shadow_map = ShadowPass::shadow_map(&state).expect("the shadow map exists");
    let bytes = device
        .download_texture(&shadow_map, 0, 0)
        .expect("the shadow map could not be read back");

    let mut written = 0u32;
    let mut cleared = 0u32;
    for pixel in bytes.chunks_exact(4) {
        let value = f32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
        if value >= 0.999 {
            cleared += 1;
        } else {
            written += 1;
        }
    }
    assert!(
        written > 0,
        "the shadow map holds nothing but the clear value: the geometry did not \
         reach the shadow map"
    );
    assert!(
        cleared > 0,
        "every pixel of the shadow map was written: the cube covers the whole \
         map, which means the light's frustum or the vertex positions are wrong"
    );
}
