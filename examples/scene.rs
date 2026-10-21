//! The product's path: a scene in, a picture out.
//!
//! `cube.rs` builds its draw items by hand, one mesh and an identity model
//! matrix, because that was the only way the render layer offered: the scene
//! held entities, transforms and meshes, and nothing read them. `Scene::entities`,
//! `Entity::transform`, `Entity::mesh` and `Transform::matrix` had no callers
//! at all.
//!
//! This example uses the bridge instead. It builds a scene with two ships at
//! different places, asks the render layer what to draw
//! (`GeometryPass::items_for_scene`), and lets the scene's transforms move the
//! ships every frame (`GeometryPass::update_models`). The two cubes are the
//! proof: they share one mesh, one uniform buffer each and one draw call
//! each, and they are in different places on the screen — which only works if
//! the per-item model matrix reaches the GPU.
//!
//! Run it with `RFS_FRAME_LIMIT=120` to render 120 frames and exit.

use std::sync::Arc;

use glam::Vec3;
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::GeometryPass;
use rfs_client::render::scene::components::MeshComponent;
use rfs_client::render::scene::entity::Entity;
use rfs_client::render::scene::Scene;
use rfs_client::render::Renderer;
use rhi::{RhiConfig, RhiError, RhiResult};
use winit::raw_window_handle::HasWindowHandle;

const SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/geometry.spv");
const DISTANCE: f32 = 9.0;
const TURN_SECONDS: f32 = 24.0;

fn main() {
    if let Err(e) = run() {
        eprintln!("scene example failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> RhiResult<()> {
    let event_loop = winit::event_loop::EventLoop::new()
        .map_err(|e| RhiError::BackendError(format!("event loop: {e}")))?;
    let window = winit::window::WindowBuilder::new()
        .with_title("RFS - the scene, drawn")
        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
        .build(&event_loop)
        .map_err(|e| RhiError::BackendError(format!("window: {e}")))?;

    // raw-window-handle 0.6 hands back a generic handle; on Windows the HINSTANCE
    // only lives on the window handle, and the display handle carries nothing.
    let window_handle = window
        .window_handle()
        .map_err(|e| RhiError::BackendError(format!("window handle: {e}")))?;    let (hwnd, hinstance) = match window_handle.as_raw() {
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

    // The swapchain chose the format, so the pass has to be built for it.
    let Some(format) = renderer.presentation_format() else {
        return Err(RhiError::BackendError(
            "the surface format cannot be expressed by the RHI".into(),
        ));
    };

    // The scene. One mesh, two ships, and the scene says where they are.
    let mut mesh = Mesh::cube("hull", 1.0);
    mesh.create_buffers(renderer.device())?;
    let mesh = Arc::new(mesh);

    let mut scene = Scene::new("two ships");
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
            eprintln!("[scene] {name} cannot be drawn: {reason}");
        }
    }
    if built.items.is_empty() {
        return Err(RhiError::BackendError(
            "the scene produced nothing to draw".into(),
        ));
    }
    println!(
        "[scene] {} item(s) from the scene, {} skipped",
        built.items.len(),
        built.skipped.len()
    );

    let (pass, state) = GeometryPass::create(
        renderer.device(),
        built.items,
        format,
        SHADER,
    )?;
    renderer.render_graph_mut().add_pass(RenderPassNode {
        name: "geometry".into(),
        pass: Box::new(pass),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });

    let start = std::time::Instant::now();
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
                        // The ships sail. The scene owns their positions, so this
                        // is the scene's business, not the pass's: the render
                        // layer only has to notice that the transforms moved.
                        let angle = (start.elapsed().as_secs_f32() / TURN_SECONDS)
                            * std::f32::consts::TAU;
                        let sway = (angle.sin()) * 1.5;
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
                        // And the pass picks the new transforms up.
                        let moved = GeometryPass::update_models(&state, &scene);
                        debug_assert!(moved > 0, "the scene's transforms did not reach the pass");

                        let view = glam::Mat4::look_at_rh(
                            glam::Vec3::new(
                                DISTANCE * angle.sin(),
                                3.0,
                                DISTANCE * angle.cos(),
                            ),
                            glam::Vec3::ZERO,
                            glam::Vec3::Y,
                        );
                        let aspect = extent.width as f32 / extent.height as f32;
                        let projection = glam::Mat4::perspective_rh(
                            std::f32::consts::FRAC_PI_4,
                            aspect,
                            0.1,
                            100.0,
                        );
                        if GeometryPass::update_uniforms(
                            &state,
                            renderer.device(),
                            view,
                            projection,
                        ) {
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