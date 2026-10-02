//! Drives the real `Renderer` against a real window.
//!
//! Where `present.rs` proved the Vulkan path with a hand-built fullscreen
//! triangle, this exercises the actual render layer: `Renderer` builds its own
//! device, owns the window surface and swapchain, and `render_frame` runs the
//! render graph and presents.
//!
//! Run with:
//!
//! ```text
//! cargo run --example scene_frame
//! ```
//!
//! Esc quits, resizing rebuilds the presentation. It is expected to print
//! "graph recorded no commands" until the passes record real work (#186) — that
//! message is the graph telling the truth rather than silently submitting
//! nothing, which is the point of #209.

use std::time::Duration;

use rhi::GraphicsApi;
use rhi::RhiConfig;
use rfs_client::{Renderer, Scene};
use winit::raw_window_handle::HasWindowHandle;

fn main() {
    if let Err(e) = run() {
        eprintln!("scene_frame failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> rhi::RhiResult<()> {
    let event_loop = winit::event_loop::EventLoop::new().expect("event loop");
    let window = winit::window::WindowBuilder::new()
        .with_title("RFS — render layer")
        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
        .build(&event_loop)
        .expect("window");

    let window_handle = window
        .window_handle()
        .map_err(|e| rhi::RhiError::BackendError(format!("window handle: {e}")))?;
    let (hwnd, hinstance) = match window_handle.as_raw() {
        winit::raw_window_handle::RawWindowHandle::Win32(win32) => {
            let hinstance = win32.hinstance.map(|h| h.get() as u64).ok_or_else(|| {
                rhi::RhiError::BackendError("Win32 handle carries no HINSTANCE".into())
            })?;
            (win32.hwnd.get() as u64, hinstance)
        }
        other => {
            return Err(rhi::RhiError::BackendError(format!(
                "expected a Win32 window handle, got {other:?}"
            )));
        }
    };

    let config = RhiConfig { api: GraphicsApi::Vulkan, ..Default::default() };
    let mut renderer = Renderer::new(std::ptr::null_mut(), config)?;

    let size = window.inner_size();
    renderer.attach_presentation(hinstance, hwnd, size.width, size.height)?;
    println!("presentation attached: has_presentation={}", renderer.has_presentation());

    let mut scene = Scene::new("scene");
    renderer.initialize(&mut scene);


    event_loop.run(move |event, control_flow| {
        control_flow.set_control_flow(winit::event_loop::ControlFlow::Wait);

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
                    if let Err(e) =
                        renderer.attach_presentation(hinstance, hwnd, size.width, size.height)
                    {
                        eprintln!("resize presentation failed: {e}");
                        control_flow.exit();
                    }
                }
                _ => {}
            },
            winit::event::Event::AboutToWait => {
                renderer.render_frame(&mut scene);
                // `ControlFlow::Wait` parks the thread; without yielding the
                // loop never reaches `AboutToWait` again.
                std::thread::sleep(Duration::from_millis(16));
            }
            _ => {}
        }
    })
    .map_err(|e| rhi::RhiError::BackendError(format!("event loop: {e}")))?;

    Ok(())
}

