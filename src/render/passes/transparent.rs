//! Transparent Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Draws the scene's transparent objects — water, smoke, glass — with proper
//! blending, back to front, so that a thing behind a thing behind it blends
//! in the right order.
//!
//! What this pass used to be: a `pipelines` map created empty and never
//! filled, and a loop over the objects whose body said "Placeholder: actual
//! rendering would go here". It opened a render pass, drew nothing, and
//! reported that it had recorded work.
//!
//! The target is the image the renderer acquired, the same way the resolve
//! pass's is: in a forward renderer the transparent objects are drawn over the
//! image the opaque pass has already produced, and the depth they are tested
//! against is the opaque pass's depth. A caller that wants the copy
//! somewhere else can pin a framebuffer, which is how the test reads the
//! result back.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::Mat4;

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::passes::base::BaseRenderPass;
use crate::render::passes::depth_target::DepthTarget;
use crate::render::passes::geometry::GeometryItem;
use crate::render::scene::Scene;
use crate::rhi::{
    ColorBlendAttachment, ColorBlendState, ColorComponentFlags, Command, CommandEncoder,
    DescriptorInfo, DescriptorSet, DescriptorSetLayout, DescriptorWrite, Framebuffer,
    FramebufferAttachment, FramebufferDesc, GraphicsPipeline, Offset2D, Rect2D,
};

/// The uniform block, matching `transparent.wgsl`:
/// `mat4 mvp` + `mat4 model` + `mat4 normal_matrix` = 192 bytes.
const UNIFORM_SIZE: u64 = 16 * 4 * 3;

/// How the transparent objects are mixed with what is behind them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    /// Standard alpha blending: `src * alpha + dst * (1 - alpha)`.
    #[default]
    Alpha,
    /// Additive: `src + dst`.
    Additive,
    /// Multiplicative: `src * dst`.
    Multiplicative,
    /// Screen: `src + dst - src * dst`.
    Screen,
}

impl BlendMode {
    /// The blend state for this mode.
    ///
    /// The factors are the whole mode: the shader writes the colour and the
    /// alpha, and what the hardware does with them is the mode. Alpha
    /// blending is the one the RHI already knows how to build; the other
    /// three are spelled out here so that the four pipelines differ in
    /// exactly one place.
    fn blend_state(self) -> ColorBlendState {
        use crate::rhi::pipeline::graphics::{BlendFactor, BlendOp};
        // The colour factors and the alpha factors are separate, and they have
        // to be: the standard alpha blend is `src * a + dst * (1 - a)` for the
        // colour and `src.a + dst.a * (1 - a)` for the alpha. Reusing the
        // colour's factors for the alpha squares the source's alpha — 0.5
        // becomes 0.25 — and the result is a transparent thing that is twice
        // as transparent as it was asked to be.
        let (src, dst, alpha_src, alpha_dst) = match self {
            BlendMode::Alpha => (
                BlendFactor::SrcAlpha,
                BlendFactor::OneMinusSrcAlpha,
                BlendFactor::One,
                BlendFactor::OneMinusSrcAlpha,
            ),
            BlendMode::Additive => (
                BlendFactor::One,
                BlendFactor::One,
                BlendFactor::One,
                BlendFactor::One,
            ),
            BlendMode::Multiplicative => (
                BlendFactor::DstColor,
                BlendFactor::Zero,
                BlendFactor::DstAlpha,
                BlendFactor::Zero,
            ),
            BlendMode::Screen => (
                BlendFactor::OneMinusDstColor,
                BlendFactor::One,
                BlendFactor::One,
                BlendFactor::OneMinusSrcAlpha,
            ),
        };
        ColorBlendState {
            attachments: vec![ColorBlendAttachment {
                blend_enable: true,
                src_color_blend_factor: src,
                dst_color_blend_factor: dst,
                color_blend_op: BlendOp::Add,
                src_alpha_blend_factor: alpha_src,
                dst_alpha_blend_factor: alpha_dst,
                alpha_blend_op: BlendOp::Add,
                color_write_mask: ColorComponentFlags::R
                    | ColorComponentFlags::G
                    | ColorComponentFlags::B
                    | ColorComponentFlags::A,
            }],
            ..Default::default()
        }
    }
}

/// How the objects are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    /// Back to front: the usual order for alpha blending.
    #[default]
    BackToFront,
    /// Front to back.
    FrontToBack,
    /// No sorting.
    None,
}

