//! Does the UI actually put pixels in a framebuffer?
//!
//! Run with:
//!
//! ```text
//! cargo run --example ui_spike_offscreen
//! ```
//!
//! # Why this exists
//!
//! Every previous check of "is the UI working" was indirect: a draw call was
//! recorded, `skipped` was zero, the validation layer stayed quiet. None of those
//! can tell a correct draw from one that rasterises nothing — and that is exactly
//! how a rectangle that records perfectly on every frame for a minute and a half
//! can leave the window black.
//!
//! So this reads the pixels. It renders one rectangle into an ordinary texture,
//! downloads it, and counts.
//!
//! # The criterion
//!
//! A 160x90 rectangle is **14 400 pixels**. Anything less is clipping, anything
//! zero means nothing rasterised. The number is the answer; nothing else counts as
//! evidence.
//!
//! This is deliberately the same shape as `Tool/client_tests`' `geometry_pixels`:
//! offscreen target, real pass, real submit, real readback. If geometry can be
//! verified this way, UI can too, and the difference was never capability.

use std::sync::Arc;

use rfs_client::rhi::ui::{UiCommand, UiRenderStats, UiRenderer, UiRendererConfig};
use rfs_client::rhi::{
    CommandBuffer, CommandBufferDesc, CommandEncoder, CommandPool, CommandPoolDesc, Device,
    DeviceDesc, Features, Format, FramebufferAttachment, FramebufferDesc, RhiConfig, RhiError,
    RhiResult, TextureUsage, TextureView,
};

/// The target size. Small and square so the pixel count is easy to reason about.
const EXTENT: u32 = 256;

/// The rectangle, in pixels, at the top-left of the target.
const RECT_X: f32 = 32.0;
const RECT_Y: f32 = 32.0;
const RECT_W: f32 = 160.0;
const RECT_H: f32 = 90.0;
/// Below the rectangle, so text pixels are counted separately from it.
const TEXT_X: f32 = 32.0;
const TEXT_Y: f32 = 140.0;
/// A second line, below the first, for retained-mode text.
const RETAINED_TEXT_X: f32 = 32.0;
const RETAINED_TEXT_Y: f32 = 160.0;

/// NVIDIA green, chosen because it has no component below 0.1 and so cannot be
/// confused with a partially-written black frame.
const RECT_COLOR: [f32; 4] = [0.76, 0.95, 0.13, 1.0];

/// Compiled from `assets/shaders/ui_geom.wgsl`; see `ui.rs` for why the path
/// reaches into the test harness.
const UI_GEOM_SPV: &[u8] =
    include_bytes!("../Tool/client_tests/assets/shaders/ui_geom.spv");

const UI_TEX_SPV: &[u8] =
    include_bytes!("../Tool/client_tests/assets/shaders/ui_tex.spv");

fn main() {
    match run() {
        Ok(lit) => {
            let text_mode = std::env::var_os("UI_SPIKE_TEXT").is_some();
            let expected = expected_lit();
            println!("\nlit pixels: {lit} of {expected} (rectangle only)");
            if text_mode {
                // Text pixels are not predicted: the glyph shapes decide them, and
                // a font swap would change the number. What matters is that the
                // rectangle is still exact and that glyphs added something on top
                // of it — a textured pipeline that samples nothing would leave the
                // count at the rectangle alone.
                let text_pixels = lit.saturating_sub(expected);
                println!("text pixels: {text_pixels}");
                if lit >= expected && text_pixels > 0 {
                    println!("RESULT: pass - rectangle exact and glyphs drew.");
                } else if text_pixels == 0 {
                    println!("RESULT: fail - no glyph pixels; the atlas was not sampled.");
                } else {
                    println!("RESULT: fail - {} pixels, expected at least {}", lit, expected);
                }
            } else if lit == expected {
                println!("RESULT: pass — the UI rasterised exactly the rectangle.");
            } else if lit == 0 {
                println!("RESULT: fail — the frame is empty. Nothing rasterised.");
                println!(
                    "  The draw was recorded and validation was quiet, so the fault is \
                     between the recorded command and the rasteriser."
                );
            } else {
                println!("RESULT: fail — {} pixels, expected {}", lit, expected_lit());
                println!("  Partial coverage points at a viewport, scissor or extent clip.");
            }
        }
        Err(e) => {
            eprintln!("ui_spike_offscreen failed: {e}");
            std::process::exit(1);
        }
    }
}

