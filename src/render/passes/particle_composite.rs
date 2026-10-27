//! The Particle Composite Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The link the particle pass was missing: it blends the particle pass's own
//! image **over** the frame, so the particles the pass drew become part of the
//! picture instead of sitting in a texture nobody read — the same role the
//! resolve pass plays for the deferred chain, and the same split of duties:
//! the particle pass draws; whoever owns the frame decides where the drawing
//! lands, and this pass is the wiring.
//!
//! The blend is the whole proof here. The particle pass draws its sprites over
//! a transparent clear with the alpha factors, which leaves the image in
//! **premultiplied** form — the stacked sprites' colour is already multiplied
//! by their accumulated coverage, and the alpha channel is that coverage. So
//! the composite writes with the premultiplied factors —
//! `result = src + dst * (1 - src.a)` — and *not* with the straight-alpha
//! factors: multiplying the already-multiplied colour by its alpha again would
//! darken every half-covered particle twice over.
//!
//! Two target shapes, chosen by where the frame lives:
//!
//! - **over the acquired image** (`execute` with no pinned framebuffer): the
//!   overlay contract the presentation UI pass states — `LoadOp::Load`, the
//!   image starts in `PresentSrc` (where the presentation geometry pass leaves
//!   it) and ends in `PresentSrc` (what `vkQueuePresentKHR` needs).
//! - **over a pinned offscreen target** (`set_framebuffer`): the same Load,
//!   but the image starts and stays `ColorAttachmentOptimal` so a test can
//!   read the result back — a presentation image ends in `PresentSrc` and is
//!   neither readable nor copyable.
//!
//! The two render passes share one pipeline: render pass compatibility is
//! fixed by the attachment formats and the subpass layouts, and load ops and
//! initial/final layouts are not part of it — so one pipeline serves both.
//!
//! Like every pass in this tree, the composite puts an explicit barrier in
//! front of each of its two reads. The graph gives order, but an order is not
//! a memory dependency:
//!
//! - the **source** (the particle pass's image) is sampled by the fragment —
//!   the barrier is the mirror pass's reflection barrier: `COLOR_ATTACHMENT_
//!   WRITE → SHADER_READ`, and the layout moves from the particle render
//!   pass's `final_layout` fact (`ColorAttachmentOptimal`) to
//!   `ShaderReadOnlyOptimal`, where a sample descriptor is legal to bind. The
//!   backend's layout map cannot see a render pass boundary (the stated limit
//!   in the Vulkan backend), so the barrier does not ask the tracker — the
//!   same stated limit the mirror pass records, and with it the same drift
//!   between frames on the tracker's side of the picture.
//! - the **target** is loaded, and the load must see the previous pass's
//!   writes — a global memory barrier, `COLOR_ATTACHMENT_WRITE → COLOR_
//!   ATTACHMENT_READ` within `COLOR_ATTACHMENT_OUTPUT`, because the swapchain
//!   image has no typed `Texture` handle in the context to name.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::graph::types::ResourceUsage;
use crate::render::passes::base::BaseRenderPass;
use crate::render::passes::particles::ParticlePassState;
use crate::render::scene::Scene;
use crate::rhi::sync::barrier::TextureBarrier;
use crate::rhi::{
    AccessFlags, Command, CommandEncoder, DescriptorInfo, DescriptorSet, DescriptorSetLayout,
    DescriptorType, DescriptorWrite, Framebuffer, FramebufferAttachment, FramebufferDesc,
    GraphicsPipeline, Offset2D, PipelineStage, Rect2D, Texture, TextureLayout, Viewport,
};

