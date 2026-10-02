//! A real cube, drawn by the render graph, presented to a real window.
//!
//! Run with:
//!
//! ```text
//! cargo run --example cube
//! ```
//!
//! This is the first example that renders actual geometry through the whole
//! stack rather than through a hand-written frame:
//!
//! ```text
//! GeometryPass -> RenderGraph -> Renderer -> acquire -> submit -> present
//! ```
//!
//! Everything the pass needs is built by `GeometryPass::create`; the example
//! only supplies the mesh and the compiled shader. Esc quits, and dragging the
//! window rebuilds the swapchain, the framebuffer and the viewport.
//!
//! The cube turns slowly so a frozen frame is distinguishable from a static
//! scene, which matters when the only symptom of a broken submit is a
//! picture that never changes.

use std::time::Duration;

use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::{GeometryItem, GeometryPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::Renderer;
use rfs_client::RhiResult;
use rhi::RhiConfig;
use winit::raw_window_handle::HasWindowHandle;

/// Compiled from `Tool/client_tests/assets/shaders/geometry.wgsl` by
/// `cargo test -p client_tests --test compile_shaders -- --ignored`.
const SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/geometry.spv");

/// How far the camera sits from the cube, and how long a turn takes.
const DISTANCE: f32 = 3.2;
const TURN_SECONDS: f32 = 8.0;

fn main() {
    if let Err(e) = run() {
        eprintln!("cube example failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> RhiResult<()> {
    let event_loop = winit::event_loop::EventLoop::new()
        .map_err(|e| rhi::RhiError::BackendError(format!("event loop: {e}")))?;
    let window = winit::window::WindowBuilder::new()
        .with_title("RFS - geometry pass")
        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
        .build(&event_loop)
        .map_err(|e| rhi::RhiError::BackendError(format!("window: {e}")))?;

    // raw-window-handle 0.6 hands back a generic handle; on Windows the HINSTANCE
    // only lives on the window handle, and the display handle carries nothing.
    let window_handle = window
        .window_handle()
        .map_err(|e| rhi::RhiError::BackendError(format!("window handle: {e}")))?;
    let (hwnd, hinstance) = match window_handle.as_raw() {
        winit::raw_window_handle::RawWindowHandle::Win32(win32) => (
            win32.hwnd.get() as u64,
            win32.hinstance.map(|h| h.get() as u64).ok_or_else(|| {
                rhi::RhiError::BackendError("Win32 window handle carries no HINSTANCE".into())
            })?,
        ),
        other => {
            return Err(rhi::RhiError::BackendError(format!(
                "expected a Win32 window handle, got {other:?}"
            )));
        }
    };

    // The window argument is unused by `new`: the surface is attached afterwards
    // from the handles, because a window is not always available (tests and
    // offscreen rendering never have one).
    let mut renderer = Renderer::new(std::ptr::null_mut(), RhiConfig::default())?;
    let size = window.inner_size();
    renderer.attach_presentation(
        hinstance,
        hwnd,
        size.width.max(1),
        size.height.max(1),
    )?;

    // The swapchain chose the format, so the pass has to be built for it.
    let Some(format) = renderer.presentation_format() else {
        return Err(rhi::RhiError::BackendError(
            "the surface format cannot be expressed by the RHI".into(),
        ));
    };

    let mut mesh = Mesh::cube("cube", 1.0);
    mesh.create_buffers(renderer.device())?;
    let vertex_buffer = mesh
        .vertex_buffer()
        .cloned()
        .ok_or_else(|| rhi::RhiError::BackendError("no vertex buffer".into()))?;
    let index_buffer = mesh
        .index_buffer()
        .cloned()
        .ok_or_else(|| rhi::RhiError::BackendError("no index buffer".into()))?;

    // Registered after `attach_presentation`, not before: the pass needs the
    // format that the surface settled on.
    let (pass, state) = GeometryPass::create(
        renderer.device(),
        vec![GeometryItem {
            name: "cube".into(),
            vertex_buffer,
            index_buffer,
            index_type: mesh.index_type(),
            index_count: mesh.index_count(),
            model: glam::Mat4::IDENTITY,
        }],
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

    let mut scene = Scene::new("cube");
    renderer.initialize(&mut scene);

    let extent = renderer.presentation_extent();
    println!(
        "presenting {}x{} as {:?} - Esc to quit",
        extent.width, extent.height, format
    );

    let start = std::time::Instant::now();
    event_loop
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
                        // Rebuilt lazily on the next frame: the swapchain cannot
                        // be recreated from inside the resize event reliably,
                        // and a rebuild is a Vulkan call, not a repaint.
                        renderer.resize(size.width.max(1), size.height.max(1));
                    }
                    _ => {}
                },
                winit::event::Event::AboutToWait => {
                    // The cube turns, so a stuck submit is visible as a frozen
                    // picture rather than passing for a static scene.
                    let angle = (start.elapsed().as_secs_f32() / TURN_SECONDS)
                        * std::f32::consts::TAU;
                    let view = glam::Mat4::look_at_rh(
                        glam::Vec3::new(
                            DISTANCE * angle.sin(),
                            1.4,
                            DISTANCE * angle.cos(),
                        ),
                        glam::Vec3::ZERO,
                        glam::Vec3::Y,
                    );
                    let extent = renderer.presentation_extent();
                    // The aspect ratio has to come from the live extent or the
                    // cube is stretched on any window that is not 16:9.
                    let aspect = if extent.height == 0 {
                        16.0 / 9.0
                    } else {
                        extent.width as f32 / extent.height as f32
                    };
                    let projection =
                        glam::Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, aspect, 0.1, 100.0);
                    // Skip the frame rather than draw it under the previous frame's matrix.
                    let uniforms_ok = GeometryPass::update_uniforms(
                        &state,
                        renderer.device(),
                        view,
                        projection,
                    );

                    if uniforms_ok {
                        renderer.render_frame(&mut scene);
                    }
                }
                _ => {}
            }
        })
        .map_err(|e| rhi::RhiError::BackendError(format!("event loop: {e}")))?;

    let _ = Duration::ZERO;
    Ok(())
}