/// Transparent pass configuration.
#[derive(Debug, Clone)]
pub struct TransparentConfig {
    pub sort_mode: SortMode,
    pub max_transparent_objects: usize,
}

impl Default for TransparentConfig {
    fn default() -> Self {
        Self {
            sort_mode: SortMode::BackToFront,
            max_transparent_objects: 1000,
        }
    }
}

/// One transparent object: a mesh and how it is mixed.
#[derive(Clone)]
pub struct TransparentItem {
    pub item: GeometryItem,
    pub blend_mode: BlendMode,
}

/// Everything the pass needs to record, shared with the caller.
pub struct TransparentPassState {
    pub pipelines: HashMap<BlendMode, GraphicsPipeline>,
    /// The pipelines for the shared-depth mode.
    ///
    /// A second set, because a pipeline belongs to a render pass and the two
    /// modes are two render passes: the standalone one clears the colour and
    /// the depth and writes the depth, and the shared one loads both and
    /// writes neither.
    pub shared_pipelines: HashMap<BlendMode, GraphicsPipeline>,
    pub descriptor_layout: DescriptorSetLayout,
    /// One descriptor set per item, in the same order as `items`.
    pub descriptor_sets: Vec<DescriptorSet>,
    /// One uniform buffer per item, in the same order as `items`.
    pub uniform_buffers: Vec<crate::rhi::Buffer>,
    pub items: Vec<TransparentItem>,
    pub render_pass: crate::rhi::RenderPass,
    /// The render pass for the shared-depth mode.
    pub shared_render_pass: Option<crate::rhi::RenderPass>,
    /// The depth the objects are tested against.
    ///
    /// The pass's own, because the image the renderer acquired has no depth
    /// and a framebuffer's attachment count must match its render pass's
    /// exactly.
    pub depth_texture: Option<crate::rhi::Texture>,
    pub depth_view: Option<crate::rhi::TextureView>,
    /// The frame's shared depth, when the caller gave one.
    ///
    /// The transparent objects are then tested against the **opaque**
    /// geometry's depth, which is the whole point: a transparent object behind
    /// a wall must not be drawn over the wall.
    pub depth_target: Option<DepthTarget>,
    /// The framebuffer the objects are drawn into.
    pub framebuffer: Option<Framebuffer>,
    /// Whether the framebuffer was chosen by the caller.
    pub framebuffer_pinned: bool,
    /// Which swapchain image the framebuffer was built for.
    pub framebuffer_image: Option<u32>,
    /// The extent it was built for.
    pub framebuffer_extent: Option<(u32, u32)>,
    /// Which swapchain generation it was built for.
    pub framebuffer_generation: Option<u64>,
    /// The generation of the shared depth the framebuffer was built with.
    pub framebuffer_depth_generation: Option<u64>,
    pub config: TransparentConfig,
}

/// Shared handle to the pass state.
pub type SharedState = Arc<Mutex<TransparentPassState>>;

/// Transparent pass.
pub struct TransparentPass {
    base: BaseRenderPass,
    config: TransparentConfig,
    state: Option<SharedState>,
    /// The particle system, drawn with the same blend as the objects.
    pub particle_system: Option<Arc<crate::render::particles::ParticleSystem>>,
}

