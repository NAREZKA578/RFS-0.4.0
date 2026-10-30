//! The product's path, with particles.
//!
//! Three passes in one graph, each fed by the layer that owns its input:
//! `GeometryPass` draws what the scene says (`items_for_scene`, the bridge
//! `scene.rs` proves), `ParticlePass` draws what the system uploaded
//! (`update` -> upload -> draw), and `ParticleCompositePass` blends the
//! particle pass's own image back over the frame — the trail becomes part of
//! the picture and reaches the screen through the same present as everything
//! else.
//!
//! The emitter is the part the caller owns: two engine emitters, one per
//! ship, moved every frame to wherever the scene's transforms put the ships.
//! The system uploads what is live, the pass draws it, the composite blends
//! it — and nothing else in this example touches particles.
//!
//! One size caveat, stated rather than hidden: the particle pass builds its
//! own image at the presentation extent it was created for, so a resize
//! after that leaves the trails at the old resolution — stretched by the
//! composite's copy, not wrong, just soft. Rebuilding the pass on resize is
//! the caller's job and the example keeps it simple on purpose.
//!
//! Run with `RFS_FRAME_LIMIT=120` to render 120 frames and exit.

use std::sync::{Arc, Mutex};

use glam::{Mat4, Vec2, Vec3, Vec4};
use rfs_client::render::graph::node::{PassDependency, RenderPassNode};
use rfs_client::render::graph::types::ResourceUsage;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::GeometryPass;
use rfs_client::render::passes::particle_composite::ParticleCompositePass;
use rfs_client::render::passes::particles::{ParticleConfig, ParticlePass};
use rfs_client::render::particles::{ParticleEmitter, ParticleEmitterConfig, ParticleSystem};
use rfs_client::render::scene::components::MeshComponent;
use rfs_client::render::scene::entity::Entity;
use rfs_client::render::scene::Scene;
use rfs_client::render::{EmitterShape, Renderer};
use rhi::{RhiConfig, RhiError, RhiResult};
use winit::raw_window_handle::HasWindowHandle;

const GEOMETRY_SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/geometry.spv");
const PARTICLE_SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/particle.spv");
// The composite draws no shader of its own: it reuses the resolve program —
// one oversized triangle from the vertex index, the source read with
// `textureLoad`.
const COMPOSITE_SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/composite.spv");

const DISTANCE: f32 = 9.0;
const TURN_SECONDS: f32 = 24.0;

fn main() {
    if let Err(e) = run() {
        eprintln!("particles example failed: {e}");
        std::process::exit(1);
    }
}

/// One engine: an emitter whose position follows a ship, emitting backwards.
///
/// The rate is what keeps the trail alive — a `burst` would die out in one
/// lifetime — and the lifetime is what makes it short enough to show the
/// ship's own motion: the oldest particle fades before the ship has turned
/// noticeably.
fn engine(name: &str) -> ParticleEmitter {
    ParticleEmitter::new(ParticleEmitterConfig {
        name: name.into(),
        max_particles: 256,
        emission_rate: 40.0,
        particle_lifetime: 1.5,
        particle_lifetime_variation: 0.3,
        particle_size: Vec2::new(0.25, 0.25),
        particle_size_variation: Vec2::new(0.08, 0.08),
        particle_color: Vec4::new(1.0, 0.6, 0.2, 1.0),
        particle_color_variation: Vec4::ZERO,
        particle_velocity: Vec3::new(0.0, 0.0, -2.5),
        particle_velocity_variation: Vec3::new(0.3, 0.3, 0.5),
        emitter_shape: EmitterShape::Sphere,
        emitter_size: Vec3::new(0.15, 0.15, 0.15),
        gravity: Vec3::ZERO,
        drag: 1.0,
        rotation_speed: 0.0,
        rotation_speed_variation: 0.0,
        ..Default::default()
    })
}

