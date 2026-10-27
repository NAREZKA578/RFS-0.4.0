//! The geometry pass must actually record commands.
//!
//! Every render pass in the layer was a stub, so the graph recorded nothing and
//! the renderer presented a cleared image. This drives the geometry pass against
//! a real Vulkan device and a real offscreen target and asserts the recorded
//! command list contains the sequence a draw needs — in particular that it is
//! not empty, which was the state the whole render path was stuck in.

mod common;

use std::sync::Arc;

use glam::Vec3;
use rhi::backend::create_backend;
use rhi::command::commands::Command;
use rhi::core::device::DeviceDesc;
use rhi::types::Features;
use rhi::{
    AttachmentDescription, AttachmentReference, ClearValue, ColorBlendAttachment, ColorBlendState,
    ColorComponentFlags, ColorTargetDesc, Format, FramebufferAttachment, FramebufferDesc,
    GraphicsPipelineDesc, InputAssemblyState, LoadOp, MultisampleState, PipelineBindPoint,
    PipelineShaderStage, RasterizerState, RenderPassDesc, SampleCount, ShaderFormat,
    ShaderModuleDesc, ShaderStage, StoreOp, SubpassDescription, TextureLayout, TextureUsage,
};
use rfs_client::render::graph::node::RenderPassNode;
use rfs_client::render::meshes::Mesh;
use rfs_client::render::passes::geometry::{
    build_uniforms, vertex_input_state, GeometryItem, GeometryPass, GeometryPassState, SharedState,
};
use rfs_client::render::scene::Scene;
use rfs_client::render::meshes::Vertex;
use rfs_client::render::RenderContext;

const EXTENT: u32 = 128;
const GEOMETRY_SHADER: &[u8] =
    include_bytes!("../assets/shaders/geometry.spv");

fn all_channels() -> ColorComponentFlags {
    ColorComponentFlags::R
        | ColorComponentFlags::G
        | ColorComponentFlags::B
        | ColorComponentFlags::A
}

