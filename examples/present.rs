//! A window, a swapchain, and the fullscreen shader presented to it.
//!
//! Run with:
//!
//! ```text
//! cargo run --example present
//! ```
//!
//! The end-to-end proof that pixels reach a screen:
//! `winit` -> `raw-window-handle` -> `VkSurfaceKHR` -> `VkSwapchainKHR` ->
//! render pass -> `vkQueuePresentKHR`. Esc or closing the window quits;
//! resizing rebuilds the swapchain, which is the path that rots silently if
//! never exercised.
//!
//! An `example` rather than a `bin` on purpose: the crate is a library and this
//! must not change what ships.

use rhi::ash::vk::Handle;
use winit::raw_window_handle::HasWindowHandle;
use rhi::backend::vulkan::{Swapchain, SwapchainRequest};
// `PrimitiveTopology` exists in both `rhi::types` and `rhi::pipeline::graphics`,
// so the glob re-exports make the short name ambiguous; named explicitly.
use rhi::pipeline::graphics::PrimitiveTopology;
use rhi::{
    ClearValue, ColorBlendAttachment, ColorBlendState, ColorComponentFlags,
    ColorTargetDesc, Device, DeviceDesc, Features, GraphicsPipeline, GraphicsPipelineDesc,
    InputAssemblyState, MultisampleState, PipelineShaderStage, RasterizerState, RenderPass,
    RhiConfig, RhiResult, SampleCount, ShaderFormat, ShaderModuleDesc, ShaderStage,
};

/// The compiled shader, produced ahead of time from
/// `Tool/client_tests/assets/shaders/fullscreen.wgsl` by
/// `cargo test -p client_tests --test compile_shaders -- --ignored`.
const SHADER: &[u8] = include_bytes!("../Tool/client_tests/assets/shaders/fullscreen.spv");

fn all_channels() -> ColorComponentFlags {
    ColorComponentFlags::R
        | ColorComponentFlags::G
        | ColorComponentFlags::B
        | ColorComponentFlags::A
}

fn main() {
    if let Err(e) = run() {
        eprintln!("present example failed: {e}");
        std::process::exit(1);
    }
}

fn open_device() -> RhiResult<Device> {
    let backend = rhi::backend::create_backend(&RhiConfig::default())?;
    let devices = backend.enumerate_physical_devices()?;
    let Some(first) = devices.first() else {
        panic!("no Vulkan device available");
    };
    let desc = DeviceDesc { features: Features::default(), ..Default::default() };
    backend.create_device(first, &desc)
}

fn run() -> RhiResult<()> {
    let device = open_device()?;

    let event_loop = winit::event_loop::EventLoop::new().expect("event loop");
    let window = winit::window::WindowBuilder::new()
        .with_title("RFS — Vulkan present")
        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
        .build(&event_loop)
        .expect("window");

    // raw-window-handle 0.6 hands back a generic handle; on Windows both the
    // HWND and the HINSTANCE live on the window handle, and the display handle
    // carries nothing.
    let window_handle = window
        .window_handle()
        .map_err(|e| rhi::RhiError::BackendError(format!("window handle: {e}")))?;
    let (hwnd, hinstance) = match window_handle.as_raw() {
        winit::raw_window_handle::RawWindowHandle::Win32(win32) => (
            win32.hwnd.get() as u64,
            win32.hinstance.map(|h| h.get() as u64).ok_or_else(|| {
                rhi::RhiError::BackendError(
                    "Win32 window handle carries no HINSTANCE".into(),
                )
            })?,
        ),
        other => {
            return Err(rhi::RhiError::BackendError(format!(
                "expected a Win32 window handle, got {other:?}"
            )));
        }
    };

    let surface = device.create_window_surface(hinstance, hwnd)?;

    let size = window.inner_size();
    let request = SwapchainRequest {
        width: size.width.max(1),
        height: size.height.max(1),
        // MAILBOX is tear-free; `present.rs` falls back to FIFO itself.
        present_mode: rhi::ash::vk::PresentModeKHR::MAILBOX,
    };
    let mut swapchain = device.create_swapchain(&surface, request, None)?;

    // The swapchain chose the format, so the pass has to follow it.
    let Some(format) = swapchain.rhi_format() else {
        return Err(rhi::RhiError::BackendError(format!(
            "surface offered {:?}, which the RHI cannot express",
            swapchain.format
        )));
    };
    let render_pass = device.presentation_render_pass(format)?;
    let pipeline = build_pipeline(&device, &render_pass)?;

    println!(
        "presenting {}x{} as {:?} — Esc to quit",
        swapchain.extent.width, swapchain.extent.height, swapchain.format
    );

    // Recreate on resize. Reported rather than ignored, because a stale chain
    // would keep drawing into images the compositor no longer shows.
    let mut recreate_requested = false;

    event_loop.run(move |event, control_flow| {
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
                    recreate_requested = true;
                    let _ = size;
                }
                _ => {}
            },
            winit::event::Event::AboutToWait => {
                if recreate_requested {
                    recreate_requested = false;
                    let size = window.inner_size();
                    let request = SwapchainRequest {
                        width: size.width.max(1),
                        height: size.height.max(1),
                        present_mode: rhi::ash::vk::PresentModeKHR::MAILBOX,
                    };
                    match device.create_swapchain(&surface, request, Some(&mut swapchain)) {
                        Ok(next) => swapchain = next,
                        Err(e) => {
                            eprintln!("swapchain recreate failed: {e}");
                            control_flow.exit();
                            return;
                        }
                    }
                }

                // `OUT_OF_DATE` is the *normal* response to a resize: the
                // image we drew into is no longer displayed. Rebuild and skip
                // this frame. Exiting here — which the first version of this
                // example did — makes the window quit the moment it is dragged.
                match draw_frame(&device, &mut swapchain, &render_pass, &pipeline) {
                    Ok(()) => {}
                    Err(rhi::RhiError::SwapchainOutOfDate) => recreate_requested = true,
                    Err(e) => {
                        eprintln!("frame failed: {e}");
                        control_flow.exit();
                    }
                }
            }
            _ => {}
        }
    })
    .map_err(|e| rhi::RhiError::BackendError(format!("event loop: {e}")))
}

