//! Does the clustering put the light in the right clusters?
//!
//! The clustering is the heart of Forward+: the screen is cut into tiles, each
//! tile into depth slices, and each of those cells — a cluster — holds the
//! indices of the lights that reach into it. A pixel then asks only its own
//! cluster which lights it has to look at.
//!
//! The test is deliberately not "the pass ran". It reads the cluster buffers
//! back and asks where the light ended up: a light at the origin, in front of
//! the camera, must be in the clusters around the middle of the screen, and
//! not in the ones at the far corner.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::passes::cluster::{ClusterConfig, ClusterPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::RhiConfig;

const SHADER: &[u8] = include_bytes!("../assets/shaders/cluster.spv");
const SCREEN: u32 = 256;

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

/// One light, in the layout the scene's light buffer uses: the position is a
/// `vec3` and so is the colour, and a `vec3` inside a struct is 16-aligned, so
/// the fields sit at 0, 16, 32 and 36 and the stride is 48.
fn one_light() -> [f32; 12] {
    let mut light = [0f32; 12];
    light[0] = 0.0;
    light[1] = 0.0;
    light[2] = 0.0;
    light[4] = 1.0;
    light[5] = 1.0;
    light[6] = 1.0;
    light[8] = 1.0;
    light[9] = 20.0;
    light
}

#[test]
fn the_light_lands_in_the_clusters_near_it() {
    let device = open_device();

    let config = ClusterConfig {
        tiles_x: 4,
        tiles_y: 4,
        depth_slices: 8,
    };
    let (pass, state) = ClusterPass::create(&device, config, SHADER).expect("cluster pass");

    // The light.
    let light = one_light();
    {
        let state = state.lock().expect("state");
        device
            .upload_buffer(&state.lights_buffer, &light)
            .expect("the light's upload must succeed");
    }

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("cluster", Box::new(pass)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(SCREEN, SCREEN);
    let mut scene = Scene::new("cluster");

    // The camera looks at the origin, where the light is.
    assert!(
        ClusterPass::update_uniforms(
            &state,
            &device,
            Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y),
            Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
            1,
            0.1,
            100.0,
            SCREEN,
            SCREEN,
        ),
        "the uniform upload must succeed"
    );

    let validation_before = rhi::validation_error_count();
    graph.execute(&device, &mut context, &mut scene);
    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while clustering the lights"
    );

    device.wait_idle().expect("wait for the frame");

    let (indices, counts) = ClusterPass::cluster_buffers(&state).expect("the cluster buffers");
    let count_bytes = device
        .download_buffer(&counts, 0, u64::from(config.cluster_count()) * 4)
        .expect("the cluster counts could not be read back");
    let index_bytes = device
        .download_buffer(
            &indices,
            0,
            u64::from(config.cluster_count())
                * u64::from(rfs_client::render::passes::cluster::MAX_LIGHTS_PER_CLUSTER)
                * 4,
        )
        .expect("the cluster indices could not be read back");

    let clusters = config.cluster_count() as usize;
    let mut filled = 0usize;
    for cluster in 0..clusters {
        let count = u32::from_le_bytes([
            count_bytes[cluster * 4],
            count_bytes[cluster * 4 + 1],
            count_bytes[cluster * 4 + 2],
            count_bytes[cluster * 4 + 3],
        ]);
        assert!(count <= 1, "cluster {cluster} holds {count} lights, and there is one");
        if count == 1 {
            filled += 1;
            // The light's index is 0 — it is the only light there is.
            let at = cluster * rfs_client::render::passes::cluster::MAX_LIGHTS_PER_CLUSTER as usize;
            let light_index = u32::from_le_bytes([
                index_bytes[at * 4],
                index_bytes[at * 4 + 1],
                index_bytes[at * 4 + 2],
                index_bytes[at * 4 + 3],
            ]);
            assert_eq!(
                light_index, 0,
                "cluster {cluster} holds the light {light_index}, and there is only one"
            );
        }
    }
    assert!(
        filled > 0,
        "no cluster holds the light: the light is at the origin, in front of the \
         camera, and it must reach the clusters around the middle of the screen"
    );
    assert!(
        filled < clusters,
        "every cluster holds the light: a light with a radius of 20 in front of \
         the camera cannot reach the far corner of the screen"
    );

    // The decisive check: the cluster the light *must* be in.
    //
    // The light is at the origin, the camera looks at the origin, so the light
    // is at the centre of the screen: the tile (2, 2) of a 4x4 grid, and the
    // slice 0, because the light is three units from the camera and the slices
    // divide the distance linearly. The cluster index is
    // `(tile_y * tiles_x + tile_x) * slices + slice` — the same formula the
    // shader uses, and the one the forward shader has to reproduce from the
    // fragment's world position.
    //
    // Without this check the test only says "some cluster somewhere holds the
    // light", which is true even when the two sides of the pipeline disagree
    // about which cluster is which — and that disagreement is exactly what a
    // forward shader reading the clusters has to get right.
    let tiles_x = config.tiles_x as usize;
    let tiles_y = config.tiles_y as usize;
    let slices = config.depth_slices as usize;
    let tile_x = 2usize.min(tiles_x - 1);
    let tile_y = 2usize.min(tiles_y - 1);
    let slice = 0usize;
    let expected = (tile_y * tiles_x + tile_x) * slices + slice;

    let count = u32::from_le_bytes([
        count_bytes[expected * 4],
        count_bytes[expected * 4 + 1],
        count_bytes[expected * 4 + 2],
        count_bytes[expected * 4 + 3],
    ]);
    assert_eq!(
        count, 1,
        "the cluster the light must be in ({expected}) holds {count} lights: the \
         clustering and the forward shader have to agree on how a cluster is \
         numbered"
    );
    let at = expected * rfs_client::render::passes::cluster::MAX_LIGHTS_PER_CLUSTER as usize;
    let light_index = u32::from_le_bytes([
        index_bytes[at * 4],
        index_bytes[at * 4 + 1],
        index_bytes[at * 4 + 2],
        index_bytes[at * 4 + 3],
    ]);
    assert_eq!(
        light_index, 0,
        "the cluster the light must be in ({expected}) holds the light {light_index}, \
         and there is only one"
    );
}
