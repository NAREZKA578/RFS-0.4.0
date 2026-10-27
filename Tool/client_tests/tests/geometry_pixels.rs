//! Does the geometry pass actually put pixels in the framebuffer?
//!
//! Recording commands correctly is not the same as drawing: the pipeline, the
//! vertex layout, the descriptor set and the rasteriser state can all be wrong
//! in ways that record perfectly and produce an untouched framebuffer. This
//! test submits the real thing to an offscreen target and reads the pixels
//! back, so the answer is observed rather than inferred.
//!
//! It is separate from the command-list test on purpose — that one asserts
//! which commands were emitted, this one asserts what came out.

use std::sync::Arc;

use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::graph::RenderGraph;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::{GeometryItem, GeometryPass};
use rfs_client::render::scene::Scene;
use rfs_client::render::RenderContext;
use rhi::core::device::DeviceDesc;
use rhi::types::Features;
use rhi::{
    AttachmentDescription, AttachmentReference, Format, FramebufferAttachment, FramebufferDesc,
    LoadOp, PipelineBindPoint, RhiConfig, SampleCount, StoreOp, SubpassDescription, TextureUsage,
};

const EXTENT: u32 = 256;
const SHADER: &[u8] = include_bytes!("../assets/shaders/geometry.spv");

/// The colour the pass clears to, in the same order `ClearValue::color` takes.
const CLEAR: [f32; 3] = [0.05, 0.06, 0.08];