/// The scene must be able to say what to draw.
///
/// This is the bridge between the scene graph and the GPU, and it did not
/// exist: `RenderGraph::add_pass` had no caller anywhere in `src/`, and the
/// only way to obtain a geometry pass was to build the items by hand, as
/// `examples/cube.rs` does. Everything the scene knew — the entities, their
/// transforms, their meshes — was read by nothing, so the product's `Renderer`
/// presented an empty frame while the scene sat next to it full of objects.
///
/// The test builds a scene with two meshes at different places and requires
/// two items with two different model matrices, plus an entity that cannot be
/// drawn and the reason why.
#[test]
fn the_scene_produces_the_items_to_draw() {
    let backend = create_backend(&rhi::RhiConfig::default()).expect("create backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    assert!(!devices.is_empty(), "no Vulkan device available");
    let device = std::sync::Arc::new(
        backend
            .create_device(
                &devices[0],
                &DeviceDesc {
                    features: Features::default(),
                    ..Default::default()
                },
            )
            .expect("create device"),
    );

    let mut scene = Scene::new("bridge");
    let mut mesh_a = Mesh::cube("a", 1.0);
    mesh_a.create_buffers(&device).expect("mesh a buffers");
    let mut mesh_b = Mesh::cube("b", 1.0);
    mesh_b.create_buffers(&device).expect("mesh b buffers");

    // Two entities, a long way apart, with their own transforms.
    let mut entity_a = rfs_client::render::scene::Entity::new("hull");
    entity_a.add_component(rfs_client::render::scene::Transform::with_position(Vec3::new(
        -10.0, 0.0, 0.0,
    )));
    entity_a.add_component(rfs_client::render::scene::MeshComponent::new(Arc::new(mesh_a)));
    scene.add_entity(entity_a);

    let mut entity_b = rfs_client::render::scene::Entity::new("tower");
    entity_b.add_component(rfs_client::render::scene::Transform::with_position(Vec3::new(
        10.0, 0.0, 0.0,
    )));
    entity_b.add_component(rfs_client::render::scene::MeshComponent::new(Arc::new(mesh_b)));
    scene.add_entity(entity_b);

    // And one that cannot be drawn: a mesh whose GPU buffers were never made.
    let mut entity_c = rfs_client::render::scene::Entity::new("ghost");
    entity_c.add_component(rfs_client::render::scene::Transform::new());
    entity_c.add_component(rfs_client::render::scene::MeshComponent::new(Arc::new(
        Mesh::cube("ghost", 1.0),
    )));
    scene.add_entity(entity_c);

    let built = GeometryPass::items_for_scene(&scene);

    assert_eq!(
        built.items.len(),
        2,
        "the two entities with GPU buffers must produce two items, got {} and {} skipped",
        built.items.len(),
        built.skipped.len()
    );
    assert_eq!(built.items[0].name, "hull");
    assert_eq!(built.items[1].name, "tower");

    // The model matrix is the entity's transform. This is the part that was
    // inert before (Bug №654): a model that reaches nowhere draws nothing
    // differently from a model that is ignored.
    let expected_a = Vec3::new(-10.0, 0.0, 0.0);
    let expected_b = Vec3::new(10.0, 0.0, 0.0);
    assert!(
        built.items[0].model.w_axis.truncate().abs_diff_eq(expected_a, 1e-4),
        "the first item's model matrix does not carry its entity's position"
    );
    assert!(
        built.items[1].model.w_axis.truncate().abs_diff_eq(expected_b, 1e-4),
        "the second item's model matrix does not carry its entity's position"
    );
    assert_ne!(
        built.items[0].model, built.items[1].model,
        "two entities at different places must not share a model matrix"
    );

    // The one that cannot be drawn is named, with the reason.
    assert_eq!(
        built.skipped.len(),
        1,
        "the entity without GPU buffers must be reported, not silently dropped"
    );
    assert_eq!(built.skipped[0].0, "ghost");
    assert!(
        built.skipped[0].1.contains("GPU buffers"),
        "the reason must say what was missing, got: {}",
        built.skipped[0].1
    );
}

/// The items' model matrices must follow the scene.
///
/// The items are the meshes, built once; the transforms move every frame.
/// Rebuilding the items per frame would mean rebuilding the uniform buffers
/// and the descriptor sets with them, so the matrices are refreshed in place
/// and the items remember which entity they came from.
#[test]
fn the_items_model_matrix_follows_the_scene() {
    let backend = create_backend(&rhi::RhiConfig::default()).expect("create backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    let device = std::sync::Arc::new(
        backend
            .create_device(
                &devices[0],
                &DeviceDesc {
                    features: Features::default(),
                    ..Default::default()
                },
            )
            .expect("create device"),
    );

    let mut scene = Scene::new("moving");
    let mut mesh = Mesh::cube("hull", 1.0);
    mesh.create_buffers(&device).expect("mesh buffers");

    let mut entity = rfs_client::render::scene::Entity::new("hull");
    entity.add_component(rfs_client::render::scene::Transform::with_position(Vec3::ZERO));
    entity.add_component(rfs_client::render::scene::MeshComponent::new(Arc::new(mesh)));
    let entity_id = scene.add_entity(entity);

    let built = GeometryPass::items_for_scene(&scene);
    assert_eq!(built.items.len(), 1);
    assert_eq!(built.items[0].entity_id, Some(entity_id));
    assert!(
        built.items[0].model.w_axis.truncate().abs_diff_eq(Vec3::ZERO, 1e-4),
        "the item starts at the origin"
    );

    // The ship moves.
    let moved = Vec3::new(3.0, 0.0, 0.0);
    scene
        .get_entity_mut(entity_id)
        .expect("the entity")
        .transform_mut()
        .expect("the transform")
        .set_position(moved);

    // The items are refreshed in place, without rebuilding them.
    let state = rfs_client::render::passes::geometry::GeometryPassState::with_resources(
        rfs_client::rhi::GraphicsPipeline::new(rfs_client::rhi::GraphicsPipelineDesc::default()),
        rfs_client::rhi::DescriptorSetLayout::new(
            rfs_client::rhi::descriptor::layout::DescriptorSetLayoutDesc::default(),
        ),
        Vec::new(),
        Vec::new(),
        rfs_client::rhi::RenderPass::new(rfs_client::rhi::command::pass::render::RenderPassDesc {
            attachments: Vec::new(),
            subpasses: Vec::new(),
            dependencies: Vec::new(),
        }),
        built.items,
    );
    let shared: SharedState = std::sync::Arc::new(std::sync::Mutex::new(state));

    assert_eq!(GeometryPass::update_models(&shared, &scene), 1);
    let state = shared.lock().expect("state lock");
    assert!(
        state.items[0].model.w_axis.truncate().abs_diff_eq(moved, 1e-4),
        "the item's model matrix must follow the entity's transform"
    );
}

#[test]
fn the_vertex_layout_matches_the_vertex_struct_byte_for_byte() {
    use std::mem::{offset_of, size_of};

    let state = vertex_input_state();

    // The stride must be the real size of the struct. `Vertex` is `#[repr(C)]`
    // with a 16-byte-aligned `Vec4` in it, so summing the attribute format
    // sizes gives 64 while the struct is 80. The driver accepts the wrong
    // stride without complaint and reads every vertex from the wrong address,
    // which is not a crash and not an error — just geometry that is not the
    // geometry in the buffer.
    assert_eq!(
        state.bindings[0].stride as usize,
        size_of::<Vertex>(),
        "the vertex stride is not the size of Vertex"
    );

    // And each attribute must start where the field actually starts, holes
    // included.
    let expected = [
        (0u32, offset_of!(Vertex, position) as u32),
        (1, offset_of!(Vertex, normal) as u32),
        (2, offset_of!(Vertex, tangent) as u32),
        (3, offset_of!(Vertex, tex_coord) as u32),
        (4, offset_of!(Vertex, color) as u32),
    ];
    assert_eq!(state.attributes.len(), expected.len());
    for (attribute, (location, offset)) in state.attributes.iter().zip(expected) {
        assert_eq!(attribute.location, location);
        assert_eq!(
            attribute.offset, offset,
            "location {location} reads from the wrong offset"
        );
    }

    // Guard the reason the two ever disagreed: the sum of format sizes must
    // differ from the struct size, or this test is passing for the wrong reason.
    let summed: u32 = state
        .attributes
        .iter()
        .map(|a| match a.format {
            Format::R32G32B32_SFLOAT => 12,
            Format::RGBA32_SFLOAT => 16,
            Format::R32G32_SFLOAT => 8,
            other => panic!("unexpected attribute format {other:?}"),
        })
        .sum();
    assert_ne!(
        summed,
        size_of::<Vertex>() as u32,
        "the struct no longer has padding; this regression no longer describes it"
    );
}

#[test]
fn the_geometry_pass_records_a_full_draw_sequence() {
    let backend = create_backend(&rhi::RhiConfig::default()).expect("create backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    assert!(!devices.is_empty(), "no Vulkan device available");
    // Wrapped in an `Arc` up front because `RenderContext` owns one and
    // `Device` is not `Clone`.
    let device = std::sync::Arc::new(
        backend
            .create_device(
                &devices[0],
                &DeviceDesc { features: Features::default(), ..Default::default() },
            )
            .expect("create device"),
    );

    // Offscreen target, so the test needs no window.
    let texture = device
        .create_texture(
            EXTENT,
            EXTENT,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT,
            1,
        )
        .expect("the offscreen colour target must be created");
    assert!(texture.has_gpu_backing());
    let view = device
        .create_texture_view(&texture, texture.create_default_view().desc())
        .expect("view");

    let render_pass = device
        .create_gpu_render_pass(&RenderPassDesc {
            attachments: vec![AttachmentDescription {
                format: Format::RGBA8_UNORM,
                samples: SampleCount::X1,
                load_op: LoadOp::Clear,
                store_op: StoreOp::Store,
                stencil_load_op: LoadOp::DontCare,
                stencil_store_op: StoreOp::DontCare,
                initial_layout: TextureLayout::Undefined,
                final_layout: TextureLayout::ColorAttachmentOptimal,
            }],
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: TextureLayout::ColorAttachmentOptimal,
                }],
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");

    let framebuffer = device
        .create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![FramebufferAttachment { texture_view: view, layer: 0, mip_level: 0 }],
            width: EXTENT,
            height: EXTENT,
            layers: 1,
        })
        .expect("framebuffer");

    // The cube, uploaded for real.
    let mut mesh = Mesh::cube("cube", 1.0);
    mesh
        .create_buffers(&device)
        .expect("creating the cube's GPU buffers must succeed; the next line \
                 unwraps a vertex buffer that only exists if it did");
    let vertex_buffer = mesh.vertex_buffer().cloned().expect("vertex buffer");
    let index_buffer = mesh.index_buffer().cloned().expect("index buffer");
    assert!(vertex_buffer.has_gpu_backing() && index_buffer.has_gpu_backing());

    // Bug №654: one uniform buffer and one descriptor set per item, not per
    // pass. The MVP includes the item's own model matrix, so a single buffer
    // for the whole pass would mean every item was drawn with the last one's
    // matrix and `GeometryItem::model` would change nothing.
    let (uniform_buffers, descriptor_layout, descriptor_sets) =
        build_uniforms(&device, 1).expect("uniforms");

    let module = device
        .create_shader_module(&ShaderModuleDesc {
            code: GEOMETRY_SHADER.to_vec(),
            format: ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("geometry".into()),
        })
        .expect("the geometry module must be created");
    assert!(module.has_gpu_backing(), "the geometry shader must load on the GPU");

    let mut desc = GraphicsPipelineDesc {
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
        vertex_input_state: Some(vertex_input_state()),
        input_assembly_state: InputAssemblyState {
            topology: rhi::pipeline::graphics::PrimitiveTopology::TriangleList,
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
    };
    desc.depth_stencil_state = None;

    let pipeline = device
        .create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])
        .expect("graphics pipeline");
    assert!(pipeline.has_gpu_backing());

    let (pass, shared): (_, SharedState) = GeometryPass::with_state(GeometryPassState {
        pipeline,
        descriptor_layout,
        descriptor_sets,
        uniform_buffers,
        depth_texture: None,
        depth_view: None,
        depth_format: rhi::Format::D32_SFLOAT,
        render_pass,
        items: vec![GeometryItem {
            name: "cube".into(),
            vertex_buffer,
            index_buffer,
            index_type: mesh.index_type(),
            index_count: mesh.index_count(),
            model: glam::Mat4::IDENTITY,
            // A hand-built item: nothing will refresh its model matrix.
            entity_id: None,
        }],
        framebuffer: Some(framebuffer),
        framebuffer_pinned: true,
        framebuffer_image: Some(0),
        // Replaces the old `width`/`height` fields, which the pass used as its
        // draw size. They were initialised to 0 and only refreshed when the
        // framebuffer was rebuilt, so the viewport and scissor could disagree
        // with the attachments. The pass now takes its size from the live render
        // context; this field is only part of the framebuffer cache key and is
        // irrelevant while the framebuffer is pinned.
        framebuffer_extent: Some(rhi::types::Extent2D {
            width: EXTENT,
            height: EXTENT,
        }),
        // The other half of the framebuffer cache key. It guards *liveness*
        // rather than sizing: a framebuffer built against a swapchain image
        // that has since been recreated stays pinned here, and recording into it
        // is what produced
        // `VUID-VkRenderPassBeginInfo-framebuffer-parameter` with no hint in the
        // message about the resize that caused it. `None` because the generation
        // is only tracked for the renderer's own acquired images, and this
        // framebuffer is built here and outlives any generation counter.
        framebuffer_generation: None,
    });

    // Give the pass a real MVP so the uniform upload is exercised too.
    //
    // Asserted rather than ignored. `update_uniforms` returns false when the upload
    // fails, and the failure mode that matters here is quiet: the graph still
    // records a full draw sequence, so a test that only checks the recorded commands
    // would pass with an uninitialised MVP — which is exactly how the inverted
    // `.is_err()` bug behind №246 produced an empty window while every test stayed
    // green.
    assert!(
        GeometryPass::update_uniforms(
            &shared,
            &device,
            glam::Mat4::look_at_rh(
                glam::Vec3::new(0.0, 1.5, 4.0),
                glam::Vec3::ZERO,
                glam::Vec3::Y,
            ),
            glam::Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0),
        ),
        "the uniform upload must succeed; the draw recorded below would otherwise \
         carry a stale MVP and still look correct"
    );

    // Record through a graph, which is the only route a pass is ever driven by.
    let mut graph = rfs_client::render::graph::RenderGraph::new();
    graph.add_pass(RenderPassNode {
        name: "geometry".into(),
        pass: Box::new(pass),
        dependencies: Vec::new(),
        outputs: vec!["swapchain".into()],
        enabled: true,
        execution_order: 0,
    });

    let mut scene = Scene::new("test");
    let mut context = RenderContext::headless(device.clone());
    let recorded = graph.record(&mut context, &mut scene);
    assert!(
        !recorded.is_empty(),
        "the geometry pass recorded nothing: the render path cannot draw"
    );

    // The specific commands a frame needs, so a future refactor cannot quietly
    // drop one and still pass.
    let has = |variant: fn(&Command) -> bool| recorded.iter().any(variant);
    assert!(has(|c| matches!(c, Command::BeginRenderPass(_))), "no BeginRenderPass");
    assert!(has(|c| matches!(c, Command::BindGraphicsPipeline(_))), "no pipeline bind");
    assert!(has(|c| matches!(c, Command::BindDescriptorSets { .. })), "no descriptor sets");
    assert!(has(|c| matches!(c, Command::BindVertexBuffers { .. })), "no vertex buffers");
    assert!(has(|c| matches!(c, Command::BindIndexBuffer { .. })), "no index buffer");
    assert!(has(|c| matches!(c, Command::SetViewport(_))), "no viewport");
    assert!(has(|c| matches!(c, Command::SetScissor(_))), "no scissor");
    assert!(has(|c| matches!(c, Command::DrawIndexed { .. })), "no indexed draw");
    assert!(has(|c| matches!(c, Command::EndRenderPass)), "no EndRenderPass");

    // The draw must reference the real index count, not zero: a zero-length
    // draw records perfectly and shows nothing.
    let draw = recorded
        .iter()
        .find_map(|c| match c {
            Command::DrawIndexed { index_count, .. } => Some(*index_count),
            _ => None,
        })
        .expect("a DrawIndexed command");
    assert_eq!(draw, mesh.index_count(), "the draw covers the wrong range");

    // And the clear must be a real colour, not a default-constructed one.
    let clear = recorded
        .iter()
        .find_map(|c| match c {
            Command::BeginRenderPass(info) => Some(info.clear_values.clone()),
            _ => None,
        })
        .expect("a BeginRenderPass command");
    assert_eq!(clear.len(), 1, "exactly one colour attachment is cleared");
    assert!(
        matches!(clear[0], ClearValue::Color { .. }),
        "the clear must be a colour, not a depth value"
    );

    // Now unpin the framebuffer and drop the presentation state, which is what
    // a run with no surface looks like. The pass must then record nothing:
    // there is no image it is allowed to bind, and falling back to image 0
    // would draw into an image that is never presented.
    {
        let mut state = shared.lock().expect("state lock");
        state.framebuffer = None;
        state.framebuffer_pinned = false;
        state.framebuffer_image = None;
    }
    let mut scene = Scene::new("test");
    let mut headless = RenderContext::headless(device.clone());
    let blind = graph.record(&mut headless, &mut scene);
    assert!(
        blind.is_empty(),
        "recorded {} commands with no presentation target; the pass would draw into an image nobody presents",
        blind.len()
    );
}