impl TransparentPass {
    /// Build a pass with no GPU objects yet.
    pub fn new(config: TransparentConfig) -> Self {
        let mut base = BaseRenderPass::new("transparent");
        // The transparent objects are drawn over the image the opaque pass
        // produced, so the pass comes after it. The depth they are tested
        // against is the opaque pass's depth.
        //
        // The dependency names the pass that writes the depth, because the
        // graph orders by that name. It used to name "opaque", and no pass
        // has ever been called that: the dependency matched nothing, so the
        // transparent pass was never ordered after the geometry — and with the
        // shared depth it has to be, or the pass reads a depth nobody wrote.
        base.add_dependency(
            "forward",
            "shared_depth",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
        base.add_output("swapchain");
        Self {
            base,
            config,
            state: None,
            particle_system: None,
        }
    }

    /// Build a pass around a new shared state and return both.
    pub fn with_state(state: TransparentPassState) -> (Self, SharedState) {
        let mut base = BaseRenderPass::new("transparent");
        base.add_dependency(
            "forward",
            "shared_depth",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
        base.add_output("swapchain");
        let shared: SharedState = Arc::new(Mutex::new(state));
        let config = shared
            .lock()
            .map(|state| state.config.clone())
            .unwrap_or_default();
        let pass = Self {
            base,
            config,
            state: Some(shared.clone()),
            particle_system: None,
        };
        (pass, shared)
    }

    /// Build a transparent pass with real GPU objects.
    ///
    /// `items` are the scene's transparent meshes. `color_format` is the
    /// format of the image the objects are drawn into — the surface's, for a
    /// windowed run — and a pipeline that declares a different colour format
    /// than its framebuffer is undefined behaviour rather than a warning.
    pub fn create(
        device: &crate::rhi::Device,
        items: Vec<TransparentItem>,
        config: TransparentConfig,
        color_format: crate::rhi::Format,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::PrimitiveTopology;
        use crate::rhi::{
            DepthStencilState, GraphicsPipelineDesc, InputAssemblyState, MultisampleState,
            PipelineShaderStage, RasterizerState, ShaderFormat, ShaderModuleDesc,
        };

        // A render pass with a colour attachment and a depth attachment: the
        // objects are tested against the depth the opaque pass left behind,
        // and a transparent object behind an opaque one must not draw over
        // it.
        let render_pass = device
            .create_gpu_render_pass(&crate::rhi::RenderPassDesc {
                attachments: vec![
                    crate::rhi::AttachmentDescription {
                        format: color_format,
                        samples: crate::rhi::SampleCount::X1,
                        load_op: crate::rhi::LoadOp::Clear,
                        store_op: crate::rhi::StoreOp::Store,
                        stencil_load_op: crate::rhi::LoadOp::DontCare,
                        stencil_store_op: crate::rhi::StoreOp::DontCare,
                        initial_layout: crate::rhi::TextureLayout::Undefined,
                        final_layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                    },
                    crate::rhi::AttachmentDescription {
                        format: crate::rhi::Format::D32_SFLOAT,
                        samples: crate::rhi::SampleCount::X1,
                        load_op: crate::rhi::LoadOp::Clear,
                        store_op: crate::rhi::StoreOp::Store,
                        stencil_load_op: crate::rhi::LoadOp::DontCare,
                        stencil_store_op: crate::rhi::StoreOp::DontCare,
                        initial_layout: crate::rhi::TextureLayout::Undefined,
                        final_layout: crate::rhi::TextureLayout::DepthStencilAttachmentOptimal,
                    },
                ],
                subpasses: vec![crate::rhi::SubpassDescription {
                    pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
                    color_attachments: vec![crate::rhi::AttachmentReference {
                        attachment: 0,
                        layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                    }],
                    depth_stencil_attachment: Some(crate::rhi::AttachmentReference {
                        attachment: 1,
                        layout: crate::rhi::TextureLayout::DepthStencilAttachmentOptimal,
                    }),
                    ..Default::default()
                }],
                dependencies: vec![],
            })
            .map_err(|e| RhiError::BackendError(format!("the transparent render pass: {e}")))?;

        // The render pass for the shared-depth mode.
        //
        // Two differences from the one above, and both are the point:
        //
        // - the colour is **loaded**, not cleared: the opaque picture is
        //   already in it, and clearing it would throw the picture away and
        //   draw the transparent objects over black;
        // - the depth is **read-only**: the transparent objects are tested
        //   against the opaque geometry's depth, and they do not write it — a
        //   transparent object that wrote depth would hide the transparent
        //   objects behind it, and the depth would no longer be the opaque
        //   geometry's.
        let shared_render_pass = device
            .create_gpu_render_pass(&crate::rhi::RenderPassDesc {
                attachments: vec![
                    crate::rhi::AttachmentDescription {
                        format: color_format,
                        samples: crate::rhi::SampleCount::X1,
                        load_op: crate::rhi::LoadOp::Load,
                        store_op: crate::rhi::StoreOp::Store,
                        stencil_load_op: crate::rhi::LoadOp::DontCare,
                        stencil_store_op: crate::rhi::StoreOp::DontCare,
                        initial_layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                        final_layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                    },
                    crate::rhi::AttachmentDescription {
                        format: crate::rhi::Format::D32_SFLOAT,
                        samples: crate::rhi::SampleCount::X1,
                        load_op: crate::rhi::LoadOp::Load,
                        store_op: crate::rhi::StoreOp::Store,
                        stencil_load_op: crate::rhi::LoadOp::DontCare,
                        stencil_store_op: crate::rhi::StoreOp::DontCare,
                        initial_layout: crate::rhi::TextureLayout::DepthStencilAttachmentOptimal,
                        // The attachment stays writable, and the *pipeline* is
                        // the one that does not write: the RHI's attachment
                        // reference has no "read only" flag, and a subpass that
                        // names a read-only layout without the flag is a
                        // validation error.
                        final_layout: crate::rhi::TextureLayout::DepthStencilAttachmentOptimal,
                    },
                ],
                subpasses: vec![crate::rhi::SubpassDescription {
                    pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
                    color_attachments: vec![crate::rhi::AttachmentReference {
                        attachment: 0,
                        layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                    }],
                    depth_stencil_attachment: Some(crate::rhi::AttachmentReference {
                        attachment: 1,
                        layout: crate::rhi::TextureLayout::DepthStencilAttachmentOptimal,
                    }),
                    ..Default::default()
                }],
                dependencies: vec![],
            })
            .map_err(|e| {
                RhiError::BackendError(format!("the transparent shared-depth render pass: {e}"))
            })?;

        // One uniform buffer and one descriptor set per object: the uniform
        // holds the object's model matrix, and one buffer for the whole pass
        // would make every object use the last one's matrix (§654).
        let descriptor_layout = device
            .create_descriptor_set_layout(
                &crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
                    bindings: vec![crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 0,
                        ty: crate::rhi::DescriptorType::UniformBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::VERTEX | crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    }],
                },
            )
            .map_err(|e| {
                RhiError::BackendError(format!("the transparent descriptor layout: {e}"))
            })?;
        let mut uniform_buffers = Vec::with_capacity(items.len());
        let mut descriptor_sets = Vec::with_capacity(items.len());
        for _ in 0..items.len() {
            let uniform = device
                .create_buffer(UNIFORM_SIZE, crate::rhi::BufferUsage::UNIFORM, true)
                .map_err(|e| {
                    RhiError::BackendError(format!("the transparent uniform buffer: {e}"))
                })?;
            let set = device.create_descriptor_set(&descriptor_layout)?;
            device.write_descriptors(
                &set,
                &[DescriptorWrite {
                    dst_set: set.clone(),
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(uniform.clone(), 0, UNIFORM_SIZE)],
                }],
            )?;
            uniform_buffers.push(uniform);
            descriptor_sets.push(set);
        }

