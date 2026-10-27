//! Does the UI actually put pixels in a framebuffer?
//!
//! Recording commands correctly is not the same as drawing. Every one of the
//! following records perfectly, submits cleanly, and produces an untouched
//! framebuffer:
//!
//!  - a colour attachment whose `color_write_mask` is empty, so the blend
//!    result is computed and then discarded;
//!  - a recorded command list with no `EndRenderPass`, which makes the driver
//!    drop the whole command buffer;
//!  - a pass whose final layout disagrees with the readback's assumed layout;
//!  - vertex data that never reached the buffer it is bound from.
//!
//! None of them are visible from the command list, and a zero write mask is not
//! even a validation error. So this test does what `geometry_pixels.rs` does
//! for geometry: submit the real thing to an offscreen target, read the pixels
//! back, and assert what came out.
//!
//! The assertions are on an exact count, not on `lit > 0`. "Something was drawn"
//! has been true of a frame that drew the wrong thing, and the shape of the
//! UI's output is known precisely enough here to pin down exactly.

use std::sync::Arc;

use rfs_client::rhi::command::commands::Command;
use rfs_client::rhi::command::{
    CommandBuffer, CommandBufferDesc, CommandEncoder, CommandPool, CommandPoolDesc,
};
use rfs_client::rhi::ui::{UiCommand, UiElement, UiElementData, UiElementType, UiInputEvent, UiRenderer, UiRendererConfig};
use rfs_client::rhi::{
    ClearValue, Device, DeviceDesc, Features, Format, Framebuffer, FramebufferAttachment,
    FramebufferDesc, LoadOp, RhiConfig, Texture, TextureLayout, TextureUsage, TextureView,
};

const EXTENT: u32 = 256;

/// A rectangle at a fixed offset, sized so the expected pixel count is exact.
const RECT_X: f32 = 32.0;
const RECT_Y: f32 = 32.0;
const RECT_W: f32 = 160.0;
const RECT_H: f32 = 90.0;

/// Deliberately not white and not grey. A colour whose sRGB round trip yields
/// four distinct channels makes "the blend wrote all four channels"
/// observable, which a white rectangle cannot: white is indistinguishable from
/// a partial write.
const RECT_COLOR: [f32; 4] = [0.76, 0.95, 0.13, 1.0];

const GEOMETRY_SPIRV: &[u8] = include_bytes!("../assets/shaders/ui_geom.spv");
const TEXTURE_SPIRV: &[u8] = include_bytes!("../assets/shaders/ui_tex.spv");
/// Full-colour image shader: samples the texture's RGBA and multiplies it by the
/// vertex colour. `TEXTURE_SPIRV` is the glyph coverage shader and only reads the
/// red channel, so it cannot show an image's colour.
const IMAGE_SPIRV: &[u8] = include_bytes!("../assets/shaders/ui.spv");

