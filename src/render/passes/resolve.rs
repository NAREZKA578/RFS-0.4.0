//! Resolve Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The last link of the deferred chain: it copies a texture into the image the
//! renderer acquired, so the frame that the compositor shows is the frame the
//! chain produced.
//!
//! Without it the chain worked and nobody saw it. The GBuffer filled four
//! targets, the lighting pass resolved them into a lit image, and the lit image
//! sat in a texture that no pass read and no screen showed — a complete
//! deferred pipeline with no way to look at the result.
//!
//! It is a fullscreen copy: one oversized triangle, one sampled texture, one
//! colour attachment.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::graph::types::ResourceUsage;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::sync::barrier::TextureBarrier;
use crate::rhi::{
    AccessFlags, Command, CommandEncoder, DescriptorInfo, DescriptorSet, DescriptorSetLayout,
    DescriptorType, DescriptorWrite, Framebuffer, FramebufferAttachment, FramebufferDesc,
    GraphicsPipeline, Offset2D, Rect2D, ShaderStage, Texture, TextureLayout, Viewport,
};

/// Everything the pass needs to record, shared with the caller.
pub struct ResolvePassState {
    pub pipeline: GraphicsPipeline,
    pub descriptor_layout: DescriptorSetLayout,
    pub descriptor_set: Option<DescriptorSet>,
    pub render_pass: crate::rhi::RenderPass,
    /// The framebuffers, one per acquired image.
    ///
    /// A swapchain has a few images and the renderer cycles through them, so a
    /// framebuffer per image is built once and reused. The extent is part of
    /// the key: after a resize the attachments have to be rebuilt, and a
    /// framebuffer for the old size would render into a frame that no longer
    /// exists.
    pub framebuffers: HashMap<u32, Framebuffer>,
    pub framebuffer_extent: Option<(u32, u32)>,
    /// The swapchain generation the cached framebuffers were built for.
    ///
    /// The extent is not enough to tell one chain from another: a chain can be
    /// rebuilt at the *same* size (a rebuild that changes nothing but the
    /// images), and then the extent matches while the images — and the views
    /// the framebuffers are built on — are new ones. The old views are
    /// destroyed with the old chain, so a framebuffer that still refers to
    /// them is a framebuffer that refers to freed memory: the validator
    /// reports an invalid `VkImageView` at the next `vkCmdBeginRenderPass`,
    /// and the driver may crash the process.
    ///
    /// The generation is the counter the renderer bumps on every rebuild, and
    /// it is the same counter the geometry pass keys its cache on.
    pub framebuffer_generation: Option<u64>,
    /// The pinned framebuffer, for a caller that wants the copy somewhere
    /// specific.
    ///
    /// The same escape hatch the geometry pass has: a test that reads the
    /// result back needs an offscreen target, because a presentation image
    /// ends in `PresentSrc` and cannot be copied out.
    pub framebuffer_pinned: Option<Framebuffer>,
    /// The image the copy reads, so the barrier can name it.
    pub source_texture: Option<Texture>,
}

/// The handle the caller keeps.
pub type SharedState = Arc<Mutex<ResolvePassState>>;

/// Copies a texture into the acquired image.
pub struct ResolvePass {
    base: BaseRenderPass,
    /// The resource this pass copies. Named, because the graph's resources are
    /// looked up by name and a pass that names its input is what makes the
    /// chain's edges visible.
    source: String,
    state: Option<SharedState>,
}

impl ResolvePass {
    /// The pass's base.
    ///
    /// The dependency names two things and both matter: the **resource** the
    /// pass copies, and the **pass** that produces it. The graph orders the
    /// passes by the second and binds the first, so a resolve pass that named
    /// the wrong producer would be ordered wrongly and copy a texture that
    /// nothing has written yet.
    fn base(source: &str, producer: &str) -> BaseRenderPass {
        let mut base = BaseRenderPass::new("resolve");
        base.add_dependency(producer, source, ResourceUsage::Sampled);
        base.add_output("swapchain");
        base
    }

    pub fn new(source: impl Into<String>, producer: impl Into<String>) -> Self {
        let source = source.into();
        let producer = producer.into();
        Self {
            base: Self::base(&source, &producer),
            source,
            state: None,
        }
    }

    /// Build a pass around a new shared state and return both.
    pub fn with_state(
        state: ResolvePassState,
        source: impl Into<String>,
        producer: impl Into<String>,
    ) -> (Self, SharedState) {
        let source = source.into();
        let producer = producer.into();
        let shared: SharedState = Arc::new(Mutex::new(state));
        let pass = Self {
            base: Self::base(&source, &producer),
            source,
            state: Some(shared.clone()),
        };
        (pass, shared)
    }