/// Everything the pass needs to record, shared with the caller.
pub struct ParticleCompositePassState {
    pub pipeline: GraphicsPipeline,
    pub descriptor_layout: DescriptorSetLayout,
    /// One set, written once: the particle pass's image is stable for the
    /// composite's life — the pass draws into the same image every frame, so
    /// the set is written once and rebound per frame.
    pub descriptor_set: Option<DescriptorSet>,
    /// The pass over the frame: `Load`, the image in `PresentSrc` at both
    /// ends. The overlay contract of the presentation UI pass.
    pub render_pass_overlay: crate::rhi::RenderPass,
    /// The pass over a pinned target: `Load`, `ColorAttachmentOptimal` at
    /// both ends, so the result can be read back.
    pub render_pass_offscreen: crate::rhi::RenderPass,
    /// The framebuffers over the acquired images, one per image.
    ///
    /// A swapchain cycles through a few images, so a framebuffer per image is
    /// built once and reused. The extent and the **generation** are part of
    /// the key's invalidation: after a resize the attachments have to be
    /// rebuilt, and a chain can be rebuilt at the *same* size — the extent
    /// matches while the images, and the views the framebuffers are built on,
    /// are new ones. The old views are destroyed with the old chain, so a
    /// framebuffer that still refers to them refers to freed memory; the
    /// generation is what tells the two chains apart (the same key the
    /// resolve pass keeps).
    pub framebuffers: HashMap<u32, Framebuffer>,
    pub framebuffer_extent: Option<(u32, u32)>,
    pub framebuffer_generation: Option<u64>,
    /// The pinned framebuffer, for a caller that wants the composite
    /// somewhere specific — the same escape hatch the geometry and resolve
    /// passes have.
    pub framebuffer_pinned: Option<Framebuffer>,
    /// The image the composite reads, so the barrier can name it.
    pub source_texture: Option<Texture>,
    /// The particle pass whose image is composited. The image, its view and
    /// the live particle count are read through this handle: the composite
    /// must not blend a texture the particle pass drew nothing into, and the
    /// count is the particle pass's own skip condition.
    pub particle_state: Arc<Mutex<ParticlePassState>>,
}

/// The handle the caller keeps.
pub type SharedState = Arc<Mutex<ParticleCompositePassState>>;

/// Blends the particle pass's image over the frame.
pub struct ParticleCompositePass {
    base: BaseRenderPass,
    state: Option<SharedState>,
}

impl ParticleCompositePass {
    /// The pass's base.
    ///
    /// The dependency names the **pass** that produces the image (for order —
    /// the composite must record after the particle pass) and the **resource**
    /// that carries it. The resource is not in the graph's catalogue, so the
    /// graph creates nothing for it; the image arrives through the particle
    /// pass's own state, and the dependency is what puts the composite in
    /// order behind its producer.
    fn base() -> BaseRenderPass {
        let mut base = BaseRenderPass::new("particles_composite");
        base.add_dependency("particles", "particles", ResourceUsage::Sampled);
        base.add_output("swapchain");
        base
    }

    /// Build a pass with no GPU objects yet.
    pub fn new() -> Self {
        Self {
            base: Self::base(),
            state: None,
        }
    }

    /// Build a pass around a new shared state and return both.
    pub fn with_state(state: ParticleCompositePassState) -> (Self, SharedState) {
        let shared: SharedState = Arc::new(Mutex::new(state));
        let pass = Self {
            base: Self::base(),
            state: Some(shared.clone()),
        };
        (pass, shared)
    }

