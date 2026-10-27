//! GBuffer Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This pass renders the scene geometry to multiple render targets (MRT):
//! - Position (XYZ + depth)
//! - Normal (XYZ)
//! - Albedo (RGB) + Roughness (A)
//! - Material (Metallic, AO, Emissive)
//!
//! It is the first pass of the deferred chain: the geometry pass fills these
//! four targets, and the lighting pass resolves them into a lit image. The
//! shader is `gbuffer.wgsl`; its uniform block is `mvp`, `model` and
//! `normal_matrix` — three matrices, so the uniform is 192 bytes, not the 112
//! the forward geometry shader uses. That is why the two passes cannot share
//! their uniform buffers: the layouts are different, and a uniform buffer is
//! read by the shader according to the shader's own declaration.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::passes::base::BaseRenderPass;
use crate::render::passes::geometry::{vertex_input_state, GeometryItem};
use crate::render::scene::Scene;
use crate::rhi::{
    AttachmentDescription, AttachmentReference, Buffer, BufferUsage, Command, CommandEncoder,
    DescriptorInfo, DescriptorSet, DescriptorSetLayout, DescriptorType, DescriptorWrite,
    Framebuffer, FramebufferAttachment, FramebufferDesc, GraphicsPipeline, Offset2D, Rect2D,
    RenderPassDesc, ShaderStage, SubpassDescription, Texture, TextureView, Viewport,
};

/// The GBuffer's four colour targets and the depth buffer, in attachment order.
///
/// The order is load-bearing: the render pass's attachments, the subpass's
/// colour references, the framebuffer's views and the shader's output
/// locations all number them the same way, and the shader's
/// `@location(0..3)` are the position, normal, albedo and material outputs.
pub const POSITION_FORMAT: crate::rhi::Format = crate::rhi::Format::RGBA32_SFLOAT;
pub const NORMAL_FORMAT: crate::rhi::Format = crate::rhi::Format::RGBA32_SFLOAT;
pub const ALBEDO_FORMAT: crate::rhi::Format = crate::rhi::Format::RGBA8_UNORM;
pub const MATERIAL_FORMAT: crate::rhi::Format = crate::rhi::Format::RGBA8_UNORM;
pub const DEPTH_FORMAT: crate::rhi::Format = crate::rhi::Format::D32_SFLOAT;

/// The names the GBuffer's targets have in the graph's resource map.
///
/// The passes name the graph's resources, and the names have to agree exactly:
/// `lighting` asks for "gbuffer_position" and reads the graph's resource under
/// that name, so a mismatch is a silently empty attachment rather than an
/// error.
pub const TARGET_POSITION: &str = "gbuffer_position";
pub const TARGET_NORMAL: &str = "gbuffer_normal";
pub const TARGET_ALBEDO: &str = "gbuffer_albedo";
pub const TARGET_MATERIAL: &str = "gbuffer_material";
pub const TARGET_DEPTH: &str = "depth";

/// The uniform block size: `mvp`, `model` and `normal_matrix`, three matrices
/// of sixteen floats.
const UNIFORM_SIZE: u64 = 48 * 4;

/// One item's uniform buffer and the descriptor set that points at it.
///
/// Per item, not per pass: the model matrix is part of the MVP, and one
/// buffer for the whole pass would mean the last item written was the one
/// every draw used. That is the defect #287 was about, and the same shape of
/// fix applies here.
pub struct ItemUniform {
    pub buffer: Buffer,
    pub descriptor_set: DescriptorSet,
}

/// Everything the pass needs to record, shared with the caller.
///
/// The caller keeps the handle and the pass keeps its own copy: the pass must
/// not be able to observe itself, and the caller must not be able to record
/// commands behind the graph's back.
pub struct GBufferPassState {
    pub pipeline: GraphicsPipeline,
    pub descriptor_layout: DescriptorSetLayout,
    /// One uniform buffer and one descriptor set per item, in item order.
    pub uniforms: Vec<ItemUniform>,
    /// The geometry to draw.
    pub items: Vec<GeometryItem>,
    pub render_pass: crate::rhi::RenderPass,
    /// Built from the graph's resources once they exist.
    pub framebuffer: Option<Framebuffer>,
}

/// The handle the caller keeps.
pub type SharedState = Arc<Mutex<GBufferPassState>>;

/// GBuffer render pass
pub struct GBufferPass {
    base: BaseRenderPass,
    state: Option<SharedState>,
}