    /// Build a resolve pass.
    ///
    /// `color_format` is the format of the image the copy lands in — the
    /// surface's format, which the caller learns from the renderer after the
    /// presentation is attached. A pipeline declares its colour target
    /// format, and using a different one than the framebuffer's is undefined
    /// behaviour rather than a warning.
    pub fn create(
        device: &crate::rhi::Device,
        color_format: crate::rhi::Format,
        spirv: &[u8],
        source: impl Into<String>,
        producer: impl Into<String>,
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::PrimitiveTopology;
        use crate::rhi::{
            ColorBlendAttachment, ColorBlendState, ColorComponentFlags, ColorTargetDesc,
            GraphicsPipelineDesc, InputAssemblyState, MultisampleState, PipelineShaderStage,
            RasterizerState, ShaderFormat, ShaderModuleDesc,
        };

        let render_pass = device.presentation_render_pass(color_format)?;

        let descriptor_layout =
            device.create_descriptor_set_layout(&crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
                bindings: vec![crate::rhi::descriptor::layout::DescriptorBinding {
                    binding: 0,
                    ty: DescriptorType::SampledTexture,
                    count: 1,
                    stages: ShaderStage::FRAGMENT,
                    immutable_samplers: None,
                }],
            });

        let module = device.create_shader_module(&ShaderModuleDesc {
            code: spirv.to_vec(),
            format: ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("resolve".into()),
        });
        if !module.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the resolve shader module was not created on the device".into(),
            ));
        }

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
            // No vertex buffer: the triangle comes from the vertex index.
            vertex_input_state: None,
            input_assembly_state: InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            rasterizer_state: Some(RasterizerState::double_sided()),
            multisample_state: MultisampleState {
                sample_count: crate::rhi::SampleCount::X1,
                ..Default::default()
            },
            color_blend_state: Some(ColorBlendState {
                // The write mask has to be set explicitly: `ColorComponentFlags`
                // derives `Default`, and for a bitflags type that default is
                // the *empty* set, which blends into nowhere.
                attachments: vec![ColorBlendAttachment {
                    color_write_mask: ColorComponentFlags::R
                        | ColorComponentFlags::G
                        | ColorComponentFlags::B
                        | ColorComponentFlags::A,
                    ..ColorTargetDesc::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        // A presentation pass has no depth attachment, so the pipeline must not
        // declare a depth test.
        desc.depth_stencil_state = None;

        let pipeline =
            device.create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])?;
        if !pipeline.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the resolve graphics pipeline was not created on the device".into(),
            ));
        }

        Ok(Self::with_state(
            ResolvePassState {
                pipeline,
                descriptor_layout,
                descriptor_set: None,
                render_pass,
                framebuffers: HashMap::new(),
                framebuffer_extent: None,
                framebuffer_generation: None,
                framebuffer_pinned: None,
                source_texture: None,
            },
            source,
            producer,
        ))
    }

    /// Pin the framebuffer the copy lands in, instead of the acquired image.
    ///
    /// The same escape hatch the geometry pass has: a test that reads the
    /// result back needs an offscreen target, because a presentation image ends
    /// in `PresentSrc` and cannot be copied out.
    pub fn set_framebuffer(
        state: &SharedState,
        framebuffer: Option<Framebuffer>,
        _width: u32,
        _height: u32,
    ) {
        if let Ok(mut state) = state.lock() {
            state.framebuffer_pinned = framebuffer;
        }
    }
}

/// The framebuffer the copy lands in this frame.
///
/// A pinned framebuffer wins — the caller said exactly where to copy, so
/// there is nothing to resolve. Otherwise the copy lands in the image the
/// renderer acquired, and the framebuffer for that image is built once and
/// reused: a swapchain cycles through a few images, and building a
/// framebuffer per frame would create a Vulkan object per frame.
fn resolve_target(
    state: &mut ResolvePassState,
    context: &RenderContext,
    device: &crate::rhi::Device,
) -> Option<Framebuffer> {
    if let Some(pinned) = state.framebuffer_pinned.as_ref() {
        return Some(pinned.clone());
    }
    let index = context.swapchain_image_index;
    let Some(view) = context.presentation_texture_views.get(index as usize) else {
        // The chain was rebuilt this frame and the recorded index no longer
        // names a live image. Drawing into it would be drawing into a
        // destroyed view.
        return None;
    };
    let (width, height) = context.drawable_size();
    if width == 0 || height == 0 {
        return None;
    }
    // The extent is part of the key: after a resize the attachments have to
    // be rebuilt, and a framebuffer for the old size would render into a frame
    // that no longer exists.
    //
    // The generation is the other half, and unlike the extent it guards
    // against memory that no longer exists: a chain can be rebuilt at the
    // *same* size, and then the extent matches while the images — and the
    // views the framebuffers are built on — are new ones. The old views are
    // destroyed with the old chain, so a framebuffer that still refers to them
    // refers to freed memory: the validator names an invalid `VkImageView` at
    // the next `vkCmdBeginRenderPass`, far from the call that caused it, and
    // the driver may take the process down with it.
    if state.framebuffer_generation != Some(context.swapchain_generation) {
        state.framebuffers.clear();
        state.framebuffer_generation = Some(context.swapchain_generation);
    }
    if state.framebuffer_extent != Some((width, height)) {
        state.framebuffers.clear();
        state.framebuffer_extent = Some((width, height));
    }
    if let Some(existing) = state.framebuffers.get(&index) {
        return Some(existing.clone());
    }
    // A real framebuffer, through the device: `Framebuffer::new` is a CPU-side
    // description with no native handle, and a render pass begun against it is
    // refused by the translator with "framebuffer has no GPU backing".
    let framebuffer = match device.create_gpu_framebuffer(&FramebufferDesc {
        render_pass: state.render_pass.clone(),
        attachments: vec![FramebufferAttachment {
            texture_view: view.clone(),
            layer: 0,
            mip_level: 0,
        }],
        width,
        height,
        layers: 1,
    }) {
        Ok(framebuffer) => framebuffer,
        Err(e) => {
            crate::error!("resolve", "the framebuffer could not be created: {e}");
            return None;
        }
    };
    state.framebuffers.insert(index, framebuffer.clone());
    Some(framebuffer)
}

