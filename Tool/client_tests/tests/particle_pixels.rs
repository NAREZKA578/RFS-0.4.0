//! The particle pass: the live particles, drawn as billboards.
//!
//! `ParticleSystem` used to be a system that could not draw. Its `render`
//! returned before recording a command — the `pipeline` field was `None`
//! forever, the comment said "This would create a pipeline", and there was no
//! particle shader among the committed ones. Every reader of the code could
//! add emitters and update the system, and every frame drew nothing, with no
//! error anywhere.
//!
//! №722 replaced the dead half with a real pass: `ParticlePass` has its own
//! image, a colour-only render pass, a pipeline whose shaders exist
//! (`particle.wgsl`, committed as `particle.spv`), and a descriptor set
//! binding the **system's own buffer** as a read-only storage buffer. The
//! draw is instanced: one instance per live particle, the particle chosen by
//! `@builtin(instance_index)`.
//!
//! The scene is built so that the drawing is the only possible cause of what
//! the image shows:
//!
//! - one emitter, a point at the origin, zero variation on everything: every
//!   particle of the burst stands at exactly `(0, 0, 0)`, with size `(0.6,
//!   0.6)` and the colour white — the burst is deterministic, and so is the
//!   picture,
//! - the camera sits at `(0, 0, 3)` and looks straight at the origin: the
//!   sprite's centre lands on the image's centre,
//! - a particle spans `2 * size` = 1.2 world units across, and the visible
//!   height at the origin's distance is about 2.49: the soft disc covers
//!   roughly a seventh of the image's width — comfortably more than the
//!   fifty pixels the assertion asks for,
//! - the corners of the image are outside every sprite (the fragment's
//!   falloff is zero past half the quad's edge): they must hold nothing but
//!   the clear colour.
//!
//! The alpha blending is part of the proof: sixty-four identical sprites
//! stack over each other, and the blend is what turns sixty-four `alpha`
//! contributions into the solid disc the assertions read. If the pass drew
//! without blending (or without the fragment's falloff) the square would show
//! and the corner assertion would catch it.
//!
//! Negative control, performed before the assertion was trusted: with the
//! pass disabled (`set_enabled(false)`) the same run drew nothing but the
//! clear colour and the fifty-pixel assertion failed with its message; the
//! enable was then restored. The test cannot lie green while the pass can
//! skip unnoticed.

use std::sync::Arc;
use std::time::Duration;

use glam::{Mat4, Vec2, Vec3, Vec4};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::particles::{ParticleEmitter, ParticleEmitterConfig, ParticleSystem};
use rfs_client::render::passes::particles::{ParticleConfig, ParticlePass};
use rfs_client::render::scene::Scene;
use rfs_client::render::{EmitterShape, RenderContext};
use rhi::core::device::DeviceDesc;
use rhi::types::{Extent2D, Features};
use rhi::RhiConfig;

const EXTENT: u32 = 64;
const PARTICLE_SHADER: &[u8] = include_bytes!("../assets/shaders/particle.spv");

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

/// One pixel of the 64x64 image.
fn pixel(bytes: &[u8], x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * EXTENT + x) * 4) as usize;
    let mut value = [0u8; 4];
    value.copy_from_slice(&bytes[offset..offset + 4]);
    value
}