        let module = device
            .create_shader_module(&ShaderModuleDesc {
                code: spirv.to_vec(),
                format: ShaderFormat::SpirV,
                entry_point: Some("vs_main".into()),
                name: Some("transparent".into()),
            })
            .map_err(|e| RhiError::BackendError(format!("the transparent shader module: {e}")))?;

        // One pipeline per blend mode: the mode *is* the blend state, and a
        // pipeline is immutable, so four modes mean four pipelines.
        let mut pipelines = HashMap::new();
        // And four more for the shared-depth mode, because a pipeline belongs
        // to a render pass, and the shared-depth mode is a different render
        // pass with a different depth state: the depth is tested and not
        // written.
        let mut shared_pipelines = HashMap::new();
        for mode in [
            BlendMode::Alpha,
            BlendMode::Additive,
            BlendMode::Multiplicative,
            BlendMode::Screen,
        ] {
            let mut desc = GraphicsPipelineDesc {
                shader_stages: vec![
                    PipelineShaderStage {
                        stage: crate::rhi::ShaderStage::VERTEX,
                        module: module.clone(),
                        entry_point: "vs_main".into(),
                    },
                    PipelineShaderStage {
                        stage: crate::rhi::ShaderStage::FRAGMENT,
                        module: module.clone(),
                        entry_point: "fs_main".into(),
                    },
                ],
                vertex_input_state: Some(crate::render::passes::geometry::vertex_input_state()),
                input_assembly_state: InputAssemblyState {
                    topology: PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                rasterizer_state: Some(RasterizerState::double_sided()),
                multisample_state: MultisampleState {
                    sample_count: crate::rhi::SampleCount::X1,
                    ..Default::default()
                },
                color_blend_state: Some(mode.blend_state()),
                ..Default::default()
            };
            desc.depth_stencil_state = Some(DepthStencilState::less_equal());
            let pipeline = device
                .create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])
                .map_err(|e| RhiError::BackendError(format!("the transparent pipeline: {e}")))?;
            if !pipeline.has_gpu_backing() {
                return Err(RhiError::BackendError(
                    "the transparent graphics pipeline was not created on the device".into(),
                ));
            }
            pipelines.insert(mode, pipeline);