fn run() -> RhiResult<()> {
    let event_loop = winit::event_loop::EventLoop::new()
        .map_err(|e| RhiError::BackendError(format!("event loop: {e}")))?;
    let window = winit::window::WindowBuilder::new()
        .with_title("RFS - ships with particle trails")
        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
        .build(&event_loop)
        .map_err(|e| RhiError::BackendError(format!("window: {e}")))?;

    // raw-window-handle 0.6 hands back a generic handle; on Windows the HINSTANCE
    // only lives on the window handle, and the display handle carries nothing.
    let window_handle = window
        .window_handle()
        .map_err(|e| RhiError::BackendError(format!("window handle: {e}")))?;
    let (hwnd, hinstance) = match window_handle.as_raw() {
        winit::raw_window_handle::RawWindowHandle::Win32(win32) => (
            win32.hwnd.get() as u64,
            win32.hinstance.map(|h| h.get() as u64).ok_or_else(|| {
                RhiError::BackendError("Win32 window handle carries no HINSTANCE".into())
            })?,
        ),
        other => {
            return Err(RhiError::BackendError(format!(
                "expected a Win32 window handle, got {other:?}"
            )));
        }
    };

    let mut renderer = Renderer::new(
        std::ptr::null_mut(),
        RhiConfig {
            validation: true,
            ..RhiConfig::default()
        },
    )?;
    let size = window.inner_size();
    renderer.attach_presentation(
        hinstance,
        hwnd,
        size.width.max(1),
        size.height.max(1),
    )?;

    // The swapchain chose the format, so the passes have to be built for it.
    let Some(format) = renderer.presentation_format() else {
        return Err(RhiError::BackendError(
            "the surface format cannot be expressed by the RHI".into(),
        ));
    };

    // The scene: one mesh, two ships, the scene says where they are.
    let mut mesh = Mesh::cube("hull", 1.0);
    mesh.create_buffers(renderer.device())?;
    let mesh = Arc::new(mesh);

    let mut scene = Scene::new("two ships with trails");
    for (name, x) in [("port", -2.5), ("starboard", 2.5)] {
        let mut entity = Entity::new(name);
        entity.add_component(
            rfs_client::render::scene::Transform::with_position(Vec3::new(x, 0.0, 0.0)),
        );
        entity.add_component(MeshComponent::new(mesh.clone()));
        scene.add_entity(entity);
    }

    // And the render layer is asked what to draw, rather than being told.
    let built = GeometryPass::items_for_scene(&scene);
    if !built.skipped.is_empty() {
        for (name, reason) in &built.skipped {
            eprintln!("[particles] {name} cannot be drawn: {reason}");
        }
    }
    if built.items.is_empty() {
        return Err(RhiError::BackendError(
            "the scene produced nothing to draw".into(),
        ));
    }
    println!(
        "[particles] {} item(s) from the scene, {} skipped",
        built.items.len(),
        built.skipped.len()
    );

    let (geometry_pass, geometry_state) = GeometryPass::create(
        renderer.device(),
        built.items,
        format,
        GEOMETRY_SHADER,
    )?;

    // The particle system, sized for both engines at full rate: two emitters
    // at 40/s over a 1.8 s worst-case lifetime is 144 live particles, and a
    // buffer smaller than the emitters produce would silently drop the
    // excess (the count is the buffer's truth, not the pools').
    let system = Arc::new(Mutex::new(ParticleSystem::new(
        renderer.device().clone(),
        256,
    )));
    {
        let mut system = system.lock().expect("particle system");
        system.add_emitter(engine("port"));
        system.add_emitter(engine("starboard"));
    }

    // The particle pass draws into its own image, at the presentation's size.
    let extent = renderer.presentation_extent();
    let (particle_pass, particle_state) = ParticlePass::create(
        renderer.device(),
        system.clone(),
        ParticleConfig {
            width: extent.width.max(1),
            height: extent.height.max(1),
        },
        PARTICLE_SHADER,
    )?;

    // The composite reads the particle pass's image back over the frame. Its
    // pipeline is built for the presentation format; the target it draws over
    // is the acquired image, taken through the context — no pinned
    // framebuffer here, this is the presentation variant.
    let (composite, _composite_state) = ParticleCompositePass::create(
        renderer.device(),
        particle_state.clone(),
        format,
        COMPOSITE_SHADER,
    )?;

    renderer.render_graph_mut().add_pass(RenderPassNode {
        name: "geometry".into(),
        pass: Box::new(geometry_pass),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });
    // The particle pass draws into its own image, not the swapchain — its
    // output names the image the composite reads, which is also what the
    // composite's dependency is written against.
    renderer.render_graph_mut().add_pass(RenderPassNode {
        name: "particles".into(),
        pass: Box::new(particle_pass),
        dependencies: Vec::new(),
        outputs: vec!["particles".into()],
        enabled: true,
        execution_order: 1,
    });
    renderer.render_graph_mut().add_pass(RenderPassNode {
        name: "particles_composite".into(),
        pass: Box::new(composite),
        dependencies: vec![PassDependency::new(
            "particles",
            "particles",
            ResourceUsage::Sampled,
        )],
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 2,
    });

    let start = std::time::Instant::now();
    let mut last_frame = start;
    let frame_limit: Option<u32> = std::env::var("RFS_FRAME_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok());
    let mut frames_done: u32 = 0;

    // The event loop returns an error on some platforms; the cube example
    // ignores it too, and the frame limit is how a run is ended on purpose.
    let _ = event_loop
        .run(move |event, control_flow| {
            control_flow.set_control_flow(winit::event_loop::ControlFlow::Poll);

            match event {
                winit::event::Event::WindowEvent { event, .. } => match event {
                    winit::event::WindowEvent::CloseRequested => control_flow.exit(),
                    winit::event::WindowEvent::KeyboardInput { event, .. }
                        if event.logical_key
                            == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) =>
                    {
                        control_flow.exit()
                    }
                    winit::event::WindowEvent::Resized(size) => {
                        renderer.resize(size.width.max(1), size.height.max(1));
                    }
                    winit::event::WindowEvent::ScaleFactorChanged { .. } => {
                        let size = window.inner_size();
                        renderer.resize(size.width.max(1), size.height.max(1));
                    }
                    _ => {}
                },
                winit::event::Event::AboutToWait => {
                    let extent = renderer.presentation_extent();
                    if extent.width != 0 && extent.height != 0 {
                        // The delta this frame's emitters live on: the system
                        // ages its particles by exactly this, and the trail's
                        // length is the lifetime measured in it.
                        let now = std::time::Instant::now();
                        let delta = now.duration_since(last_frame);
                        last_frame = now;

                        // The ships sail, exactly as the scene example does.
                        let elapsed = start.elapsed().as_secs_f32();
                        let angle = (elapsed / TURN_SECONDS) * std::f32::consts::TAU;
                        let sway = angle.sin() * 1.5;
                        for (index, entity) in scene.entities_mut().iter_mut().enumerate() {
                            let side = if index == 0 { -1.0 } else { 1.0 };
                            if let Some(transform) = entity.transform_mut() {
                                transform.set_position(Vec3::new(
                                    side * 2.5,
                                    sway * 0.3,
                                    sway,
                                ));
                            }
                        }
                        let moved = GeometryPass::update_models(&geometry_state, &scene);
                        debug_assert!(moved > 0, "the scene's transforms did not reach the pass");

                        // The emitters follow their ships: the engine's
                        // position is the ship's position, so the trail starts
                        // where the hull is. Without this the trail stays
                        // behind at the spawn-time positions and the picture
                        // shows the two disconnected.
                        {
                            let mut system = system.lock().expect("particle system");
                            for (index, emitter_index) in [(0usize, 0usize), (1usize, 1usize)] {
                                let position = scene.entities()[index]
                                    .transform()
                                    .map(|t| t.position())
                                    .unwrap_or(Vec3::ZERO);
                                system
                                    .get_emitter_mut(emitter_index)
                                    .expect("emitter")
                                    .set_position(position);
                            }
                            system.update(delta);
                        }

                        // One camera for both passes: the ships and their
                        // trails are in one picture, which only holds if the
                        // billboards project through the same matrices the
                        // meshes do.
                        let view = Mat4::look_at_rh(
                            Vec3::new(
                                DISTANCE * angle.sin(),
                                3.0,
                                DISTANCE * angle.cos(),
                            ),
                            Vec3::ZERO,
                            Vec3::Y,
                        );
                        let aspect = extent.width as f32 / extent.height as f32;
                        let projection = Mat4::perspective_rh(
                            std::f32::consts::FRAC_PI_4,
                            aspect,
                            0.1,
                            100.0,
                        );
                        let geometry_ready = GeometryPass::update_uniforms(
                            &geometry_state,
                            renderer.device(),
                            view,
                            projection,
                        );
                        let particles_ready = ParticlePass::update_uniforms(
                            &particle_state,
                            renderer.device(),
                            view,
                            projection,
                        );
                        if geometry_ready && particles_ready {
                            renderer.render_frame(&mut scene);
                        }
                    }

                    frames_done += 1;
                    if let Some(limit) = frame_limit {
                        if frames_done >= limit {
                            control_flow.exit();
                        }
                    }
                }
                _ => {}
            }
        });

    Ok(())
}
