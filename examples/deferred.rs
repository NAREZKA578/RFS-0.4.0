//! The deferred chain, end to end, on the screen.
//!
//! `cube.rs` draws the scene the forward way: one geometry pass straight into
//! the swapchain. This example runs the *deferred* way, and it is the first
//! thing in the repository where three passes hand work to each other:
//!
//! ```text
//!   scene ──▶ GBuffer ──▶ lighting ──▶ resolve ──▶ swapchain
//! ```
//!
//! - The **GBuffer** fills four targets: world position, normal, albedo and
//!   material.
//! - The **lighting** pass reads them, adds the scene's lights, and writes a
//!   lit image.
//! - The **resolve** pass copies that image into the image the renderer
//!   acquired.
//!
//! The chain is only as good as its edges, and two of them are easy to get
//! wrong and were wrong here first:
//!
//! - **Order is not a memory dependency.** A render pass boundary does not
//!   make one pass's writes visible to the next pass's sampled reads. Without
//!   an explicit barrier the lighting pass reads the GBuffer while it is still
//!   being written, and the picture changes from run to run. The lighting and
//!   resolve passes each put a barrier in front of their reads.
//! - **The pass must name both ends of the edge.** The graph orders the passes
//!   by the *producer's* name and binds the *resource* by its own name, so a
//!   resolve pass that named the wrong producer would be ordered wrongly and
//!   copy a texture nobody has written.
//!
//! Run it with `RFS_FRAME_LIMIT=120` to render 120 frames and exit.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::lighting::light::{Light, LightType};
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::gbuffer::GBufferPass;
use rfs_client::render::passes::geometry::GeometryPass;
use rfs_client::render::passes::lighting::LightingPass;
use rfs_client::render::passes::resolve::ResolvePass;
use rfs_client::render::scene::components::{LightComponent, MeshComponent};
use rfs_client::render::scene::entity::Entity;
use rfs_client::render::scene::Scene;
use rfs_client::render::Renderer;
use rhi::{RhiConfig, RhiError, RhiResult};
use winit::raw_window_handle::HasWindowHandle;

const GBUFFER_SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/gbuffer.spv");
const LIGHTING_SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/lighting.spv");
const RESOLVE_SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/resolve.spv");

const DISTANCE: f32 = 7.0;
const TURN_SECONDS: f32 = 20.0;