#[test]
fn the_pass_draws_the_live_particles_as_a_soft_billboard() {
    let device = open_device();

    // The emitter: a point at the origin, every variation zero, so the burst
    // of sixty-four is sixty-four particles at exactly `(0, 0, 0)` — the
    // same white sprite, stacked. A stack is also a blending test: the
    // picture is sixty-four alpha contributions over the same pixels, and
    // the disc the assertions read is what the blend made of them.
    let emitter = ParticleEmitter::new(ParticleEmitterConfig {
        name: "burst".into(),
        max_particles: 64,
        emission_rate: 0.0,
        particle_lifetime: 10.0,
        particle_lifetime_variation: 0.0,
        particle_size: Vec2::new(0.6, 0.6),
        particle_size_variation: Vec2::ZERO,
        particle_color: Vec4::ONE,
        particle_color_variation: Vec4::ZERO,
        particle_velocity: Vec3::ZERO,
        particle_velocity_variation: Vec3::ZERO,
        emitter_shape: EmitterShape::Point,
        emitter_size: Vec3::ZERO,
        gravity: Vec3::ZERO,
        drag: 0.0,
        rotation_speed: 0.0,
        rotation_speed_variation: 0.0,
        ..Default::default()
    });

    // The system: the buffer holds exactly the burst's worth. The pass is
    // created first because it allocates the system's buffer if the system
    // has none — the burst and the upload that follow write into a buffer
    // that exists.
    let system = Arc::new(std::sync::Mutex::new(ParticleSystem::new(
        device.clone(),
        64,
    )));
    let (pass, state) = ParticlePass::create(
        &device,
        system.clone(),
        ParticleConfig {
            width: EXTENT,
            height: EXTENT,
        },
        PARTICLE_SHADER,
    )
    .expect("the particle pass");

    {
        let mut system = system.lock().expect("system");
        system.add_emitter(emitter);
        system.get_emitter_mut(0).expect("emitter").burst(64);
        // A zero-length step changes nothing: the positions hold, the
        // lifetimes hold, and the upload carries the burst to the GPU.
        system.update(Duration::ZERO);
    }
    assert_eq!(
        system.lock().expect("system").particle_count(),
        64,
        "the system must count the burst's live particles — the pass draws \
         exactly that many instances"
    );

    // The camera looks straight at the origin: the billboards' right and up
    // are the world's x and y, and the sprite's centre is the image's centre.
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::Y);
    let projection = Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0);
    assert!(
        ParticlePass::update_uniforms(&state, &device, view, projection),
        "the camera uniform upload must succeed"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("particles", Box::new(pass)));
    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(EXTENT, EXTENT);
    let mut scene = Scene::new("particles");

    let validation_before = rhi::validation_error_count();
    graph.execute(&device, &mut context, &mut scene);
    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while drawing the particles"
    );
    device.wait_idle().expect("wait for the frame");

    let texture = {
        let state = state.lock().expect("state");
        state.output_texture.clone().expect("the pass's image")
    };
    let bytes = device
        .download_texture(&texture, 0, 0)
        .expect("the pass's image could not be read back");

    // The disc: the stacked sprites' blended core, centred on the origin's
    // projection. Fifty pixels is far below what a 0.6-unit sprite covers at
    // this distance — the number is a floor, not the expectation.
    let lit = bright_pixels(&bytes, 0.4);
    assert!(
        lit >= 50,
        "the image holds {lit} lit pixels of {} — the pass drew nothing but \
         the clear colour, or drew it without the particles",
        EXTENT * EXTENT
    );

    // The centre of the image is the centre of the stack: sixty-four alpha
    // contributions of a full falloff — the blend saturates it to white.
    for (x, y) in [(31u32, 31u32), (32u32, 32u32)] {
        let centre = pixel(&bytes, x, y);
        assert!(
            centre.iter().all(|channel| *channel as f32 / 255.0 > 0.9),
            "the image's centre at ({x}, {y}) is {centre:?} — the stack's core \
             is not solid, so the blending is not stacking"
        );
    }

    // The corners are outside every sprite: the fragment's falloff is zero
    // past half the quad's edge, so nothing but the clear colour may be
    // there. A square without the falloff (or without the blend) would put
    // the quad's corners here.
    for (x, y) in [(0u32, 0u32), (EXTENT - 1, 0u32), (0u32, EXTENT - 1), (EXTENT - 1, EXTENT - 1)] {
        let corner = pixel(&bytes, x, y);
        assert!(
            corner.iter().all(|channel| *channel == 0),
            "the image's corner at ({x}, {y}) is {corner:?} — the sprite's \
             square shows, so the falloff is not what the shader says"
        );
    }
}