impl RenderPass for ResolvePass {
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
        // The source is the graph's resource. The pass asks for it by name and
        // the graph created it, because the pass that produces it is in this
        // frame.
        let source = resources.get(&self.source).and_then(|resource| {
            let texture = resource.texture()?;
            let view = resource.view()?;
            Some((texture.clone(), view.clone()))
        });
        let Some((texture, view)) = source else {
            crate::error!(
                "resolve",
                "the graph has no resource {:?}; the pass will not draw",
                self.source
            );
            if let Ok(mut state) = self.state.as_ref().expect("the state exists").lock() {
                state.descriptor_set = None;
            }
            return;
        };

        let Some(state) = self.state.as_ref() else {
            return;
        };
        let Ok(mut state) = state.lock() else {
            return;
        };

        let set = match device.create_descriptor_set(&state.descriptor_layout) {
            Ok(set) => set,
            Err(e) => {
                crate::error!("resolve", "the descriptor set could not be created: {e}");
                state.descriptor_set = None;
                return;
            }
        };
        if let Err(e) = device.write_descriptors(
            &set,
            &[DescriptorWrite {
                dst_set: set.clone(),
                dst_binding: 0,
                dst_array_element: 0,
                descriptors: vec![DescriptorInfo::Texture(view, None)],
            }],
        ) {
            crate::error!("resolve", "the descriptors could not be written: {e}");
            state.descriptor_set = None;
            return;
        }
        state.descriptor_set = Some(set);

        // Remember the source image, so the barrier can name it.
        state.source_texture = Some(texture);
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) -> PassOutcome {
        if !self.base.is_enabled() {
            return PassOutcome::skipped("disabled");
        }
        let Some(state) = self.state.as_ref() else {
            return PassOutcome::skipped("the pass has no state");
        };
        let device = context.device().clone();
        let Ok(mut state) = state.lock() else {
            return PassOutcome::skipped("the state lock is poisoned");
        };
        let Some(descriptor_set) = state.descriptor_set.clone() else {
            return PassOutcome::skipped("no descriptor set");
        };
        let Some(framebuffer) = resolve_target(&mut state, context, device.as_ref()) else {
            return PassOutcome::skipped("no target for the acquired image");
        };

        let (width, height) = context.drawable_size();
        let mut commands: Vec<Command> = Vec::with_capacity(10);

        // The barrier the chain needs, and the same lesson as the lighting
        // pass: the graph gives order, but an order is not a memory
        // dependency. Without this the copy reads the source texture while it
        // is still being written, and the picture changes from run to run.
        if let Some(texture) = state.source_texture.as_ref() {
            commands.push(Command::PipelineBarrier {
                src_stage: crate::rhi::PipelineStage::COLOR_ATTACHMENT_OUTPUT,
                dst_stage: crate::rhi::PipelineStage::FRAGMENT_SHADER,
                memory_barriers: vec![],
                buffer_barriers: vec![],
                texture_barriers: vec![TextureBarrier {
                    texture: texture.clone(),
                    src_access: AccessFlags::COLOR_ATTACHMENT_WRITE,
                    dst_access: AccessFlags::SHADER_READ,
                    old_layout: TextureLayout::ShaderReadOnlyOptimal,
                    new_layout: TextureLayout::ShaderReadOnlyOptimal,
                }],
            });
        }

        commands.push(Command::BeginRenderPass(
            crate::rhi::command::pass::render::RenderPassBeginInfo {
                render_pass: state.render_pass.clone(),
                framebuffer: framebuffer.clone(),
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D { width, height },
                },
                clear_values: vec![crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0)],
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
        commands.push(Command::BindDescriptorSets {
            pipeline: state.pipeline.clone(),
            first_set: 0,
            sets: vec![descriptor_set.clone()],
        });
        commands.push(Command::DrawIndexed {
            index_count: 3,
            instance_count: 1,
            first_index: 0,
            vertex_offset: 0,
            first_instance: 0,
        });
        commands.push(Command::EndRenderPass);

        for command in commands {
            if !encoder.record(command) {
                crate::error!("resolve", "a command was rejected: the encoder is not recording");
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