            // The shared-depth pipeline: the depth is tested against the
            // opaque geometry's and not written.
            desc.depth_stencil_state = Some(DepthStencilState::read_only());
            let pipeline = device
                .create_gpu_graphics_pipeline(
                    &desc,
                    &shared_render_pass,
                    &[&descriptor_layout],
                )
                .map_err(|e| {
                    RhiError::BackendError(format!("the transparent shared-depth pipeline: {e}"))
                })?;
            if !pipeline.has_gpu_backing() {
                return Err(RhiError::BackendError(
                    "the transparent shared-depth graphics pipeline was not created on the \
                     device"
                        .into(),
                ));
            }
            shared_pipelines.insert(mode, pipeline);
        }

        Ok(Self::with_state(TransparentPassState {
            pipelines,
            shared_pipelines,
            descriptor_layout,
            descriptor_sets,
            uniform_buffers,
            items,
            render_pass,
            shared_render_pass: Some(shared_render_pass),
            depth_texture: None,
            depth_view: None,
            depth_target: None,
            framebuffer: None,
            framebuffer_pinned: false,
            framebuffer_image: None,
            framebuffer_extent: None,
            framebuffer_generation: None,
            framebuffer_depth_generation: None,
            config,
        }))
    }

    /// The frame's shared depth, the one the opaque geometry wrote.
    ///
    /// With it the transparent objects are tested against the **opaque**
    /// geometry's depth: a transparent object behind a wall is not drawn over
    /// the wall, and a transparent object in front of it is.
    ///
    /// Without it the pass keeps its own depth, and the transparent objects
    /// are tested against nothing but each other.
    pub fn set_depth_target(state: &SharedState, depth: DepthTarget) {
        if let Ok(mut state) = state.lock() {
            state.depth_target = Some(depth);
        }
    }

    /// Pin the framebuffer the objects are drawn into.
    ///
    /// The same escape hatch the geometry and resolve passes have: a test
    /// that reads the result back needs an offscreen target, because a
    /// presentation image ends in `PresentSrc` and cannot be copied out.
    pub fn set_framebuffer(
        state: &SharedState,
        framebuffer: Option<Framebuffer>,
        _width: u32,
        _height: u32,
    ) {
        if let Ok(mut state) = state.lock() {
            let pinned = framebuffer.is_some();
            state.framebuffer = framebuffer;
            state.framebuffer_pinned = pinned;
        }
    }

    /// The camera matrices for the frame.
    pub fn update_uniforms(
        state: &SharedState,
        device: &crate::rhi::Device,
        view: Mat4,
        projection: Mat4,
    ) -> bool {
        let Ok(state) = state.lock() else {
            return false;
        };
        let mvp = projection * view;
        let mut ok = true;
        for (index, entry) in state.items.iter().enumerate() {
            let Some(buffer) = state.uniform_buffers.get(index) else {
                continue;
            };
            let mut data = [0f32; 48];
            data[..16].copy_from_slice(&mvp.to_cols_array());
            data[16..32].copy_from_slice(&entry.item.model.to_cols_array());
            // The normal matrix: the inverse transpose of the model, so that
            // a non-uniform scale does not tilt the normals. The cube is
            // uniform, but the water is not.
            let normal = entry
                .item
                .model
                .inverse()
                .transpose()
                .to_cols_array();
            data[32..].copy_from_slice(&normal);
            if device.upload_buffer(buffer, &data).is_err() {
                ok = false;
            }
        }
        ok
    }

    /// The configuration the pass was built with.
    ///
    /// Read-only on purpose: the live configuration is the one in the state,
    /// and a setter here would change a copy that nothing reads.
    pub fn config(&self) -> &TransparentConfig {
        &self.config
    }

    /// The scene's transparent objects, as drawable items.    ///
    /// The bridge between the scene graph and the GPU, the same one the
    /// geometry pass has, narrowed to the objects that are not opaque: a light
    /// and a camera are not drawn, and neither is an opaque box.
    pub fn items_for_scene(scene: &Scene) -> Vec<TransparentItem> {
        let mut items = Vec::new();
        for entity in scene.entities() {
            if !entity.is_transparent() || !entity.is_visible() {
                continue;
            }
            let Some(mesh_component) = entity.mesh() else {
                continue;
            };
            let Some(transform) = entity.transform() else {
                continue;
            };
            let blend_mode = match entity.blend_mode() {
                crate::render::scene::components::BlendMode::Opaque => BlendMode::Alpha,
                crate::render::scene::components::BlendMode::Alpha => BlendMode::Alpha,
                crate::render::scene::components::BlendMode::Additive => BlendMode::Additive,
                crate::render::scene::components::BlendMode::Multiplicative => {
                    BlendMode::Multiplicative
                }
                crate::render::scene::components::BlendMode::Screen => BlendMode::Screen,
            };
            let mesh = &mesh_component.mesh;
            let (Some(vertex_buffer), Some(index_buffer)) =
                (mesh.vertex_buffer(), mesh.index_buffer())
            else {
                continue;
            };
            if mesh.index_count() == 0 {
                continue;
            }
            items.push(TransparentItem {
                item: GeometryItem {
                    name: entity.name().to_string(),
                    vertex_buffer: vertex_buffer.clone(),
                    index_buffer: index_buffer.clone(),
                    index_type: mesh.index_type(),
                    index_count: mesh.index_count(),
                    model: transform.matrix(),
                    entity_id: Some(entity.id()),
                },
                blend_mode,
            });
        }
        items
    }
}