impl GBufferPass {
    /// The pass's base, with the targets it produces.
    ///
    /// Both constructors use it. `with_state` used to build a *fresh* base with
    /// no outputs, so the pass declared nothing and the graph — which creates a
    /// resource when a pass in the frame names it — created none: the pass
    /// had no framebuffer and drew nothing. The symptom looked like a broken
    /// pass; the cause was a base that had forgotten what it produced.
    fn base() -> BaseRenderPass {
        let mut base = BaseRenderPass::new("gbuffer");

        // Add outputs
        base.add_output(TARGET_POSITION);
        base.add_output(TARGET_NORMAL);
        base.add_output(TARGET_ALBEDO);
        base.add_output(TARGET_MATERIAL);
        base.add_output(TARGET_DEPTH);

        base
    }

    pub fn new() -> Self {
        Self {
            base: Self::base(),
            state: None,
        }
    }

    /// Build a pass around a new shared state and return both.
    pub     fn with_state(state: GBufferPassState) -> (Self, SharedState) {
        let shared: SharedState = Arc::new(Mutex::new(state));
        let pass = Self {
            base: Self::base(),
            state: Some(shared.clone()),
        };
        (pass, shared)
    }

    /// Build a pass that draws `items` into the GBuffer.
    ///
    /// `spirv` is taken as bytes rather than read from a path, so the render
    /// layer carries no knowledge of where assets live; the embedder uses
    /// `include_bytes!`.
    ///
    /// The targets themselves are **not** created here. They belong to the
    /// graph: the lighting pass reads them, and two owners for one texture is
    /// how a frame ends up writing into an object somebody else destroyed. The
    /// graph creates them because this pass and the lighting pass both name
    /// them, and the frame hands them to the pass in `initialize`.
    pub fn create(
        device: &crate::rhi::Device,
        items: Vec<GeometryItem>,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::PrimitiveTopology;
        use crate::rhi::{
            GraphicsPipelineDesc, InputAssemblyState, MultisampleState, PipelineShaderStage,
            RasterizerState, ShaderFormat, ShaderModuleDesc,
        };

        let render_pass = device.create_gpu_render_pass(&render_pass_desc())?;

        // One uniform buffer and one descriptor set per item. See `ItemUniform`.
        let descriptor_layout = device
            .create_descriptor_set_layout(&crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
                bindings: vec![crate::rhi::descriptor::layout::DescriptorBinding {
                    binding: 0,
                    ty: DescriptorType::UniformBuffer,
                    count: 1,
                    stages: ShaderStage::VERTEX | ShaderStage::FRAGMENT,
                    immutable_samplers: None,
                }],
            })
            .map_err(|e| RhiError::BackendError(format!("the gbuffer descriptor layout: {e}")))?;
        let mut uniforms = Vec::with_capacity(items.len());
        for _ in 0..items.len() {
            // The uniform is host-visible: the frame writes it directly, every
            // frame, for every item. A device-local buffer would need a
            // staging copy per item per frame, and the upload would fail
            // outright — "buffer memory is not host mapped and the buffer
            // lacks transfer usage" — because the buffer has no transfer
            // usage to stage through.
            let buffer = device.create_buffer_from_desc(&crate::rhi::BufferDesc {
                size: UNIFORM_SIZE,
                usage: BufferUsage::UNIFORM,
                memory_flags: crate::rhi::types::MemoryPropertyFlags::HOST_VISIBLE
                    | crate::rhi::types::MemoryPropertyFlags::HOST_COHERENT,
                sharing_mode: crate::rhi::types::SharingMode::Exclusive,
                queue_family_indices: Vec::new(),
            })?;
            // №642: `create_buffer_from_desc` returns the backend's refusal
            // as an Err now, so the_ok value is one the backend built and the
            // has_gpu_backing() guard had nothing left to catch.
            let set = device.create_descriptor_set(&descriptor_layout)?;
            device.write_descriptors(
                &set,
                &[DescriptorWrite {
                    dst_set: set.clone(),
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(buffer.clone(), 0, UNIFORM_SIZE)],
                }],
            )?;
            uniforms.push(ItemUniform { buffer, descriptor_set: set });
        }

        let module = device
            .create_shader_module(&ShaderModuleDesc {
                code: spirv.to_vec(),
                format: ShaderFormat::SpirV,
                entry_point: Some("vs_main".into()),
                name: Some("gbuffer".into()),
            })
            .map_err(|e| RhiError::BackendError(format!("the gbuffer shader module: {e}")))?;

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
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            rasterizer_state: Some(RasterizerState::double_sided()),
            multisample_state: MultisampleState {
                sample_count: crate::rhi::SampleCount::X1,
                ..Default::default()
            },
            // Four colour targets, so four blend attachments — and the write
            // mask has to be set **explicitly** on each one. `ColorComponentFlags`
            // derives `Default`, and for a bitflags type that default is the
            // *empty* set: a pipeline with an empty write mask blends into
            // nowhere, which is legal, silent, and looks exactly like a shader
            // that does nothing.
            color_blend_state: Some(crate::rhi::ColorBlendState {
                attachments: vec![
                    blend_attachment(),
                    blend_attachment(),
                    blend_attachment(),
                    blend_attachment(),
                ],
                ..Default::default()
            }),
            ..Default::default()
        };
        // The render pass has a depth attachment, so the pipeline's depth
        // state has to agree with it: a pipeline that declares a depth test
        // against a pass with no depth attachment is invalid.
        desc.depth_stencil_state = Some(crate::rhi::DepthStencilState::enabled());

        let pipeline =
            device.create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])?;
        // №642: an Ok pipeline is one the backend built — the refusal is an Err.

        Ok(Self::with_state(GBufferPassState {
            pipeline,
            descriptor_layout,
            uniforms,
            items,
            render_pass,
            framebuffer: None,
        }))
    }

    /// Write this frame's uniforms: the MVP, the item's model matrix and the
    /// normal matrix.
    ///
    /// The normal matrix is the inverse transpose of the model's upper 3×3, so
    /// that a non-uniform scale does not tilt the normals. The shader declares
    /// it as a `mat4x4` because WGSL has no 3×3 matrix type, so the fourth
    /// column is zero.
    ///
    /// Returns whether the GPU now holds this frame's uniforms. A failure on
    /// one item fails the whole frame: a partial update leaves some items
    /// drawn with the previous frame's matrix.
    pub fn update_uniforms(
        state: &SharedState,
        device: &crate::rhi::Device,
        view: glam::Mat4,
        projection: glam::Mat4,
    ) -> bool {
        let Ok(state) = state.lock() else {
            return false;
        };
        let mut all_ok = true;
        for (index, item) in state.items.iter().enumerate() {
            let Some(uniform) = state.uniforms.get(index) else {
                eprintln!(
                    "[gbuffer] no uniform for item {:?} ({} of {} have one)",
                    item.name,
                    state.uniforms.len(),
                    state.items.len()
                );
                all_ok = false;
                continue;
            };
            let mvp = projection * view * item.model;
            let normal = normal_matrix(item.model);

            // Column-major, so the floats go down unchanged.
            let mut data: [f32; 48] = [0.0; 48];
            data[..16].copy_from_slice(&mvp.to_cols_array());
            data[16..32].copy_from_slice(&item.model.to_cols_array());
            data[32..48].copy_from_slice(&normal.to_cols_array());

            if let Err(e) = device.upload_buffer(&uniform.buffer, &data) {
                eprintln!("[gbuffer] uniform upload failed for {}: {e}", item.name);
                all_ok = false;
            }
        }
        all_ok
    }

    /// Refresh the items' model matrices from the scene.
    ///
    /// The items are the meshes and they are built once; the transforms move
    /// every frame. Same shape as the geometry pass's `update_models`.
    pub fn update_models(state: &SharedState, scene: &Scene) -> usize {
        let Ok(mut state) = state.lock() else {
            return 0;
        };
        let mut updated = 0;
        for item in state.items.iter_mut() {
            let Some(entity_id) = item.entity_id else {
                continue;
            };
            let Some(entity) = scene.get_entity(entity_id) else {
                continue;
            };
            let Some(transform) = entity.transform() else {
                continue;
            };
            item.model = transform.matrix();
            updated += 1;
        }
        updated
    }
}