fn main() {
    if let Err(e) = run() {
        eprintln!("deferred example failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> RhiResult<()> {
    let event_loop = winit::event_loop::EventLoop::new()
        .map_err(|e| RhiError::BackendError(format!("event loop: {e}")))?;
    let window = winit::window::WindowBuilder::new()
        .with_title("RFS - the deferred chain")
        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
        .build(&event_loop)
        .map_err(|e| RhiError::BackendError(format!("window: {e}")))?;

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

    // The surface's format: both the resolve pipeline and the render pass it
    // draws into have to agree with it, and a pipeline that declares a
    // different colour format than its framebuffer's is undefined behaviour
    // rather than a warning.
    let Some(format) = renderer.presentation_format() else {
        return Err(RhiError::BackendError(
            "the surface format cannot be expressed by the RHI".into(),
        ));
    };

    // The scene: a hull and a light.
    let mut mesh = Mesh::cube("hull", 1.4);
    mesh.create_buffers(renderer.device())?;
    let mesh = Arc::new(mesh);

    let mut scene = Scene::new("deferred");
    let mut hull = Entity::new("hull");
    hull.add_component(rfs_client::render::scene::Transform::new());
    hull.add_component(MeshComponent::new(mesh));
    scene.add_entity(hull);

    let mut sun = Entity::new("sun");
    sun.add_component(rfs_client::render::scene::Transform::with_position(Vec3::new(
        4.0, 5.0, 6.0,
    )));
    sun.add_component(LightComponent::new(
        Light::new("sun", LightType::Point)
            .with_color(Vec3::new(1.0, 0.95, 0.85))
            .with_intensity(1.4)
            .with_radius(60.0),
    ));
    scene.add_entity(sun);

    // The render layer is asked what to draw.
    let built = GeometryPass::items_for_scene(&scene);
    if !built.skipped.is_empty() {
        for (name, reason) in &built.skipped {
            eprintln!("[deferred] {name} cannot be drawn: {reason}");
        }
    }
    if built.items.is_empty() {
        return Err(RhiError::BackendError(
            "the scene produced nothing to draw".into(),
        ));
    }

    let (gbuffer, gbuffer_state) = GBufferPass::create(renderer.device(), built.items, GBUFFER_SHADER)?;
    let (lighting, lighting_state) =
        LightingPass::create(renderer.device(), Default::default(), LIGHTING_SHADER)?;
    // The resolve pass copies the lighting pass's result into the image the
    // renderer acquired. It names both ends of the edge: the resource it
    // copies and the pass that produces it.
    let (resolve, _resolve_state) = ResolvePass::create(
        renderer.device(),
        format,
        RESOLVE_SHADER,
        "lighting",
        "lighting",
    )?;

    let graph = renderer.render_graph_mut();
    graph.add_pass(RenderPassNode::new("gbuffer", Box::new(gbuffer)));
    graph.add_pass(RenderPassNode::new("lighting", Box::new(lighting)));
    graph.add_pass(RenderPassNode::new("resolve", Box::new(resolve)));

    // The graph's initialization is what creates the resources the passes
    // name and hands them over. Without it the passes have no targets, the
    // frame records nothing, and the graph says so.
    //
    // The renderer initializes the graph itself on the first frame, so this
    // call is not strictly needed — but it is the explicit form, and it is
    // here because the *order* matters: the passes have to be in the graph
    // before it is initialized, or they would be left without their
    // resources.
    renderer.initialize(&mut scene);

    let start = std::time::Instant::now();
    let frame_limit: Option<u32> = std::env::var("RFS_FRAME_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok());
    let mut frames_done: u32 = 0;
    // The image the renderer acquired last, so the example can report a change
    // rather than a number every frame.
    let mut last_image: u32 = u32::MAX;

    // `RFS_RESIZE_SELFCHECK=<n>` resizes the window `n` times, one per frame,
    // and prints the validator's error count after each step.
    //
    // The count is the point. A resize changes the size of every screen-sized
    // target, and the chain has to rebuild them; if it does not, the frame is
    // recorded against attachments of the old size and the validator says so.
    // The count at `vkDestroyDevice` would not: it prints the final number and
    // says nothing about whether it grew while the program ran.
    let resize_steps: Option<usize> = std::env::var("RFS_RESIZE_SELFCHECK")
        .ok()
        .and_then(|v| v.parse().ok());
    if let Some(steps) = resize_steps {
        eprintln!("[deferred] resize self-check: {steps} steps");
    }
    const SELFCHECK_WARMUP_FRAMES: u32 = 5;
    let mut resize_step: usize = 0;

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
                    // The resize happens before the frame is drawn, so the frame
                    // being drawn is the one that has to cope with the new size.
                    if let Some(steps) = resize_steps {
                        if resize_step < steps && frames_done >= SELFCHECK_WARMUP_FRAMES {
                            let width = 640 + (resize_step as u32) * 37;
                            let height = 480 + (resize_step as u32) * 23;
                            let before = rhi::validation_error_count();
                            // The *window* is resized, not just the logical
                            // size the renderer is told about. The surface's
                            // extent is what the swapchain is built from, and
                            // it only changes when the window itself changes:
                            // a self-check that only calls `renderer.resize`
                            // rebuilds the chain at the old size and never
                            // exercises the path a user takes.
                            let _ = window.request_inner_size(winit::dpi::LogicalSize::new(
                                width, height,
                            ));
                            resize_step += 1;
                            // The count is printed after the frame, because the
                            // frame is where the stale attachments are used.
                            let after = rhi::validation_error_count();
                            eprintln!(
                                "[deferred] resize step {resize_step}: {width}x{height}, \
                                 validator errors {before} -> {after}"
                            );
                        }
                    }
                    let extent = renderer.presentation_extent();
                    if extent.width != 0 && extent.height != 0 {
                        // The hull turns; the light stands still.
                        let angle =
                            (start.elapsed().as_secs_f32() / TURN_SECONDS) * std::f32::consts::TAU;
                        for entity in scene.entities_mut().iter_mut() {
                            if entity.name() != "hull" {
                                continue;
                            }
                            if let Some(transform) = entity.transform_mut() {
                                transform.set_rotation(glam::Quat::from_rotation_y(angle));
                            }
                        }
                        GBufferPass::update_models(&gbuffer_state, &scene);

                        let camera = Vec3::new(
                            DISTANCE * angle.sin(),
                            2.5,
                            DISTANCE * angle.cos(),
                        );
                        let view = Mat4::look_at_rh(camera, Vec3::ZERO, Vec3::Y);
                        let aspect = extent.width as f32 / extent.height as f32;
                        let projection = Mat4::perspective_rh(
                            std::f32::consts::FRAC_PI_4,
                            aspect,
                            0.1,
                            100.0,
                        );

                        let uniforms_ok = GBufferPass::update_uniforms(
                            &gbuffer_state,
                            renderer.device(),
                            view,
                            projection,
                        );
                        let light_count = LightingPass::update_lights(
                            &lighting_state,
                            renderer.device(),
                            &scene,
                        );
                        let lighting_ok = LightingPass::update_uniforms(
                            &lighting_state,
                            renderer.device(),
                            projection * view,
                            camera,
                            light_count as u32,
                        );
                        if uniforms_ok && lighting_ok {
                            renderer.render_frame(&mut scene);
                            // The acquired image has to cycle. A chain that keeps
                            // handing out the same image is a chain whose
                            // presents never release it, and that is the
                            // symptom №282 describes.
                            let image = renderer.context().swapchain_image_index;
                            if image != last_image {
                                last_image = image;
                                eprintln!("[deferred] frame {frames_done}: image {image}");
                            }
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