/// The framebuffer the objects are drawn into this frame.
///
/// A pinned framebuffer wins — the caller said exactly where to draw. Otherwise
/// the pass draws into the image the renderer acquired, and the framebuffer is
/// cached per image, rebuilt when the image, the extent or the chain's
/// generation changes. The generation is the part that matters for liveness:
/// a chain rebuilt at the same size has the same extent and new views, and
/// the old ones are gone with the old chain.
fn resolve_target(
    state: &mut TransparentPassState,
    context: &RenderContext,
    device: &crate::rhi::Device,
) -> Option<Framebuffer> {
    if state.framebuffer_pinned {
        return state.framebuffer.clone();
    }
    let index = context.swapchain_image_index;
    let Some(view) = context.presentation_texture_views.get(index as usize) else {
        return None;
    };
    let (width, height) = context.drawable_size();
    if width == 0 || height == 0 {
        return None;
    }
    if state.framebuffer_generation != Some(context.swapchain_generation) {
        state.framebuffer = None;
        state.framebuffer_generation = Some(context.swapchain_generation);
    }
    if state.framebuffer_extent != Some((width, height)) {
        state.framebuffer = None;
        state.framebuffer_extent = Some((width, height));
    }
    // The depth attachment: the frame's shared depth when the caller gave one,
    // and the pass's own otherwise.
    //
    // The shared depth is the whole point: the transparent objects are tested
    // against the **opaque** geometry's depth. A depth of their own is a depth
    // of nothing — the pass would clear it to the far plane, and every
    // transparent object would be in front of every other, including the ones
    // behind a wall.
    let shared = state.depth_target.is_some();
    let depth_view = if let Some(target) = state.depth_target.clone() {
        let Some(view) = target.view(device, width, height) else {
            return None;
        };
        view
    } else {
        // The pass's own depth, rebuilt when the size changes: a framebuffer's
        // attachments must all be the size of the render area. The old
        // framebuffer is dropped first, because it holds the old depth's view.
        let depth_stale = state
            .depth_texture
            .as_ref()
            .is_none_or(|texture| texture.width() != width || texture.height() != height);
        if depth_stale {
            let depth = match device.create_texture(
                width,
                height,
                1,
                crate::rhi::Format::D32_SFLOAT,
                crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT,
                1,
            ) {
                Ok(texture) => texture,
                Err(e) => {
                    crate::error!("transparent", "the depth texture could not be created: {e}");
                    return None;
                }
            };
            let view = match device.create_texture_view(
                &depth,
                &crate::rhi::TextureViewDesc {
                    texture: depth.clone(),
                    format: None,
                    view_type: crate::rhi::TextureViewType::D2,
                    aspects: crate::rhi::TextureAspectFlags::DEPTH,
                    base_mip_level: 0,
                    mip_level_count: 1,
                    base_array_layer: 0,
                    array_layer_count: 1,
                },
            ) {
                Ok(view) => view,
                Err(e) => {
                    crate::error!("transparent", "the depth view could not be created: {e}");
                    return None;
                }
            };
            state.depth_texture = Some(depth);
            state.depth_view = Some(view);
            state.framebuffer = None;
        }
        let Some(view) = state.depth_view.clone() else {
            return None;
        };
        view
    };
    // The framebuffer is rebuilt when the depth was: the old one holds the old
    // depth's view.
    let depth_generation = state
        .depth_target
        .as_ref()
        .map(|target| target.generation())
        .unwrap_or(0);
    if state.framebuffer_depth_generation != Some(depth_generation) {
        state.framebuffer = None;
        state.framebuffer_depth_generation = Some(depth_generation);
    }
    if state.framebuffer_image == Some(index) {
        if let Some(framebuffer) = state.framebuffer.clone() {
            return Some(framebuffer);
        }
    }
    let render_pass = if shared {
        state.shared_render_pass.clone()
    } else {
        Some(state.render_pass.clone())
    };
    let Some(render_pass) = render_pass else {
        return None;
    };
    let framebuffer = match device.create_gpu_framebuffer(&FramebufferDesc {
        render_pass,
        attachments: vec![
            FramebufferAttachment {
                texture_view: view.clone(),
                layer: 0,
                mip_level: 0,
            },
            FramebufferAttachment {
                texture_view: depth_view,
                layer: 0,
                mip_level: 0,
            },
        ],
        width,
        height,
        layers: 1,
    }) {
        Ok(framebuffer) => framebuffer,
        Err(e) => {
            crate::error!("transparent", "the framebuffer could not be created: {e}");
            return None;
        }
    };
    state.framebuffer_image = Some(index);
    state.framebuffer = Some(framebuffer.clone());
    Some(framebuffer)
}