/// The inverse transpose of the model's upper 3×3, as a 4×4.
///
/// The fourth row and column are zero: WGSL has no 3×3 matrix, and the
/// shader reads a `mat4x4` and uses the upper 3×3.
fn normal_matrix(model: glam::Mat4) -> glam::Mat4 {
    let upper = glam::Mat3::from_mat4(model);
    let inverse_transpose = upper.inverse().transpose();
    glam::Mat4::from_mat3(inverse_transpose)
}

/// A colour blend attachment that writes every channel.
fn blend_attachment() -> crate::rhi::ColorBlendAttachment {
    crate::rhi::ColorBlendAttachment {
        color_write_mask: crate::rhi::ColorComponentFlags::R
            | crate::rhi::ColorComponentFlags::G
            | crate::rhi::ColorComponentFlags::B
            | crate::rhi::ColorComponentFlags::A,
        ..crate::rhi::ColorTargetDesc::default()
    }
}

/// The GBuffer's render pass: four colour targets and a depth buffer.
fn render_pass_desc() -> RenderPassDesc {
    let colour = |format| AttachmentDescription {
        format,
        samples: crate::rhi::SampleCount::X1,
        load_op: crate::rhi::LoadOp::Clear,
        store_op: crate::rhi::StoreOp::Store,
        stencil_load_op: crate::rhi::LoadOp::DontCare,
        stencil_store_op: crate::rhi::StoreOp::DontCare,
        // Undefined in, readable out: the lighting pass samples these four.
        initial_layout: crate::rhi::TextureLayout::Undefined,
        final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
    };
    RenderPassDesc {
        attachments: vec![
            colour(POSITION_FORMAT),
            colour(NORMAL_FORMAT),
            colour(ALBEDO_FORMAT),
            colour(MATERIAL_FORMAT),
            AttachmentDescription {
                format: DEPTH_FORMAT,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::Clear,
                stencil_store_op: crate::rhi::StoreOp::Store,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
            },
        ],
        subpasses: vec![SubpassDescription {
            pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
            color_attachments: (0..4)
                .map(|attachment| AttachmentReference {
                    attachment,
                    layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                })
                .collect(),
            depth_stencil_attachment: Some(AttachmentReference {
                attachment: 4,
                layout: crate::rhi::TextureLayout::DepthStencilAttachmentOptimal,
            }),
            ..Default::default()
        }],
        dependencies: vec![],
    }
}