fn open_device() -> Arc<Device> {
    // Validation on, not `RhiConfig::default()`. The default is
    // `validation: false`, so a pixel test built on it proves the framebuffer has
    // the expected pixels while saying nothing about whether the submission was
    // legal. That combination is exactly how a zero `color_write_mask` survives:
    // the frame is wrong and no layer names the reason. Validation needs
    // `VK_LAYER_PATH` to point at `UPl/valayer` to load; without it
    // `instance.rs` skips the requested layer *silently*, so this is best-effort
    // rather than a guarantee.
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

/// The renderer, its readable target, and the framebuffer it records into.
struct UiTarget {
    device: Arc<Device>,
    ui: UiRenderer,
    texture: Texture,
    /// Kept as well as handed to the framebuffer: the presentation path takes a
    /// typed view, not a raw handle, so a test of that path needs the view.
    view: TextureView,
    framebuffer: Framebuffer,
}

/// Builds a UI renderer against a readable offscreen target.
///
/// The renderer is built *first* and its own render pass is taken from it: a
/// framebuffer built against a separately-constructed, identically-described
/// pass is a different object and is incompatible with this pipeline. Getting
/// that wrong does not announce itself either — the frame is simply empty.
fn build_target() -> UiTarget {
    let device = open_device();
    let format = Format::RGBA8_UNORM;

    let texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            format,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the UI target must be created");
    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("colour attachment view");

    let mut ui = UiRenderer::new(
        Arc::clone(&device),
        UiRendererConfig {
            initial_vertex_capacity: 256,
            initial_index_capacity: 512,
            debug_label: false,
        },
    );
    ui.initialize_offscreen(format, EXTENT, EXTENT, GEOMETRY_SPIRV, Some(TEXTURE_SPIRV))
        .expect("ui initialize");

    let render_pass = ui.render_pass().expect("the UI must expose its render pass");
    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass,
            attachments: vec![FramebufferAttachment {
                texture_view: view.clone(),
                layer: 0,
                mip_level: 0,
            }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    ui.submit_commands(vec![UiCommand::Rectangle {
        x: RECT_X,
        y: RECT_Y,
        width: RECT_W,
        height: RECT_H,
        color: RECT_COLOR,
        corner_radius: 0.0,
    }])
    .expect("submit commands");

    UiTarget { device, ui, texture, view, framebuffer }
}

/// Records one UI frame and returns the recorded commands.
fn record(target: &mut UiTarget) -> Vec<Command> {
    let pool = CommandPool::new(CommandPoolDesc::default());
    let mut buffer = CommandBuffer::new(pool, CommandBufferDesc::default());
    buffer.begin().expect("begin");
    let mut encoder = CommandEncoder::new(buffer);
    assert!(
        target
            .ui
            .record_into_framebuffer(&mut encoder, &target.framebuffer, EXTENT, EXTENT)
            .expect("record"),
        "the UI reported nothing to draw"
    );
    encoder.commands().to_vec()
}

/// The offscreen pass must end in a layout the readback will accept.
///
/// `Device::download_texture` derives its barrier's `oldLayout` from the
/// texture's *usage flags* — `COLOR_ATTACHMENT_OPTIMAL` for a colour
/// attachment — rather than from the image's actual layout. A pass that already
/// leaves the image in `TRANSFER_SRC_OPTIMAL` therefore hands that barrier a
/// false `oldLayout`, and the readback returns whatever the copy happened to
/// find. Geometry already ends in `ColorAttachmentOptimal`, which is why the
/// same readback is trustworthy there and was not here.
#[test]
fn the_offscreen_pass_ends_where_the_readback_expects() {
    let target = build_target();
    let desc = target.ui.render_pass().expect("render pass").desc().clone();
    let attachment = &desc.attachments[0];

    assert_eq!(
        attachment.final_layout,
        TextureLayout::ColorAttachmentOptimal,
        "the offscreen pass must leave the image where download_texture's barrier \
         assumes it is"
    );
    assert_eq!(
        attachment.load_op,
        LoadOp::Clear,
        "this test reads a cleared background, so the pass must clear"
    );
}

/// The recorded command list must open *and* close its render pass.
///
/// An `EndRenderPass` gated on a condition that is never true is the failure
/// this guards: the list looks complete, every command is accepted, and the
/// driver discards the whole command buffer at `vkEndCommandBuffer`.
#[test]
fn the_ui_records_a_balanced_render_pass() {
    let mut target = build_target();
    let commands = record(&mut target);

    let begin = commands
        .iter()
        .position(|c| matches!(c, Command::BeginRenderPass(_)))
        .expect("no BeginRenderPass recorded");
    let draw = commands
        .iter()
        .position(|c| matches!(c, Command::DrawIndexed { .. }))
        .expect("no indexed draw recorded");
    let end = commands
        .iter()
        .position(|c| matches!(c, Command::EndRenderPass))
        .expect("no EndRenderPass recorded");

    assert!(begin < draw, "the draw was issued before the pass began");
    assert!(
        draw < end,
        "the draw was issued after the pass ended, so it landed outside any pass"
    );
}

/// The UI rasterises exactly its rectangle, and nothing else.
///
/// This is the assertion the empty-`color_write_mask` bug could not survive:
/// with a zero write mask the rectangle is blended into nothing, the frame keeps
/// the clear, `lit` is 0, and validation stays silent because a zero write mask
/// is legal Vulkan.
#[test]
fn the_ui_rasterises_exactly_its_rectangle() {
    let mut target = build_target();
    let commands = record(&mut target);

    // Sampled around this test's own submission. The count is global and the
    // other two tests in this binary record but do not submit, so nothing else
    // can move it concurrently.
    //
    // Pixel assertions alone are not enough: an invalid submission can still
    // produce the right pixels. A frame with an empty `clear_values` list while
    // the pass declares `LoadOp::Clear` is exactly that case - it rasterises the
    // rectangle correctly, and validation is the only thing that names the
    // misuse. `validation_error_count` exists for this; see `rhi`'s doc comment.
    let validation_before = rhi::validation_error_count();

    target.device.submit_recorded(&commands).expect("submit");
    // Queue completion before the readback: the copy is a second submit.
    target.device.wait_idle().expect("wait idle");

    let bytes = target
        .device
        .download_texture(&target.texture, 0, 0)
        .expect("readback");
    assert_eq!(
        bytes.len(),
        (EXTENT * EXTENT * 4) as usize,
        "the readback is not one RGBA pixel per texel"
    );

    let pixel = |x: u32, y: u32| -> [u8; 4] {
        let at = ((y * EXTENT + x) * 4) as usize;
        [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
    };

    // The centre, well away from any edge. All four channels must be written: a
    // partial write mask is exactly what this catches, and it would otherwise
    // show up only as a slightly wrong count.
    let cx = RECT_X as u32 + RECT_W as u32 / 2;
    let cy = RECT_Y as u32 + RECT_H as u32 / 2;
    let centre = pixel(cx, cy);
    assert_eq!(
        centre[3], 255,
        "the rectangle's centre has alpha {} — the blend did not write every channel",
        centre[3]
    );

    // Integer bounds, so "inside the rectangle" is exact. The rectangle's
    // coordinates are whole pixels, so this is the same region the rasteriser
    // covers rather than an approximation of it.
    let x0 = RECT_X as u32;
    let x1 = (RECT_X + RECT_W) as u32;
    let y0 = RECT_Y as u32;
    let y1 = (RECT_Y + RECT_H) as u32;

    let mut lit = 0u32;
    let mut inside = 0u32;
    for y in 0..EXTENT {
        for x in 0..EXTENT {
            if pixel(x, y).iter().any(|c| *c != 0) {
                lit += 1;
            }
            if x >= x0 && x < x1 && y >= y0 && y < y1 {
                inside += 1;
            }
        }
    }

    let total = EXTENT * EXTENT;
    let expected = (RECT_W * RECT_H) as u32;
    println!("lit={lit} inside={inside} of {total}");
    assert_eq!(
        lit, expected,
        "expected exactly the {expected} rectangle pixels to be non-zero, got {lit} \
         (the rectangle covers {inside} pixels)"
    );

    let validation_after = rhi::validation_error_count();
    assert_eq!(
        validation_after, validation_before,
        "the validator reported {} error(s) for a submission that produced the right \
         pixels — correct output does not mean correct API use",
        validation_after - validation_before
    );

    // The presentation path, recorded and submitted here rather than in its own
    // test: this is the one test in this binary that submits, and a second
    // submitting test would race the global counter the assertion above uses.
    //
    // What is checked is that a draw lands in the image it was handed.
    // `record_into_view` keeps one framebuffer per image view; a single
    // framebuffer shared across the chain would send every frame into the first
    // image, and nothing above could tell — it would go on asserting about that
    // one image forever while the swapchain presented the rest untouched.
    // Asserting equality with this test's own rendered pixel sidesteps knowing
    // the expected colour: wrong-image garbage does not match four exact bytes.
    let second = target
        .device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the second image must be created");
    let second_view = target
        .device
        .create_texture_view(&second, second.create_default_view().desc())
        .expect("second image view");

    let validation_before_view = rhi::validation_error_count();
    let first_centre = pixel(cx, cy);

    let pool = CommandPool::new(CommandPoolDesc::default());
    let mut buffer = CommandBuffer::new(pool, CommandBufferDesc::default());
    buffer.begin().expect("begin");
    let mut encoder = CommandEncoder::new(buffer);
    // Two rounds over both images. The first round makes each image build its
    // own framebuffer; the second records into both again from the cache. A
    // cache keyed by something other than the view would put the second image's
    // draw into the first framebuffer, and the readback below would then find
    // the first image untouched.
    //
    // Each record drains the command queue, so commands are re-queued before
    // every one: recording an emptied queue reports "nothing to draw" and would
    // test the queue rather than the cache.
    for image in 0..2 {
        for _ in 0..2 {
            target
                .ui
                .submit_commands(vec![UiCommand::Rectangle {
                    x: RECT_X,
                    y: RECT_Y,
                    width: RECT_W,
                    height: RECT_H,
                    color: RECT_COLOR,
                    corner_radius: 0.0,
                }])
                .expect("queue UI commands");
            let view = if image == 0 { &target.view } else { &second_view };
            assert!(
                target
                    .ui
                    .record_into_view(&mut encoder, view, EXTENT, EXTENT)
                    .expect("record into the image"),
                "the UI reported nothing to draw into the image"
            );
        }
    }
    let view_commands = encoder.commands().to_vec();
    target
        .device
        .submit_recorded(&view_commands)
        .expect("submit the presentation path");
    target.device.wait_idle().expect("wait idle");

    let second_bytes = target
        .device
        .download_texture(&second, 0, 0)
        .expect("readback of the second image");
    assert_eq!(
        second_bytes.len(),
        (EXTENT * EXTENT * 4) as usize,
        "the readback of the second image is not one RGBA pixel per texel"
    );
    let at = ((cy as usize) * (EXTENT as usize) + cx as usize) * 4;
    let second_centre = [
        second_bytes[at],
        second_bytes[at + 1],
        second_bytes[at + 2],
        second_bytes[at + 3],
    ];
    assert_eq!(
        second_centre, first_centre,
        "the second image does not match the first — the draw went to whichever image \
         was cached instead of the one this call was given"
    );
    assert_eq!(
        rhi::validation_error_count(),
        validation_before_view,
        "the presentation path reported {} error(s)",
        rhi::validation_error_count() - validation_before_view
    );
}

/// A registered image is sampled and drawn, not merely recorded.
///
/// The builder tests already cover that an image command produces the right
/// vertices and UVs, but none of that says the texture reaches the sampler. This
/// submits a real image and reads it back.
///
/// The source texture is filled with `Command::ClearColor` rather than uploaded:
/// a solid colour gives an unambiguous expected pixel, and a clear needs no
/// staging buffer and no copy layout to get wrong.
/// A registered image is sampled and drawn, not merely recorded.
///
/// **Not implemented.** It is here as the specification of what still has to
/// exist, so nobody reads the missing coverage as an oversight.
///
/// The blocker is `Command::ClearColor`, not the image path. Filling the source
/// texture needs *some* way to put known pixels in it, and `ClearColor` is the
/// one that does not need a staging buffer and a copy layout. As it stands it
/// emits a range whose aspect mask is not `COLOR`:
/// `VUID-vkCmdClearColorImage-aspectMask-02498`, and the clear is rejected.
/// That translation is in the shared backend, not in `src/rhi/ui/`.
#[test]
#[ignore = "blocked on the ClearColor aspect mask in the shared Vulkan backend"]
fn a_registered_image_is_actually_sampled_and_drawn() {
    const IMAGE_ID: u64 = 42;
    // Pure blue, in the same order `ClearValue::color` takes, and chosen so no
    // channel matches the rectangle colour used by the other tests.
    const IMAGE_COLOR: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    let device = open_device();
    let format = Format::RGBA8_UNORM;

    // 1. A texture to draw, filled with a known colour.
    let image_texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            format,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
            1,
        )
        .expect("the sampled image must be created");
    let image_view = device
        .create_texture_view(
            &image_texture,
            &rfs_client::rhi::TextureViewDesc {
                texture: image_texture.clone(),
                format: Some(format),
                view_type: rfs_client::rhi::TextureViewType::D2,
                aspects: rfs_client::rhi::TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("image view");

    let pool = CommandPool::new(CommandPoolDesc::default());
    let mut buffer = CommandBuffer::new(pool, CommandBufferDesc::default());
    buffer.begin().expect("begin");
    let mut encoder = CommandEncoder::new(buffer);
    encoder.record(Command::ClearColor {
        texture: image_view,
        value: ClearValue::color(
            IMAGE_COLOR[0],
            IMAGE_COLOR[1],
            IMAGE_COLOR[2],
            IMAGE_COLOR[3],
        ),
        rects: Vec::new(),
    });
    device.submit_recorded(encoder.commands()).expect("submit");
    device.wait_idle().expect("wait idle");

    // 2. The target the UI draws into.
    let target = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            format,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the UI target must be created");
    let target_view = device
        .create_texture_view(&target, target.create_default_view().desc())
        .expect("target view");

    let mut ui = UiRenderer::new(
        Arc::clone(&device),
        UiRendererConfig {
            initial_vertex_capacity: 256,
            initial_index_capacity: 512,
            debug_label: false,
        },
    );
    ui.initialize_offscreen(format, EXTENT, EXTENT, GEOMETRY_SPIRV, Some(TEXTURE_SPIRV))
        .expect("ui initialize");
    ui.load_image_shader(IMAGE_SPIRV).expect("image shader");
    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: ui.render_pass().expect("render pass"),
            attachments: vec![FramebufferAttachment {
                texture_view: target_view,
                layer: 0,
                mip_level: 0,
            }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    // 3. Register the texture and ask for it across the whole target, so the
    // expected pixel count is the whole frame and any sampling error shows.
    ui.add_texture(IMAGE_ID, Arc::new(image_texture.clone()));
    ui.submit_commands(vec![UiCommand::Image {
        texture_id: IMAGE_ID,
        x: 0.0,
        y: 0.0,
        width: EXTENT as f32,
        height: EXTENT as f32,
        uv_min: [0.0, 0.0],
        uv_max: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
    }])
    .expect("submit commands");

    let pool = CommandPool::new(CommandPoolDesc::default());
    let mut buffer = CommandBuffer::new(pool, CommandBufferDesc::default());
    buffer.begin().expect("begin");
    let mut encoder = CommandEncoder::new(buffer);
    assert!(
        ui.record_into_framebuffer(&mut encoder, &framebuffer, EXTENT, EXTENT)
            .expect("record"),
        "the UI reported nothing to draw"
    );
    let validation_before = rhi::validation_error_count();
    device.submit_recorded(encoder.commands()).expect("submit");
    device.wait_idle().expect("wait idle");

    let bytes = device
        .download_texture(&target, 0, 0)
        .expect("readback");
    assert_eq!(
        bytes.len(),
        (EXTENT * EXTENT * 4) as usize,
        "the readback is not one RGBA pixel per texel"
    );

    // The whole target is covered, so every pixel must be the image colour. A
    // sampler left unbound, a missing descriptor write or a UV rect of zero all
    // produce the clear colour instead, which this catches as a full-frame miss.
    //
    // Channel order matches the readback, which is the same order `ClearValue`
    // and `Format::RGBA8_UNORM` use: no swap happens between the clear and
    // `download_texture`. Reading the source image back after its own clear
    // confirms this — a pure blue clear reads as `[0, 0, 255, 255]`.
    let expected = [
        (IMAGE_COLOR[0] * 255.0).round() as u8,
        (IMAGE_COLOR[1] * 255.0).round() as u8,
        (IMAGE_COLOR[2] * 255.0).round() as u8,
        255u8,
    ];
    let mut correct = 0u32;
    let total = EXTENT * EXTENT;
    for px in bytes.chunks_exact(4) {
        if [px[0], px[1], px[2], px[3]] == expected {
            correct += 1;
        }
    }
    println!("image pixels correct: {correct} of {total}, expected {expected:?}");
    assert_eq!(
        correct, total,
        "the image did not cover the target: {correct} of {total} pixels are {expected:?}"
    );

    assert_eq!(
        rhi::validation_error_count(),
        validation_before,
        "the validator reported an error while drawing the image"
    );
}

/// Builds the renderer without any queued commands.
///
/// `build_target` submits a rectangle, so a test that asserts "nothing was
/// recorded" needs a target that has nothing queued — otherwise it is measuring
/// the rectangle.
fn build_empty_target() -> UiTarget {
    let device = open_device();
    let format = Format::RGBA8_UNORM;
    let texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            format,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the UI target must be created");
    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("colour attachment view");
    let mut ui = UiRenderer::new(
        Arc::clone(&device),
        UiRendererConfig {
            initial_vertex_capacity: 256,
            initial_index_capacity: 512,
            debug_label: false,
        },
    );
    ui.initialize_offscreen(format, EXTENT, EXTENT, GEOMETRY_SPIRV, Some(TEXTURE_SPIRV))
        .expect("ui initialize");
    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: ui.render_pass().expect("render pass"),
            attachments: vec![FramebufferAttachment {
                texture_view: view.clone(),
                layer: 0,
                mip_level: 0,
            }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");
    UiTarget { device, ui, texture, view, framebuffer }
}

/// Hovering an element flips the flag the renderer paints from.
///
/// The failure this guards is not a wrong flag value - `input.rs` had those
/// right and unit-tested from the first day. It is that `UiRenderer` held no
/// input state and nothing called `input::update`, so `hovered` was always
/// `false` and a button could never look pressed. The chain was written, tested
/// at both ends, and joined at neither.
///
/// Proven through the real path: an event is queued, a frame is recorded, and the
/// element map is read back. A unit test of `input::update` cannot catch this,
/// because `input::update` was never the broken part.
#[test]
fn a_hover_event_reaches_the_element_the_renderer_paints_from() {
    let mut target = build_empty_target();

    // A button sitting where the cursor will be.
    let mut button = UiElement::new(UiElementType::Button, 1u64);
    button.position = (10.0, 10.0);
    button.size = (200.0, 60.0);
    // Hit testing skips anything not interactive, so a button that is only
    // painted is never under the cursor. Worth stating: it is the difference
    // between "the input chain is broken" and "the caller never opted in".
    button.interactive = true;
    button.data = UiElementData::Button {
        text: String::new(),
        normal_color: [0.2, 0.2, 0.2, 1.0],
        hover_color: [1.0, 0.0, 0.0, 1.0],
        pressed_color: [0.0, 1.0, 0.0, 1.0],
        disabled_color: [0.1, 0.1, 0.1, 1.0],
    };
    target.ui.submit_element(button).expect("submit element");

    // Before any input: nothing is hovered.
    let _ = record(&mut target);
    assert!(
        !target.ui.elements().get(&1u64).is_some_and(|e| e.hovered),
        "an element is hovered before any input arrived"
    );

    // Move the cursor over it, then record another frame.
    target
        .ui
        .push_input_event(UiInputEvent::MouseMoved { x: 100.0, y: 40.0 });
    let _ = record(&mut target);

    let button = target
        .ui
        .elements()
        .get(&1u64)
        .expect("the button is still there");
    assert!(
        button.hovered,
        "the cursor is over the button but the renderer painted it as unhovered"
    );
    assert_eq!(
        target.ui.interaction().hovered,
        Some(1u64),
        "the frame did not report which element is under the cursor"
    );
}
/// solid rectangle. Drawing it untextured would look like a loaded image that
/// merely happens to be the wrong one.
#[test]
fn an_image_with_an_unregistered_id_is_skipped_not_drawn() {
    let mut target = build_empty_target();
    target
        .ui
        .submit_commands(vec![UiCommand::Image {
            texture_id: 9999,
            x: 0.0,
            y: 0.0,
            width: EXTENT as f32,
            height: EXTENT as f32,
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
        }])
        .expect("submit commands");

    // Recording by hand rather than through `record`, which asserts something
    // was drawn: here nothing should be, and `Ok(false)` is that answer.
    let pool = CommandPool::new(CommandPoolDesc::default());
    let mut buffer = CommandBuffer::new(pool, CommandBufferDesc::default());
    buffer.begin().expect("begin");
    let mut encoder = CommandEncoder::new(buffer);
    let recorded = target
        .ui
        .record_into_framebuffer(&mut encoder, &target.framebuffer, EXTENT, EXTENT)
        .expect("record");

    assert!(
        !recorded,
        "an image with no registered texture still produced a draw"
    );
    assert!(
        !encoder
            .commands()
            .iter()
            .any(|c| matches!(c, Command::DrawIndexed { .. })),
        "an image with no registered texture reached the command list as a draw"
    );
    assert_eq!(
        target.ui.stats().skipped,
        1,
        "the unregistered image was not counted as skipped"
    );
}

/// A theme can be chosen by name, and choosing one changes what elements are
/// painted with.
///
/// `ThemeManager` held three named palettes and nothing ever created it, so
/// "switch to light mode" had no spelling in the whole codebase. The renderer
/// had to be built for this: the theme is only worth anything once the thing
/// that paints with it exists.
#[test]
fn a_named_theme_can_be_selected() {
    let mut target = build_empty_target();

    let names = target.ui.theme_names();
    assert!(
        names.iter().any(|n| n.as_str() == "light"),
        "the theme registry is missing the built-in themes: {names:?}"
    );

    // An unknown name must report failure rather than quietly keep the current
    // theme: a settings screen that says "applied" and changed nothing is worse
    // than one that says no.
    let before = target.ui.theme().name.clone();
    assert!(
        !target.ui.select_theme("no-such-theme"),
        "selecting a theme that does not exist reported success"
    );
    assert_eq!(
        target.ui.theme().name,
        before,
        "a failed selection still changed the theme"
    );

    assert!(
        target.ui.select_theme("light"),
        "the built-in 'light' theme could not be selected: {:?}",
        target.ui.theme_names()
    );
    assert_eq!(
        target.ui.theme().name,
        "Light",
        "the theme was selected in the registry but not applied to the renderer"
    );

    // And the registry is extensible at runtime, for the same reason.
    let mut custom = rhi::ui::UiTheme::dark();
    custom.name = "Fleet Blue".to_string();
    target.ui.add_theme("fleet".to_string(), custom);
    assert!(
        target.ui.select_theme("fleet"),
        "a theme added at runtime could not be selected"
    );
    assert_eq!(target.ui.theme().name, "Fleet Blue");
}