impl RenderPass for TransparentPass {
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
        _device: &crate::rhi::Device,
        _context: &RenderContext,
        _resources: &mut HashMap<String, GraphResource>,
    ) {
        // Everything the pass needs was built with the device in `create`:
        // the pipelines, the per-object uniforms and the render pass. The
        // framebuffer is built per frame, because it depends on the image the
        // renderer acquired.
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) -> PassOutcome {
        if !self.base.is_enabled() {
            return PassOutcome::skipped("disabled");
        }
        let Some(state) = self.state.as_ref() else {
            return PassOutcome::skipped("the pass has no state");
        };
        let Ok(mut state) = state.lock() else {
            return PassOutcome::skipped("the state lock is poisoned");
        };
        if state.items.is_empty() {
            return PassOutcome::skipped("no transparent objects");
        }
        let device = context.device().clone();
        let Some(framebuffer) = resolve_target(&mut state, context, device.as_ref()) else {
            return PassOutcome::skipped("no target for the acquired image");
        };

        // Back to front: the distance is what decides it, and it changes with
        // the camera, so the order is computed per frame and not once.
        let mut order: Vec<usize> = (0..state.items.len()).collect();
        if state.config.sort_mode != SortMode::None {
            let distances: Vec<f32> = state
                .items
                .iter()
                .map(|entry| {
                    let position = entry.item.model.w_axis.truncate();
                    let camera = context.camera_position();
                    (position - camera).length()
                })
                .collect();
            order.sort_by(|a, b| {
                let (a, b) = (distances[*a], distances[*b]);
                match state.config.sort_mode {
                    SortMode::BackToFront => b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal),
                    SortMode::FrontToBack => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
                    SortMode::None => std::cmp::Ordering::Equal,
                }
            });
        }
        order.truncate(state.config.max_transparent_objects);

        // The size of the *target*, not of the window.
        //
        // The render area has to fit inside the framebuffer, and the
        // viewport has to be the size of what is being drawn into. The
        // framebuffer's own descriptor is the size of its attachments, which
        // is the size of the image — and a headless context has no window at
        // all, so its resolution is a default that has nothing to do with the
        // target.
        let desc = framebuffer.desc();
        let (width, height) = (desc.width, desc.height);
        if width == 0 || height == 0 {
            return PassOutcome::skipped("the target has no size");
        }
        let mut commands: Vec<Command> = Vec::with_capacity(8 + order.len() * 4);

        // The shared-depth mode: the opaque picture is in the target and the
        // opaque depth is the depth, so the target is **loaded** and the depth
        // is **read-only**.
        //
        // Clearing either of them here would throw away exactly what the
        // shared depth is for: a cleared colour throws the opaque picture away,
        // and a cleared depth makes every transparent object in front of
        // every other.
        let shared = state.depth_target.is_some();
        let render_pass = if shared {
            state.shared_render_pass.clone()
        } else {
            Some(state.render_pass.clone())
        };
        let Some(render_pass) = render_pass else {
            return PassOutcome::skipped("the shared-depth render pass is missing");
        };
        let clear_values = if shared {
            // The load operations ignore the clear values, and a render pass
            // with a load operation has nothing to clear.
            Vec::new()
        } else {
            vec![
                crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 0.0),
                crate::rhi::ClearValue::depth_stencil(1.0, 0),
            ]
        };
        if shared {
            // The barrier between the opaque pass and this one.
            //
            // The order of the passes is not a memory dependency: the forward
            // pass wrote the depth and the colour, and without this the
            // transparent pass would read them without being guaranteed to see
            // them. A render pass's own dependencies do not cover the gap
            // between two render passes — the second pass's "external" subpass
            // dependencies would, and this render pass declares none.
            //
            // So the barrier is **before** the render pass starts: inside a
            // render pass instance a barrier needs a self-dependency, and the
            // render pass has none.
            commands.push(Command::PipelineBarrier {
                src_stage: crate::rhi::PipelineStage::COLOR_ATTACHMENT_OUTPUT
                    | crate::rhi::PipelineStage::LATE_FRAGMENT_TESTS,
                dst_stage: crate::rhi::PipelineStage::COLOR_ATTACHMENT_OUTPUT
                    | crate::rhi::PipelineStage::FRAGMENT_SHADER
                    | crate::rhi::PipelineStage::EARLY_FRAGMENT_TESTS,
                memory_barriers: vec![crate::rhi::sync::barrier::MemoryBarrier {
                    src_access: crate::rhi::AccessFlags::COLOR_ATTACHMENT_WRITE
                        | crate::rhi::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                    dst_access: crate::rhi::AccessFlags::COLOR_ATTACHMENT_READ
                        | crate::rhi::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ,
                }],
                buffer_barriers: vec![],
                texture_barriers: vec![],
            });
        }
        commands.push(Command::BeginRenderPass(
            crate::rhi::command::pass::render::RenderPassBeginInfo {
                render_pass,
                framebuffer: framebuffer.clone(),
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D { width, height },
                },
                clear_values,
            },
        ));
        commands.push(Command::SetViewport(crate::rhi::Viewport {
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

        for index in order {
            let Some(entry) = state.items.get(index) else {
                continue;
            };
            let pipeline = if shared {
                state.shared_pipelines.get(&entry.blend_mode)
            } else {
                state.pipelines.get(&entry.blend_mode)
            };
            let Some(pipeline) = pipeline else {
                continue;
            };
            let Some(descriptor_set) = state.descriptor_sets.get(index) else {
                continue;
            };
            commands.push(Command::BindGraphicsPipeline(pipeline.clone()));
            commands.push(Command::BindDescriptorSets {
                pipeline: pipeline.clone(),
                first_set: 0,
                sets: vec![descriptor_set.clone()],
            });
            commands.push(Command::BindVertexBuffers {
                first_binding: 0,
                buffers: vec![((*entry.item.vertex_buffer).clone(), 0)],
            });
            commands.push(Command::BindIndexBuffer {
                buffer: (*entry.item.index_buffer).clone(),
                offset: 0,
                index_type: match entry.item.index_type {
                    crate::render::meshes::IndexType::U16 => crate::rhi::IndexType::U16,
                    crate::render::meshes::IndexType::U32 => crate::rhi::IndexType::U32,
                    crate::render::meshes::IndexType::U8 => {
                        crate::warn!(
                            "transparent",
                            "{}: 8-bit indices are not supported",
                            entry.item.name
                        );
                        continue;
                    }
                },
            });
            commands.push(Command::DrawIndexed {
                index_count: entry.item.index_count,
                instance_count: 1,
                first_index: 0,
                vertex_offset: 0,
                first_instance: 0,
            });
        }

        commands.push(Command::EndRenderPass);

        for command in commands {
            if !encoder.record(command) {
                crate::error!(
                    "transparent",
                    "a command was rejected: the encoder is not recording"
                );
                return PassOutcome::skipped("the encoder rejected a command");
            }
        }
        let _ = scene;
        PassOutcome::recorded()
    }

    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }
}