    /// Build a composite pass with real GPU objects.
    ///
    /// `color_format` is the format of the image the composite lands in — the
    /// surface's format for the overlay, `RGBA8_UNORM` for a pinned offscreen
    /// target the test reads back. A pipeline declares its colour target
    /// format, and using a different one than the framebuffer's is undefined
    /// behaviour rather than a warning.
    ///
    /// `spirv` is the fullscreen copy program (`resolve.spv`): one oversized
    /// triangle from the vertex index, the source read with `textureLoad`.
    /// The composite adds no shader of its own — the blending that makes it a
    /// composite instead of a copy lives in the pipeline's blend state.
    pub fn create(
        device: &crate::rhi::Device,
        particle_state: Arc<Mutex<ParticlePassState>>,
        color_format: crate::rhi::Format,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::{BlendFactor, BlendOp, PrimitiveTopology};
        use crate::rhi::{
            ColorBlendAttachment, ColorBlendState, ColorComponentFlags, GraphicsPipelineDesc,
            InputAssemblyState, MultisampleState, PipelineShaderStage, RasterizerState,
            ShaderFormat, ShaderModuleDesc,
        };

        // Over the frame: the overlay contract — Load, PresentSrc at both
        // ends. The image is in PresentSrc when this pass begins, because the
        // presentation pass that drew before it ends the frame there.
        let render_pass_overlay = device
            .create_gpu_render_pass(&crate::rhi::RenderPassDesc {
                attachments: vec![crate::rhi::AttachmentDescription {
                    format: color_format,
                    samples: crate::rhi::SampleCount::X1,
                    load_op: crate::rhi::LoadOp::Load,
                    store_op: crate::rhi::StoreOp::Store,
                    stencil_load_op: crate::rhi::LoadOp::DontCare,
                    stencil_store_op: crate::rhi::StoreOp::DontCare,
                    initial_layout: TextureLayout::PresentSrc,
                    final_layout: TextureLayout::PresentSrc,
                }],
                subpasses: vec![crate::rhi::SubpassDescription {
                    pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
                    color_attachments: vec![crate::rhi::AttachmentReference {
                        attachment: 0,
                        layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                    }],
                    depth_stencil_attachment: None,
                    ..Default::default()
                }],
                dependencies: vec![],
            })
            .map_err(|e| RhiError::BackendError(format!("the composite overlay render pass: {e}")))?;

        // Over a pinned target: the same Load, but the image is a colour
        // attachment at both ends, so the result is readable — and the fact
        // the promise names matches a pass that drew into the target before.
        let render_pass_offscreen = device
            .create_gpu_render_pass(&crate::rhi::RenderPassDesc {
                attachments: vec![crate::rhi::AttachmentDescription {
                    format: color_format,
                    samples: crate::rhi::SampleCount::X1,
                    load_op: crate::rhi::LoadOp::Load,
                    store_op: crate::rhi::StoreOp::Store,
                    stencil_load_op: crate::rhi::LoadOp::DontCare,
                    stencil_store_op: crate::rhi::StoreOp::DontCare,
                    initial_layout: TextureLayout::ColorAttachmentOptimal,
                    final_layout: TextureLayout::ColorAttachmentOptimal,
                }],
                subpasses: vec![crate::rhi::SubpassDescription {
                    pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
                    color_attachments: vec![crate::rhi::AttachmentReference {
                        attachment: 0,
                        layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                    }],
                    depth_stencil_attachment: None,
                    ..Default::default()
                }],
                dependencies: vec![],
            })
            .map_err(|e| {
                RhiError::BackendError(format!("the composite offscreen render pass: {e}"))
            })?;

        let descriptor_layout = device
            .create_descriptor_set_layout(
                &crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
                    bindings: vec![crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 0,
                        ty: DescriptorType::SampledTexture,
                        count: 1,
                        stages: crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    }],
                },
            )
            .map_err(|e| RhiError::BackendError(format!("the composite descriptor layout: {e}")))?;

        let module = device
            .create_shader_module(&ShaderModuleDesc {
                code: spirv.to_vec(),
                format: ShaderFormat::SpirV,
                entry_point: Some("vs_main".into()),
                name: Some("particles_composite".into()),
            })
            .map_err(|e| RhiError::BackendError(format!("the composite shader module: {e}")))?;

        let desc = GraphicsPipelineDesc {
            shader_stages: vec![
                PipelineShaderStage {
                    stage: crate::rhi::ShaderStage::VERTEX,
                    module: module.clone(),
                    entry_point: "vs_main".into(),
                },
                PipelineShaderStage {
                    stage: crate::rhi::ShaderStage::FRAGMENT,
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
                // The premultiplied factors, and the write mask has to be set
                // explicitly: `ColorComponentFlags` derives `Default`, and for
                // a bitflags type that default is the *empty* set, which
                // blends into nowhere.
                attachments: vec![ColorBlendAttachment {
                    blend_enable: true,
                    src_color_blend_factor: BlendFactor::One,
                    dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
                    color_blend_op: BlendOp::Add,
                    src_alpha_blend_factor: BlendFactor::One,
                    dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
                    alpha_blend_op: BlendOp::Add,
                    color_write_mask: ColorComponentFlags::R
                        | ColorComponentFlags::G
                        | ColorComponentFlags::B
                        | ColorComponentFlags::A,
                }],
                ..Default::default()
            }),
            // No depth state: both render passes have no depth attachment.
            depth_stencil_state: None,
            ..Default::default()
        };
        // One pipeline against the offscreen pass; the overlay pass is
        // compatible with it (same formats, same subpass layouts — load ops
        // and end layouts are not part of compatibility), so the same
        // pipeline records into either target.
        let pipeline =
            device.create_gpu_graphics_pipeline(&desc, &render_pass_offscreen, &[&descriptor_layout])?;
        // №642: an Ok pipeline is one the backend built — the refusal is an Err.

        Ok(Self::with_state(ParticleCompositePassState {
            pipeline,
            descriptor_layout,
            descriptor_set: None,
            render_pass_overlay,
            render_pass_offscreen,
            framebuffers: HashMap::new(),
            framebuffer_extent: None,
            framebuffer_generation: None,
            framebuffer_pinned: None,
            source_texture: None,
            particle_state,
        }))
    }

    /// Pin the framebuffer the composite lands in, instead of the acquired
    /// image.
    pub fn set_framebuffer(state: &SharedState, framebuffer: Option<Framebuffer>) {
        if let Ok(mut state) = state.lock() {
            state.framebuffer_pinned = framebuffer;
        }
    }

    /// The framebuffer the composite lands in this frame.
    ///
    /// A pinned framebuffer wins — the caller said exactly where to composite,
    /// and the offscreen render pass matches it. Otherwise the composite lands
    /// in the image the renderer acquired, and the framebuffer for that image
    /// is built once and reused: building a framebuffer per frame would create
    /// a Vulkan object per frame.
    fn target(
        state: &mut ParticleCompositePassState,
        context: &RenderContext,
        device: &crate::rhi::Device,
    ) -> Option<(Framebuffer, crate::rhi::RenderPass)> {
        if let Some(pinned) = state.framebuffer_pinned.as_ref() {
            return Some((pinned.clone(), state.render_pass_offscreen.clone()));
        }
        let index = context.swapchain_image_index;
        let Some(view) = context.presentation_texture_views.get(index as usize) else {
            // The chain was rebuilt this frame and the recorded index no
            // longer names a live image. Drawing into it would be drawing into
            // a destroyed view.
            return None;
        };
        let (width, height) = context.drawable_size();
        if width == 0 || height == 0 {
            return None;
        }
        // The extent is part of the key: after a resize the attachments have
        // to be rebuilt, and a framebuffer for the old size would render into
        // a frame that no longer exists. The generation is the other half: a
        // chain rebuilt at the *same* size has new images and new views, and
        // the old views are destroyed with it (the resolve pass's key; the
        // lesson is №681 — the extent alone does not guard freed memory).
        if state.framebuffer_generation != Some(context.swapchain_generation) {
            state.framebuffers.clear();
            state.framebuffer_generation = Some(context.swapchain_generation);
        }
        if state.framebuffer_extent != Some((width, height)) {
            state.framebuffers.clear();
            state.framebuffer_extent = Some((width, height));
        }
        if let Some(existing) = state.framebuffers.get(&index) {
            return Some((existing.clone(), state.render_pass_overlay.clone()));
        }
        // A real framebuffer, through the device: `Framebuffer::new` is a
        // CPU-side description with no native handle, and a render pass begun
        // against it is refused by the translator with "framebuffer has no GPU
        // backing".
        let framebuffer = match device.create_gpu_framebuffer(&FramebufferDesc {
            render_pass: state.render_pass_overlay.clone(),
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
                crate::error!("particles_composite", "the framebuffer could not be created: {e}");
                return None;
            }
        };
        state.framebuffers.insert(index, framebuffer.clone());
        Some((framebuffer, state.render_pass_overlay.clone()))
    }

    /// The image the composite reads: the particle pass's own image.
    pub fn output_texture(state: &SharedState) -> Option<Texture> {
        state.lock().ok().and_then(|state| state.source_texture.clone())
    }
}