/// Acquire, render, wait, present.
fn draw_frame(
    device: &Device,
    swapchain: &mut Swapchain,
    render_pass: &RenderPass,
    pipeline: &GraphicsPipeline,
) -> RhiResult<()> {
    let acquired = swapchain.acquire(u64::MAX)?;
    let width = swapchain.extent.width;
    let height = swapchain.extent.height;
    let Some(format) = swapchain.rhi_format() else {
        return Err(rhi::RhiError::BackendError(format!(
            "surface offered {:?}, which the RHI cannot express",
            swapchain.format
        )));
    };

    // The framebuffer wraps a swapchain image, so it is rebuilt for the image
    // actually acquired. Correct but wasteful; caching per image index is the
    // obvious next step once more than one frame is in flight.
    let framebuffer = device.swapchain_image_framebuffer(
        render_pass,
        swapchain.images[acquired.image_index as usize].as_raw(),
        acquired.image_view.as_raw(),
        format,
        width,
        height,
    )?;

    // 3 vertices, no vertex buffer: the shader derives the fullscreen triangle.
    device.draw_framebuffer(
        &framebuffer,
        pipeline,
        &[],
        None,
        3,
        0,
        &[ClearValue::color(0.0, 0.0, 0.0, 1.0)],
    )?;

    // No fence wait: draw_framebuffer submits and blocks until the GPU is
    // finished, so the image cannot be handed over half-written.
    swapchain.present(acquired.image_index)?;
    Ok(())
}

fn build_pipeline(device: &Device, render_pass: &RenderPass) -> RhiResult<GraphicsPipeline> {
    let module = device.create_shader_module(&ShaderModuleDesc {
        code: SHADER.to_vec(),
        format: ShaderFormat::SpirV,
        entry_point: Some("vs_main".into()),
        name: Some("fullscreen".into()),
    });
    device.create_gpu_graphics_pipeline(
        &GraphicsPipelineDesc {
            shader_stages: vec![
                PipelineShaderStage {
                    stage: ShaderStage::VERTEX,
                    module: module.clone(),
                    entry_point: "vs_main".into(),
                },
                PipelineShaderStage {
                    stage: ShaderStage::FRAGMENT,
                    module,
                    entry_point: "fs_main".into(),
                },
            ],
            input_assembly_state: InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            rasterizer_state: Some(RasterizerState::double_sided()),
            multisample_state: MultisampleState {
                sample_count: SampleCount::X1,
                ..Default::default()
            },
            color_blend_state: Some(ColorBlendState {
                attachments: vec![ColorBlendAttachment {
                    color_write_mask: all_channels(),
                    ..ColorTargetDesc::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
        render_pass,
        &[],
    )
}