/// Take a texture and its view out of the graph's resource map.
fn target<'a>(
    resources: &'a HashMap<String, GraphResource>,
    name: &str,
) -> Option<(&'a Texture, &'a TextureView)> {
    let resource = resources.get(name)?;
    let texture = resource.texture()?;
    let view = resource.view()?;
    Some((texture, view))
}

impl RenderPass for GBufferPass {
    fn name(&self) -> &str {
        self.base.name()
    }

    fn dependencies(&self) -> &[PassDependency] {
        self.base.dependencies()
    }

    fn outputs(&self) -> &[String] {
        self.base.outputs()
    }

    fn initialize(
        &mut self,
        device: &crate::rhi::Device,
        _context: &RenderContext,
        resources: &mut HashMap<String, GraphResource>,
    ) {
        // The targets belong to the graph. The pass asked for them by name, the
        // graph created them because both ends of the dependency are in this
        // frame, and here they are.
        //
        // A missing target is reported by name and the pass does without it —
        // a frame that draws nothing is recoverable, a process that aborts is
        // not.
        let mut missing = Vec::new();
        let mut views: Vec<TextureView> = Vec::with_capacity(5);
        let mut width = 0;
        let mut height = 0;
        for name in [
            TARGET_POSITION,
            TARGET_NORMAL,
            TARGET_ALBEDO,
            TARGET_MATERIAL,
            TARGET_DEPTH,
        ] {
            match target(resources, name) {
                Some((texture, view)) => {
                    if width == 0 {
                        width = texture.width();
                        height = texture.height();
                    }
                    views.push(view.clone());
                }
                None => {
                    missing.push(name);
                }
            }
        }
        if !missing.is_empty() {
            crate::error!(
                "gbuffer",
                "the graph has no target(s) {missing:?}; the pass will not draw"
            );
            if let Ok(mut state) = self.state.as_ref().expect("the state exists").lock() {
                state.framebuffer = None;
            }
            return;
        }

        let Some(state) = self.state.as_ref() else {
            return;
        };
        let Ok(mut state) = state.lock() else {
            return;
        };
        let render_pass = state.render_pass.clone();
        // The framebuffer has to be a real one: `Framebuffer::new` is a
        // CPU-side description with no native handle, and a render pass begun
        // against it is refused by the translator with "framebuffer has no GPU
        // backing" — an error that names the pass rather than the object.
        match device.create_gpu_framebuffer(&FramebufferDesc {
            render_pass,
            attachments: views
                .into_iter()
                .map(|texture_view| FramebufferAttachment {
                    texture_view,
                    layer: 0,
                    mip_level: 0,
                })
                .collect(),
            width,
            height,
            layers: 1,
        }) {
            Ok(framebuffer) => state.framebuffer = Some(framebuffer),
            Err(e) => {
                crate::error!("gbuffer", "the framebuffer could not be created: {e}");
                state.framebuffer = None;
            }
        }
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) -> PassOutcome {
        // Bug №653: the pass says what it did, so the graph can tell "was
        // called" from "contributed to the frame". The reasons are the
        // actionable part: "no framebuffer" and "no pipeline" are two
        // different bugs, and a list of names cannot tell them apart.
        if !self.base.is_enabled() {
            return PassOutcome::skipped("disabled");
        }
        let Some(state) = self.state.as_ref() else {
            return PassOutcome::skipped("the pass has no state");
        };
        let Ok(state) = state.lock() else {
            return PassOutcome::skipped("the state lock is poisoned");
        };
        let Some(framebuffer) = state.framebuffer.as_ref() else {
            return PassOutcome::skipped("no framebuffer");
        };
        if state.items.is_empty() {
            return PassOutcome::skipped("no items");
        }
        if state.uniforms.len() < state.items.len() {
            return PassOutcome::skipped("not every item has a uniform");
        }

        let (width, height) = context.drawable_size();
        let mut commands: Vec<Command> = Vec::with_capacity(16 + state.items.len() * 4);

        commands.push(Command::BeginRenderPass(
            crate::rhi::command::pass::render::RenderPassBeginInfo {
                render_pass: state.render_pass.clone(),
                framebuffer: framebuffer.clone(),
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D { width, height },
                },
                // One clear value per attachment, in attachment order: four
                // colour and one depth. A depth attachment with a clear load
                // operation and no matching clear value is invalid.
                //
                // The colour targets are cleared to **alpha zero**, and that is
                // the alpha channel's meaning here: zero where no geometry
                // landed, one where it did. It also makes the four targets
                // comparable, which matters because the material target's
                // contents are currently all zeroes — the shader has no
                // metallic or occlusion data to put there yet — and a target
                // cleared to (0,0,0,1) would be indistinguishable from one the
                // shader wrote.
                clear_values: vec![
                    crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 0.0),
                    crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 0.0),
                    crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 0.0),
                    crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 0.0),
                    crate::rhi::ClearValue::depth_stencil(1.0, 0),
                ],
            },
        ));
        commands.push(Command::SetViewport(Viewport {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }));
        commands.push(Command::SetScissor(Rect2D {
            offset: Offset2D { x: 0, y: 0 },
            extent: crate::rhi::Extent2D { width, height },
        }));
        commands.push(Command::BindGraphicsPipeline(state.pipeline.clone()));

        for (index, item) in state.items.iter().enumerate() {
            let Some(uniform) = state.uniforms.get(index) else {
                continue;
            };
            commands.push(Command::BindDescriptorSets {
                pipeline: state.pipeline.clone(),
                first_set: 0,
                sets: vec![uniform.descriptor_set.clone()],
            });
            commands.push(Command::BindVertexBuffers {
                first_binding: 0,
                buffers: vec![((*item.vertex_buffer).clone(), 0)],
            });
            commands.push(Command::BindIndexBuffer {
                buffer: (*item.index_buffer).clone(),
                offset: 0,
                index_type: match item.index_type {
                    crate::render::meshes::IndexType::U16 => crate::rhi::IndexType::U16,
                    crate::render::meshes::IndexType::U32 => crate::rhi::IndexType::U32,
                    crate::render::meshes::IndexType::U8 => {
                        crate::warn!("gbuffer", "{}: 8-bit indices are not supported", item.name);
                        continue;
                    }
                },
            });
            commands.push(Command::DrawIndexed {
                index_count: item.index_count,
                instance_count: 1,
                first_index: 0,
                vertex_offset: 0,
                first_instance: 0,
            });
        }

        commands.push(Command::EndRenderPass);

        for command in commands {
            if !encoder.record(command) {
                crate::error!("gbuffer", "a command was rejected: the encoder is not recording");
                return PassOutcome::skipped("the encoder rejected a command");
            }
        }
        PassOutcome::recorded()
    }

    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }
}

impl Default for GBufferPass {
    fn default() -> Self {
        Self::new()
    }
}