impl RenderPass for ParticleCompositePass {
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
        _resources: &mut HashMap<String, GraphResource>,
    ) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        // The composite's own state first: the particle handle lives in it.
        let Ok(mut state) = state.lock() else {
            return;
        };
        // The image and its view live in the particle pass's state, not in the
        // graph: the pass draws into its own image, and the composite asks it
        // through the handle the caller wired.
        let Ok(particle) = state.particle_state.lock() else {
            crate::error!(
                "particles_composite",
                "the particle pass's state lock is poisoned; the pass will not draw"
            );
            state.descriptor_set = None;
            return;
        };
        let Some(texture) = particle.output_texture.clone() else {
            crate::error!(
                "particles_composite",
                "the particle pass has no image; the pass will not draw"
            );
            drop(particle);
            state.descriptor_set = None;
            return;
        };
        let Some(view) = particle.output_view.clone() else {
            crate::error!(
                "particles_composite",
                "the particle pass's image has no view; the pass will not draw"
            );
            drop(particle);
            state.descriptor_set = None;
            return;
        };
        drop(particle);

        let set = match device.create_descriptor_set(&state.descriptor_layout) {
            Ok(set) => set,
            Err(e) => {
                crate::error!("particles_composite", "the descriptor set could not be created: {e}");
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
            crate::error!("particles_composite", "the descriptors could not be written: {e}");
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

        // The live count, read through the particle pass's state: a frame
        // where the particle pass draws nothing must composite nothing —
        // blending a stale image over the frame would put last frame's
        // particles on this frame's picture.
        let live = {
            let Ok(particle) = state.particle_state.lock() else {
                return PassOutcome::skipped("the particle pass's state lock is poisoned");
            };
            let Some(system) = particle.system.as_ref() else {
                return PassOutcome::skipped("the particle pass has no system");
            };
            let Ok(system) = system.lock() else {
                return PassOutcome::skipped("the particle system's lock is poisoned");
            };
            system.particle_count()
        };
        if live == 0 {
            return PassOutcome::skipped("no live particles");
        }

        let Some((framebuffer, render_pass)) = Self::target(&mut state, context, device.as_ref())
        else {
            return PassOutcome::skipped("no target for the frame");
        };

        // The render area is the target's own size, not the context's: the
        // caller may pin a framebuffer of a different size than the
        // presentation (the headless pixel tests do exactly that), and a
        // render area larger than the attachment is undefined behaviour —
        // the driver survives single submissions of it and loses the device
        // at the next one. The particle pass reads its own image's size for
        // the same reason.
        let framebuffer_width = framebuffer.desc().width.max(1);
        let framebuffer_height = framebuffer.desc().height.max(1);
        let (width, height) = (framebuffer_width, framebuffer_height);
        let mut commands: Vec<Command> = Vec::with_capacity(11);

        // The source read: the same barrier the mirror pass puts in front of
        // the water's reflection read — the graph's order is not a memory
        // dependency, and a sample descriptor is legal only on an image in
        // `ShaderReadOnlyOptimal`. The old layout is the particle render
        // pass's `final_layout` — a fact from the pass's own description, not
        // the tracker, which cannot see a render pass boundary.
        if let Some(texture) = state.source_texture.as_ref() {
            commands.push(Command::PipelineBarrier {
                src_stage: PipelineStage::COLOR_ATTACHMENT_OUTPUT,
                dst_stage: PipelineStage::FRAGMENT_SHADER,
                memory_barriers: vec![],
                buffer_barriers: vec![],
                texture_barriers: vec![TextureBarrier {
                    texture: texture.clone(),
                    src_access: AccessFlags::COLOR_ATTACHMENT_WRITE,
                    dst_access: AccessFlags::SHADER_READ,
                    old_layout: TextureLayout::ColorAttachmentOptimal,
                    new_layout: TextureLayout::ShaderReadOnlyOptimal,
                }],
            });
        }

        // The target read: the Load must see the previous pass's writes, and
        // the graph's order is not a memory dependency. A global memory
        // barrier, because the swapchain image has no typed texture handle in
        // the context — and the layouts do not change, so no texture barrier
        // is needed.
        commands.push(Command::PipelineBarrier {
            src_stage: PipelineStage::COLOR_ATTACHMENT_OUTPUT,
            dst_stage: PipelineStage::COLOR_ATTACHMENT_OUTPUT,
            memory_barriers: vec![crate::rhi::sync::barrier::MemoryBarrier {
                src_access: AccessFlags::COLOR_ATTACHMENT_WRITE,
                dst_access: AccessFlags::COLOR_ATTACHMENT_READ,
            }],
            buffer_barriers: vec![],
            texture_barriers: vec![],
        });

        commands.push(Command::BeginRenderPass(
            crate::rhi::command::pass::render::RenderPassBeginInfo {
                render_pass,
                framebuffer,
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D { width, height },
                },
                // Load, not Clear: the frame is preserved. The value is not
                // read, and the count matches the attachment count the other
                // passes pass.
                clear_values: vec![crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 0.0)],
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
            sets: vec![descriptor_set],
        });
        commands.push(Command::Draw {
            vertex_count: 3,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
        });
        commands.push(Command::EndRenderPass);

        for command in commands {
            if !encoder.record(command) {
                crate::error!(
                    "particles_composite",
                    "a command was rejected: the encoder is not recording"
                );
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

impl Default for ParticleCompositePass {
    fn default() -> Self {
        Self::new()
    }
}
