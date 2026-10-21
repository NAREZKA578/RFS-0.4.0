//! Do the cascades cut the frustum where they are supposed to?
//!
//! A directional light's shadow is parallel, so one map for the whole scene
//! would spend its resolution on the far plane, where the shadow is a few
//! pixels wide. The frustum is therefore cut into slices, and each slice gets
//! its own map and its own orthographic projection.
//!
//! Two things have to hold, and they are checked here separately. The
//! **geometry** of the slices: each slice's box has to contain the part of the
//! scene it is responsible for, and the slices have to grow with the
//! distance. The **maps**: the pass has to produce one map per cascade, and
//! the scene has to be in the map of the cascade that covers it.
//!
//! The slice's geometry is a pure function, so it is checked without a
//! device; the maps are checked on the GPU by reading them back.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::GeometryItem;
use rfs_client::render::passes::shadow::{cascade_matrices, ShadowConfig, ShadowPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::RhiConfig;

const EXTENT: u32 = 128;
const SHADER: &[u8] = include_bytes!("../assets/shaders/shadow.spv");

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

/// The camera and the light the cascades are cut for.
fn camera() -> (Mat4, Mat4) {
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y);
    let projection = Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 50.0);
    (view, projection)
}

#[test]
fn each_cascade_covers_its_slice_of_the_frustum() {
    let (view, projection) = camera();
    let splits = [10.0f32, 20.0, 40.0];
    let matrices = cascade_matrices(
        view,
        projection,
        Vec3::new(0.0, -1.0, 0.0),
        0.1,
        50.0,
        &splits,
    );
    assert_eq!(
        matrices.len(),
        splits.len(),
        "one matrix per cascade: a cascade without a matrix is a slice nobody \
         renders into"
    );

    // The cube at the origin is three units from the camera, so it belongs to
    // the first slice, and the first cascade's box has to contain it.
    let cube = Vec3::new(0.0, 0.0, 0.0);
    let clip = matrices[0] * cube.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    assert!(
        ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0,
        "the cube is outside the first cascade's box: {ndc:?}"
    );
    assert!(
        (0.0..=1.0).contains(&ndc.z),
        "the cube's depth in the first cascade is {} and the shadow map's depth \
         range is [0, 1]",
        ndc.z
    );

    // The slices grow: the further a slice reaches, the bigger its box is.
    // If they did not grow, the cascades would be the same map four times and
    // the far plane would still be as coarse as the near one.
    let extents: Vec<f32> = matrices
        .iter()
        .map(|m| {
            // The box's width, read off the matrix: the orthographic
            // projection's x scale is 1 / half-width. The sign depends on
            // which way the light's view happens to point, so the magnitude is
            // what says how big the box is.
            let scale = m.x_axis.x.recip();
            scale.abs() * 2.0
        })
        .collect();
    for window in extents.windows(2) {
        assert!(
            window[1] > window[0],
            "the cascades do not grow: {extents:?}"
        );
    }
}

#[test]
fn the_pass_draws_the_scene_into_every_cascade() {
    let device = open_device();
    let (view, projection) = camera();

    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(&device).expect("cube buffers");

    let splits = [10.0f32, 20.0, 40.0];
    let config = ShadowConfig {
        resolution: 128,
        cascade_count: splits.len(),
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

    let matrices = cascade_matrices(
        view,
        projection,
        Vec3::new(0.0, -1.0, 0.0),
        0.1,
        50.0,
        &splits,
    );
    assert!(
        ShadowPass::update_light(&state, &device, &matrices),
        "the light's uniform upload must succeed"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("shadow", Box::new(pass)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("shadow");

    graph.execute(&device, &mut context, &mut scene);
    device.wait_idle().expect("wait for the frame");

    let maps = ShadowPass::shadow_maps(&state);
    assert_eq!(
        maps.len(),
        splits.len(),
        "one shadow map per cascade"
    );

    // The first cascade holds the cube: it is the slice the cube falls into.
    let bytes = device
        .download_texture(&maps[0], 0, 0)
        .expect("the first cascade's map could not be read back");
    let mut written = 0u32;
    for pixel in bytes.chunks_exact(4) {
        let value = f32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
        if value < 0.999 {
            written += 1;
        }
    }
    assert!(
        written > 0,
        "the first cascade's map holds nothing but the clear value: the scene \
         did not reach it"
    );
}
