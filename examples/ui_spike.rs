//! The UI integration spike: does a UI rectangle reach the screen?
//!
//! Run with:
//!
//! ```text
//! cargo run --example ui_spike
//! ```
//!
//! Every UI test in this repository answers a question about arithmetic: how many
//! vertices a batch produced, whether a glyph wrapped, whether a click landed on
//! a button. None of them answer the only question that matters, which is whether
//! a pixel arrives. So this example exists to settle that, and to keep settling it
//! afterwards.
//!
//! It draws exactly one thing: a green rectangle in the top-left corner, via the
//! real pass, the real render graph, and the real presentation path.
//!
//! ```text
//! UIPass -> RenderGraph -> Renderer -> acquire -> submit -> present
//! ```
//!
//! What to look for:
//!
//!  - a green rectangle in the top-left of the window, and
//!  - one `[ui] first frame recorded at WxH: ...` line on stderr.
//!
//! A run that prints nothing from `[ui]` has told you the UI is not drawing,
//! which is exactly the fact this example was written to surface. The window
//! deliberately renders nothing behind it, so anything visible came from the UI
//! and nothing else.
//!
//! Esc quits. Resizing rebuilds the renderer, because the projection is derived
//! from the physical extent and cannot be reused across a scale change.

use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::passes::ui::{UIConfig, UIPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::Renderer;
use rfs_client::RhiResult;
use rhi::RhiConfig;
// raw-window-handle 0.6 hands back a generic handle; on Windows the HINSTANCE only
// lives on the window handle.
use winit::raw_window_handle::HasWindowHandle;

fn main() {
    if let Err(e) = run() {
        eprintln!("ui_spike example failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> RhiResult<()> {
    let event_loop = winit::event_loop::EventLoop::new()
        .map_err(|e| rhi::RhiError::BackendError(format!("event loop: {e}")))?;
    let window = winit::window::WindowBuilder::new()
        .with_title("RFS - UI spike (expect a green rectangle top-left)")
        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
        .build(&event_loop)
        .map_err(|e| rhi::RhiError::BackendError(format!("window: {e}")))?;

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

    let mut renderer = Renderer::new(
        std::ptr::null_mut(),
        RhiConfig {
            // Validation names the exact misuse before it becomes a crash or a
            // blank frame. A blank frame is otherwise indistinguishable from
            // "the projection is wrong", which is the very thing under test.
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

    let extent = renderer.presentation_extent();
    println!(
        "presenting {}x{} physical - expect a green rectangle top-left, Esc to quit",
        extent.width, extent.height
    );
    println!("if nothing appears, the [ui] lines on stderr say why");

    renderer.render_graph_mut().add_pass(RenderPassNode {
        name: "ui".into(),
        pass: Box::new(UIPass::new(UIConfig::default())),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });

    let mut scene = Scene::new("ui_spike");
    renderer.initialize(&mut scene);

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
                        // The swapchain cannot be rebuilt from inside the event
                        // reliably, and a rebuild is a Vulkan call, not a repaint.
                        renderer.resize(size.width.max(1), size.height.max(1));
                    }
                    winit::event::WindowEvent::ScaleFactorChanged { .. } => {
                        // Going fullscreen changes the scale factor as well as the
                        // size, and winit delivers that separately.
                        let size = window.inner_size();
                        renderer.resize(size.width.max(1), size.height.max(1));
                    }
                    _ => {}
                },
                winit::event::Event::AboutToWait => {
                    // A minimised window reports a zero-sized swapchain, and
                    // acquiring or presenting there is not valid.
                    let extent = renderer.presentation_extent();
                    if extent.width != 0 && extent.height != 0 {
                        renderer.render_frame(&mut scene);
                    }
                }
                _ => {}
            }
        })
        .map_err(|e| rhi::RhiError::BackendError(format!("event loop: {e}")))?;

    Ok(())
}