fn open_device() -> Arc<rhi::Device> {
    // Validation on, not `RhiConfig::default()` (which is `validation: false`).
    // This test reads pixels, so on its own it would pass even while the
    // submission behind those pixels was invalid. Needs
    // `VK_LAYER_PATH=C:\RFS-0.4.0\UPl\valayer`; without it the layer is skipped
    // silently.
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

#[test]
fn a_geometry_framebuffer_accepts_a_device_created_depth_view() {
    let device = open_device();

    // The bug this guards: the depth view was built with
    // `Texture::create_view`, which returns a CPU-side handle with no GPU
    // backing. Nothing complained at the point of the mistake — the frame that
    // used it failed with "framebuffer attachment has no GPU backing", reported
    // from the draw rather than from the creation.
    let depth = device
        .create_texture(
            64,
            64,
            1,
            Format::D32_SFLOAT,
            TextureUsage::DEPTH_STENCIL_ATTACHMENT,
            1,
        )
        .expect("the depth fixture's texture must be created");
    let depth_view = device
        .create_texture_view(
            &depth,
            &rhi::TextureViewDesc {
                texture: depth.clone(),
                format: None,
                view_type: rhi::TextureViewType::D2,
                aspects: rhi::TextureAspectFlags::DEPTH,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("depth view");
    assert!(
        depth_view.has_gpu_backing(),
        "a depth view created through the device must have GPU backing"
    );

    // And the render pass `create` builds must actually declare depth, since
    // the pipeline's depth state is derived from it.
    let (_pass, state) = GeometryPass::create(
        &device,
        Vec::new(),
        Format::RGBA8_UNORM,
        SHADER,
    )
    .expect("geometry pass");
    let render_pass = state.lock().expect("state lock").render_pass.clone();
    assert!(
        render_pass
            .desc()
            .subpasses
            .first()
            .is_some_and(|s| s.depth_stencil_attachment.is_some()),
        "the presentation render pass must declare a depth attachment"
    );
}

#[test]
fn the_geometry_pass_actually_lights_pixels() {
    let device = open_device();

    // A readable render pass: a presentation pass ends in `PresentSrc`, which
    // cannot be copied out, so it cannot be used to check what was drawn.
    let render_pass = device
        .create_gpu_render_pass(&rhi::RenderPassDesc {
            attachments: vec![AttachmentDescription {
                format: Format::RGBA8_UNORM,
                samples: SampleCount::X1,
                load_op: LoadOp::Clear,
                store_op: StoreOp::Store,
                stencil_load_op: LoadOp::DontCare,
                stencil_store_op: StoreOp::DontCare,
                initial_layout: rhi::TextureLayout::Undefined,
                final_layout: rhi::TextureLayout::TransferSrcOptimal,
            }],
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: rhi::TextureLayout::ColorAttachmentOptimal,
                }],
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");

    let texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the colour target must be created");
    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("colour attachment view");
    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![FramebufferAttachment { texture_view: view, layer: 0, mip_level: 0 }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    let mut mesh = Mesh::cube("cube", 1.0);
    mesh
        .create_buffers(&device)
        .expect("creating the cube's GPU buffers must succeed");
    let (pass, state) = GeometryPass::create_with_render_pass(
        &device,
        vec![GeometryItem {
            name: "cube".into(),
            vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
            index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
            index_type: mesh.index_type(),
            index_count: mesh.index_count(),
            model: glam::Mat4::IDENTITY,
            // A hand-built item: nothing will refresh its model matrix.
            entity_id: None,
        }],
        render_pass,
        SHADER,
    )
    .expect("geometry pass");

    // Pinned to the offscreen target, and sized to match it.
    GeometryPass::set_framebuffer(&state, Some(framebuffer), EXTENT, EXTENT);

    // Straight down the barrel at the cube, so the geometry must be inside the
    // frustum. A camera pointed elsewhere would produce a correct, empty frame.
    // Asserted because this test reads pixels back. If the upload failed the frame
    // would still record every command and still submit, and the pixel assertions
    // below would be measuring an uninitialised MVP rather than the geometry.
    assert!(
        GeometryPass::update_uniforms(
            &state,
            &device,
            glam::Mat4::look_at_rh(
                glam::Vec3::new(0.0, 0.0, 3.0),
                glam::Vec3::ZERO,
                glam::Vec3::Y,
            ),
            glam::Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
        ),
        "the uniform upload must succeed before pixels are read back"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode {
        name: "geometry".into(),
        pass: Box::new(pass),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });

    let mut scene = Scene::new("pixels");
    let mut context = RenderContext::headless(device.clone());

    // Sampled around this test's own submission. `graph.execute` records and
    // submits here, so everything the layer reports inside it is ours.
    //
    // Pixel assertions alone do not make the submission legal: a frame with the
    // right pixels is exactly what an invalid submission can still produce.
    let validation_before = rhi::validation_error_count();

    graph.execute(&device, &mut context, &mut scene);

    let validation_after = rhi::validation_error_count();
    assert_eq!(
        validation_after, validation_before,
        "the validator reported {} error(s) while producing a frame this test is \
         willing to call correct — correct output does not mean correct API use",
        validation_after - validation_before
    );

    // Bug №282: the readback has to wait for the GPU, and used to get that wait
    // for free.
    //
    // `submit_recorded` used to block on its fence before returning, so by the
    // time this point was reached the frame was always finished. With a ring of
    // frames in flight the CPU deliberately gets ahead of the GPU, so the
    // guarantee is gone and has to be asked for.
    //
    // Reading before the draw completes is not a flaky test, it is a wrong
    // answer: the image still holds its initial contents, which is why this
    // reported every pixel black rather than occasionally.
    device
        .wait_idle()
        .expect("wait for the submitted frame before reading it back");

    let bytes = device.download_texture(&texture, 0, 0).expect("readback");
    assert_eq!(
        bytes.len(),
        (EXTENT * EXTENT * 4) as usize,
        "the readback is not one RGBA pixel per texel"
    );

    // Count pixels that are neither the clear colour nor black. A frame that is
    // uniformly the clear colour means nothing was rasterised; a frame that is
    // uniformly black means the shader wrote nothing.
    let mut clear_pixels = 0u32;
    let mut black_pixels = 0u32;
    let mut lit_pixels = 0u32;
    for px in bytes.chunks_exact(4) {
        let rgb = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
        if rgb.iter().all(|c| *c < 0.02) {
            black_pixels += 1;
        } else if rgb
            .iter()
            .zip(CLEAR)
            .all(|(actual, clear)| (actual - clear).abs() < 0.02)
        {
            clear_pixels += 1;
        } else {
            lit_pixels += 1;
        }
    }
    let total = EXTENT * EXTENT;
    println!(
        "lit={lit_pixels} clear={clear_pixels} black={black_pixels} of {total}"
    );

    assert_ne!(
        black_pixels, total,
        "every pixel is black: the render pass never ran on the target"
    );    assert!(
        lit_pixels > 0,
        "no pixel is lit: {clear_pixels} are the clear colour and {black_pixels} are black, \
         so the pass recorded but rasterised nothing"
    );
    // The cube is small in frame, so most of the image is still the clear
    // colour. A pass that covered everything would mean the cube filled the
    // screen, which is not what a 1-unit cube at 3 units with a 90-degree
    // vertical field of view looks like.
    assert!(
        lit_pixels < total / 2,
        "{lit_pixels} of {total} pixels are lit: the cube is covering the whole target, \
         which suggests the projection or the vertex positions are wrong"
    );
}

/// Bug №654: `GeometryItem::model` has to move the geometry.
///
/// The field was public, documented, and set by every caller — and inert. The
/// uniform held one MVP for the whole pass, so every item was drawn with the
/// same matrix and the model's own value never reached the GPU. The only
/// reason this stayed invisible is that every caller in the repository passed
/// the identity matrix, so the correct answer and the broken one were the
/// same picture.
///
/// The test moves one cube to the right and requires the lit pixels to be
/// entirely in the right half of the frame. With the model ignored, the cube
/// is drawn at the centre instead, and the assertion fails on the pixel
/// positions rather than on a count.
///
/// The numbers: the camera is at 3 units with a 45-degree vertical field of
/// view, so the frame spans [-1.2426, 1.2426] in world units over 256 pixels —
/// about 103 pixels per unit. A 0.4-unit cube moved by 0.8 units spans
/// [0.6, 1.0], which is pixels [190, 231]: the whole cube is past the middle
/// of the frame. Unmoved, it spans pixels [107, 149], which straddles it.
#[test]
fn the_geometry_pass_honours_the_items_model_matrix() {
    let device = open_device();

    let render_pass = device
        .create_gpu_render_pass(&rhi::RenderPassDesc {
            attachments: vec![AttachmentDescription {
                format: Format::RGBA8_UNORM,
                samples: SampleCount::X1,
                load_op: LoadOp::Clear,
                store_op: StoreOp::Store,
                stencil_load_op: LoadOp::DontCare,
                stencil_store_op: StoreOp::DontCare,
                initial_layout: rhi::TextureLayout::Undefined,
                final_layout: rhi::TextureLayout::TransferSrcOptimal,
            }],
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: rhi::TextureLayout::ColorAttachmentOptimal,
                }],
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");

    let texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the colour target must be created");
    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("colour attachment view");
    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![FramebufferAttachment { texture_view: view, layer: 0, mip_level: 0 }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    let mut mesh = Mesh::cube("cube", 0.4);
    mesh.create_buffers(&device).expect("cube buffers");
    let (pass, state) = GeometryPass::create_with_render_pass(
        &device,
        vec![GeometryItem {
            name: "cube".into(),
            vertex_buffer: mesh.vertex_buffer().cloned().expect("vertex buffer"),
            index_buffer: mesh.index_buffer().cloned().expect("index buffer"),
            index_type: mesh.index_type(),
            index_count: mesh.index_count(),
            // The whole point of the test: the cube is moved to the right.
            model: glam::Mat4::from_translation(glam::Vec3::new(0.8, 0.0, 0.0)),
            // A hand-built item: nothing will refresh its model matrix.
            entity_id: None,
        }],
        render_pass,
        SHADER,
    )
    .expect("geometry pass");

    GeometryPass::set_framebuffer(&state, Some(framebuffer), EXTENT, EXTENT);

    assert!(
        GeometryPass::update_uniforms(
            &state,
            &device,
            glam::Mat4::look_at_rh(
                glam::Vec3::new(0.0, 0.0, 3.0),
                glam::Vec3::ZERO,
                glam::Vec3::Y,
            ),
            glam::Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
        ),
        "the uniform upload must succeed before pixels are read back"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode {
        name: "geometry".into(),
        pass: Box::new(pass),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });

    let mut scene = Scene::new("model");
    let mut context = RenderContext::headless(device.clone());
    graph.execute(&device, &mut context, &mut scene);
    device
        .wait_idle()
        .expect("wait for the submitted frame before reading it back");

    let bytes = device.download_texture(&texture, 0, 0).expect("readback");

    // The lit pixels' horizontal span. The cube is the only thing in the
    // frame, so every lit pixel belongs to it.
    let mut min_x = u32::MAX;
    let mut max_x = 0u32;
    let mut lit = 0u32;
    for (index, px) in bytes.chunks_exact(4).enumerate() {
        let rgb = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
        let is_clear = rgb
            .iter()
            .zip(CLEAR)
            .all(|(actual, clear)| (actual - clear).abs() < 0.02);
        if !is_clear && rgb.iter().any(|c| *c > 0.02) {
            let x = (index as u32) % EXTENT;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            lit += 1;
        }
    }

    assert!(lit > 0, "nothing was drawn, so the model cannot have moved it");
    assert!(
        min_x > EXTENT / 2,
        "the cube was moved to the right, so every lit pixel must be right of \
         the middle; the lit pixels start at column {min_x} and the frame is \
         {EXTENT} wide. The model matrix was not applied."
    );
    assert!(
        max_x < EXTENT,
        "the lit pixels run to column {max_x}, past the frame"
    );
}

/// The scene's entities must land where the scene says they are.
///
/// This is the whole product path in one assertion: a scene with two ships at
/// different places, the render layer asked what to draw
/// (`GeometryPass::items_for_scene`), and the picture showing the ships in
/// different places.
///
/// Before the bridge, the scene was read by nobody — `Scene::entities`,
/// `Entity::transform`, `Entity::mesh` and `Transform::matrix` had no callers —
/// and the model matrix was not applied anyway, so even a hand-built item with
/// a model would have been drawn at the origin. With both defects in place,
/// the two ships land on top of each other in the middle of the frame and the
/// right half is empty.
#[test]
fn the_scenes_entities_land_where_the_scene_says() {
    let device = open_device();

    let mut mesh = Mesh::cube("hull", 1.0);
    mesh.create_buffers(&device).expect("cube buffers");
    let mesh = Arc::new(mesh);

    let mut scene = Scene::new("two ships");
    for (name, x) in [("port", -2.5f32), ("starboard", 2.5f32)] {
        let mut entity = rfs_client::render::scene::entity::Entity::new(name);
        entity.add_component(
            rfs_client::render::scene::Transform::with_position(glam::Vec3::new(x, 0.0, 0.0)),
        );
        entity.add_component(rfs_client::render::scene::components::MeshComponent::new(
            mesh.clone(),
        ));
        scene.add_entity(entity);
    }

    // The render layer is asked what to draw, rather than being told.
    let built = GeometryPass::items_for_scene(&scene);
    assert!(
        built.skipped.is_empty(),
        "both entities have a mesh with GPU buffers, so neither should be skipped: {:?}",
        built.skipped
    );
    assert_eq!(built.items.len(), 2);
    assert_eq!(built.items[0].name, "port");
    assert_eq!(built.items[1].name, "starboard");

    // The offscreen target, so the frame can be read back.
    let render_pass = device
        .create_gpu_render_pass(&rhi::RenderPassDesc {
            attachments: vec![AttachmentDescription {
                format: Format::RGBA8_UNORM,
                samples: SampleCount::X1,
                load_op: LoadOp::Clear,
                store_op: StoreOp::Store,
                stencil_load_op: LoadOp::DontCare,
                stencil_store_op: StoreOp::DontCare,
                initial_layout: rhi::TextureLayout::Undefined,
                final_layout: rhi::TextureLayout::TransferSrcOptimal,
            }],
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: rhi::TextureLayout::ColorAttachmentOptimal,
                }],
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");
    let texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the colour target must be created");
    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("colour attachment view");
    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![FramebufferAttachment { texture_view: view, layer: 0, mip_level: 0 }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    let (pass, state) = GeometryPass::create_with_render_pass(
        &device,
        built.items,
        render_pass,
        SHADER,
    )
    .expect("geometry pass");
    GeometryPass::set_framebuffer(&state, Some(framebuffer), EXTENT, EXTENT);

    assert!(
        GeometryPass::update_uniforms(
            &state,
            &device,
            glam::Mat4::look_at_rh(
                glam::Vec3::new(0.0, 0.0, 8.0),
                glam::Vec3::ZERO,
                glam::Vec3::Y,
            ),
            glam::Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
        ),
        "the uniform upload must succeed before pixels are read back"
    );

    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode {
        name: "geometry".into(),
        pass: Box::new(pass),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });
    let mut context = RenderContext::headless(device.clone());
    let mut draw_scene = Scene::new("draw");
    graph.execute(&device, &mut context, &mut draw_scene);
    device
        .wait_idle()
        .expect("wait for the submitted frame before reading it back");

    let bytes = device.download_texture(&texture, 0, 0).expect("readback");
    let mut left = 0u32;
    let mut right = 0u32;
    for (index, px) in bytes.chunks_exact(4).enumerate() {
        let rgb = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
        let is_clear = rgb
            .iter()
            .zip(CLEAR)
            .all(|(actual, clear)| (actual - clear).abs() < 0.02);
        if !is_clear && rgb.iter().any(|c| *c > 0.02) {
            let x = (index as u32) % EXTENT;
            // The **outer** quarters, not the halves.
            //
            // A cube drawn at the origin straddles the middle of the frame, so
            // "there are lit pixels on both sides of the centre" is true even
            // when both ships were drawn in the same place. The outer quarters
            // are empty in that case and full in the right one: the ships are
            // 2.5 units either side of the origin and the camera is 8 units
            // away, so the half-width at that distance is 3.31 and the ships
            // land at columns 32 and 224.
            if x < EXTENT / 4 {
                left += 1;
            } else if x >= 3 * EXTENT / 4 {
                right += 1;
            }
        }
    }

    assert!(left > 0, "the port ship was not drawn on the left of the frame");
    assert!(
        right > 0,
        "the starboard ship was not drawn on the right of the frame. The scene's \
         transforms did not reach the GPU, so both ships were drawn in the same \
         place — at the origin, which is in the middle of the frame."
    );
}