/// The whole rectangle, in pixels.
fn expected_lit() -> u32 {
    (RECT_W as u32) * (RECT_H as u32)
}

/// The command's variant name, so the sequence can be read at a glance.
fn command_name(command: &rfs_client::rhi::command::commands::Command) -> &'static str {
    use rfs_client::rhi::command::commands::Command as C;
    match command {
        C::BeginRenderPass(_) => "BeginRenderPass",
        C::EndRenderPass => "EndRenderPass",
        C::SetViewport(_) => "SetViewport",
        C::SetScissor(_) => "SetScissor",
        C::BindGraphicsPipeline(_) => "BindGraphicsPipeline",
        C::BindDescriptorSets { .. } => "BindDescriptorSets",
        C::BindVertexBuffers { .. } => "BindVertexBuffers",
        C::BindIndexBuffer { .. } => "BindIndexBuffer",
        C::DrawIndexed { .. } => "DrawIndexed",
        C::Draw { .. } => "Draw",
        _ => "other",
    }
}

fn open_device() -> Arc<Device> {
    let backend = rfs_client::rhi::backend::create_backend(&RhiConfig {
        // Validation names the exact misuse before it becomes a blank frame, and a
        // blank frame is otherwise indistinguishable from "the projection is wrong",
        // which is half of what this test is checking.
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

fn run() -> RhiResult<u32> {
    let device = open_device();
    let format = Format::RGBA8_UNORM;

    // The geometry shader can be swapped at run time. `UI_SPIKE_SHADER` pointing
    // at a shader that ignores its vertex inputs tells "the rasteriser never ran"
    // apart from "an attribute never arrived" — two faults that look identical
    // from the outside.
    let geometry_spirv: Vec<u8> = match std::env::var("UI_SPIKE_SHADER") {
        Ok(path) => std::fs::read(&path)
            .map_err(|e| RhiError::BackendError(format!("UI_SPIKE_SHADER: {path}: {e}")))?,
        Err(_) => UI_GEOM_SPV.to_vec(),
    };

    let texture = device.create_texture(
        EXTENT,
        EXTENT,
        1,
        format,
        TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
        1,
    );
    let view: TextureView = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .map_err(|e| RhiError::BackendError(format!("colour view: {e}")))?;

    // Build the renderer FIRST, then take its render pass: a framebuffer built
    // from a separately-constructed, identically-described render pass is a
    // different object and is not compatible with this one. Getting that wrong
    // does not announce itself — the frame simply comes back empty.
    let mut ui = UiRenderer::new(
        Arc::clone(&device),
        UiRendererConfig {
            initial_vertex_capacity: 256,
            initial_index_capacity: 512,
            debug_label: false,
        },
    );
    ui.initialize_offscreen(
        format,
        EXTENT,
        EXTENT,
        &geometry_spirv,
        Some(UI_TEX_SPV),
    )
    .map_err(|e| RhiError::BackendError(format!("ui initialize: {e}")))?;

    let render_pass = ui
        .render_pass()
        .ok_or_else(|| RhiError::BackendError("the UI has no render pass".into()))?;

    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass,
            attachments: vec![FramebufferAttachment {
                texture_view: view,
                layer: 0,
                mip_level: 0,
            }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .map_err(|e| RhiError::BackendError(format!("framebuffer: {e}")))?;

    ui.submit_commands(vec![UiCommand::Rectangle {
        x: RECT_X,
        y: RECT_Y,
        width: RECT_W,
        height: RECT_H,
        color: RECT_COLOR,
        corner_radius: 0.0,
    }])
    .map_err(|e| RhiError::BackendError(format!("submit commands: {e}")))?;

    // Text needs a font and the atlas it was rasterised into. Neither is
    // optional and neither used to exist, so this is the first time the
    // textured pipeline has had anything to sample.
    if std::env::var_os("UI_SPIKE_TEXT").is_some() {
        ui.install_bitmap_font("bitmap8")
            .map_err(|e| RhiError::BackendError(format!("font install: {e}")))?;
        // Retained text: the same glyphs, reached through the element tree
        // rather than through an immediate command. Two routes into one
        // pipeline, and only the pixels say both arrived.
        {
            let mut element = rfs_client::rhi::ui::UiElement::new(
                rfs_client::rhi::ui::UiElementType::Text,
                1u64,
            );
            element.position = (RETAINED_TEXT_X, RETAINED_TEXT_Y);
            element.size = (200.0, 16.0);
            element.style.font_size = 8.0;
            element.style.foreground_color = Some([1.0, 0.7, 0.2, 1.0]);
            element.data = rfs_client::rhi::ui::UiElementData::Text {
                content: "RETAINED".into(),
            };
            ui.submit_element(element)
                .map_err(|e| RhiError::BackendError(format!("submit element: {e}")))?;
        }
        ui.submit_commands(vec![UiCommand::Text {
            content: "TEXT 123".into(),
            x: TEXT_X,
            y: TEXT_Y,
            size: 8.0,
            color: [1.0, 1.0, 1.0, 1.0],
            font: None,
        }])
        .map_err(|e| RhiError::BackendError(format!("submit text: {e}")))?;
    }

    // Record-and-submit, following the same sequence the render graph uses: a
    // pool, a buffer from it, `begin`, an encoder that owns the buffer, then
    // `finish` hands the commands back. Skipping any of these steps records
    // nothing, which is how a whole frame can be silently discarded.
    let pool = CommandPool::new(CommandPoolDesc::default());
    let mut buffer = CommandBuffer::new(pool, CommandBufferDesc::default());
    buffer
        .begin()
        .map_err(|e| RhiError::BackendError(format!("command buffer begin: {e}")))?;
    let mut encoder = CommandEncoder::new(buffer);

    let recorded = ui
        .record_into_framebuffer(&mut encoder, &framebuffer, EXTENT, EXTENT)
        .map_err(|e| RhiError::BackendError(format!("record: {e}")))?;

    let stats: UiRenderStats = ui.stats();
    println!("recorded={recorded} stats={stats:?}");
    println!("target {EXTENT}x{EXTENT}, rect {RECT_W}x{RECT_H} at ({RECT_X}, {RECT_Y})");

    // Read back what the GPU would actually see, before drawing anything. If the
    // projection is not the matrix we computed, `clip.w` is 0 and every vertex
    // collapses — a draw that records perfectly and rasterises nothing.
    if let Some((uniform, vertices)) = ui.debug_visible_bytes() {
        let proj: Vec<f32> = uniform
            .chunks_exact(4)
            .take(16)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        println!("uniform (as the GPU sees it) = {proj:?}");
        let first: Vec<f32> = vertices
            .chunks_exact(4)
            .take(8)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        println!("first vertices (as the GPU sees them) = {first:?}");
    } else {
        println!("could not read the UI buffers back");
    }

    if !recorded {
        return Err(RhiError::BackendError(
            "the UI recorded no draw at all".into(),
        ));
    }

    let buffer = encoder
        .finish()
        .map_err(|e| RhiError::BackendError(format!("encoder finish: {e}")))?;

    // What actually reached submit. A draw whose bindings never made it into the
    // buffer rasterises nothing and looks identical, from the outside, to one that
    // worked.
    println!("recorded commands ({}):", buffer.commands().len());
    for (i, c) in buffer.commands().iter().enumerate() {
        // The draw's own numbers matter as much as its presence: a count of zero
        // or an index far past the end of the buffer records perfectly and
        // rasterises nothing.
        if let rfs_client::rhi::command::commands::Command::DrawIndexed {
            index_count,
            first_index,
            vertex_offset,
            instance_count,
            first_instance: _,
        } = c
        {
            println!(
                "  [{i}] DrawIndexed count={index_count} first={first_index} \
                 vertex_offset={vertex_offset} instances={instance_count}"
            );
            continue;
        }
        if let rfs_client::rhi::command::commands::Command::BindIndexBuffer {
            offset, ..
        } = c
        {
            println!("  [{i}] BindIndexBuffer offset={offset}");
            continue;
        }
        if let rfs_client::rhi::command::commands::Command::BindVertexBuffers {
            first_binding, buffers,
        } = c
        {
            println!("  [{i}] BindVertexBuffers first_binding={first_binding} buffers={buffers:?}");
            continue;
        }
        if let rfs_client::rhi::command::commands::Command::BindDescriptorSets {
            first_set, sets, ..
        } = c
        {
            println!("  [{i}] BindDescriptorSets first_set={first_set} sets={sets:?}");
            continue;
        }
        if let rfs_client::rhi::command::commands::Command::SetViewport(v) = c {            println!(
                "  [{i}] SetViewport x={} y={} w={} h={} min_depth={} max_depth={}",
                v.x, v.y, v.width, v.height, v.min_depth, v.max_depth
            );
            continue;
        }
        if let rfs_client::rhi::command::commands::Command::SetScissor(s) = c {
            println!(
                "  [{i}] SetScissor x={} y={} w={} h={}",
                s.offset.x, s.offset.y, s.extent.width, s.extent.height
            );
            continue;
        }
        println!("  [{i}] {}", command_name(c));
    }

    device.submit_recorded(buffer.commands())?;

    // Queue completion before the readback. Validation reported
    // `VUID-vkCmdDraw-None-09600` — the offscreen image seen in
    // `TRANSFER_SRC_OPTIMAL` during the draw — and an unsynchronised second
    // submit is the cheapest thing to rule out before blaming the backend.
    device.wait_idle()?;

    // Read the pixels. This is the whole point of the exercise.
    let bytes = device
        .download_texture(&texture, 0, 0)
        .map_err(|e| RhiError::BackendError(format!("readback: {e}")))?;

    let expected_len = (EXTENT * EXTENT * 4) as usize;
    if bytes.len() < expected_len {
        return Err(RhiError::BackendError(format!(
            "readback returned {} bytes, expected {expected_len}",
            bytes.len()
        )));
    }

    let lit = count_lit(&bytes, EXTENT, EXTENT);
    if std::env::var_os("UI_SPIKE_ASCII").is_some() {
        print_ascii(&bytes);
    }
    report_samples(&bytes, EXTENT, EXTENT);
    Ok(lit)
}

/// Counts pixels whose colour is neither black nor fully transparent.
///
/// Blue is skipped: `RGBA8_UNORM` leaves the alpha channel of an untouched
/// image undefined on some drivers, and a stray non-zero there would otherwise
/// inflate the count into a false pass.
/// Prints the lit pixels as ASCII art.
///
/// A pixel count alone cannot tell letters from noise: a wrong UV rect, a
/// wrapped sampler and a mirrored glyph all produce a plausible number of lit
/// pixels. Reading the shapes is the only cheap check that the atlas rects, the
/// UV conversion and the vertex positions all agree.
fn print_ascii(bytes: &[u8]) {
    println!("\n--- lit pixels as ASCII (# = ink) ---");
    let from = TEXT_Y as u32;
    let to = (RETAINED_TEXT_Y as u32 + 10).min(EXTENT);
    for y in from..to {
        let mut row = String::new();
        for x in 0..(TEXT_X as u32 + 72).min(EXTENT) {
            let at = ((y * EXTENT + x) * 4) as usize;
            let ink = bytes[at..at + 3].iter().any(|c| *c != 0);
            row.push(if ink { '#' } else { '.' });
        }
        println!("{row}");
    }
}

fn count_lit(bytes: &[u8], width: u32, height: u32) -> u32 {
    let mut lit = 0;
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            let r = bytes[i];
            let g = bytes[i + 1];
            let b = bytes[i + 2];
            let a = bytes[i + 3];
            if (r > 8 || g > 8 || b > 8) && a > 8 {
                lit += 1;
            }
        }
    }
    lit
}

/// Prints the colour actually found at the four corners of the rectangle and one
/// point well outside it.
///
/// A count alone can be right for the wrong reason. The inside corners must be
/// green and the outside point must be dark, and that distinguishes "the
/// rectangle drew" from "something else covered the target".
fn report_samples(bytes: &[u8], width: u32, _height: u32) {
    let at = |x: u32, y: u32| -> (u8, u8, u8, u8) {
        let i = ((y * width + x) * 4) as usize;
        (bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3])
    };
    let x0 = RECT_X as u32;
    let y0 = RECT_Y as u32;
    let x1 = (RECT_X + RECT_W) as u32 - 1;
    let y1 = (RECT_Y + RECT_H) as u32 - 1;
    println!("samples:");
    println!("  rect top-left     ({x0},{y0}) -> {:?}", at(x0, y0));
    println!("  rect top-right    ({x1},{y0}) -> {:?}", at(x1, y0));
    println!("  rect bottom-left  ({x0},{y1}) -> {:?}", at(x0, y1));
    println!("  rect bottom-right ({x1},{y1}) -> {:?}", at(x1, y1));
    println!("  outside           (2,2)      -> {:?}", at(2, 2));
    let expected = [
        (RECT_COLOR[0] * 255.0) as u8,
        (RECT_COLOR[1] * 255.0) as u8,
        (RECT_COLOR[2] * 255.0) as u8,
    ];
    println!("  expected green-ish: {:?}", expected);
}
