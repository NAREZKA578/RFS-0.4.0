//! The Particle Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Draws the particle system's live particles: camera-facing quads
//! (billboards), one instance per particle, blended over the pass's own image
//! — the same arrangement the mirror pass draws its reflection with, and the
//! same split of duties. The pass draws; whoever owns the particles updates
//! and uploads them.
//!
//! The particles come from the `ParticleSystem`'s buffer, where the system's
//! own `update` uploads the live pool entries every frame. The pass reads the
//! count and the buffer under the caller's lock and draws them instanced —
//! one instance per live particle, the quad's corner in the vertex attribute,
//! the particle itself in the storage buffer at `@builtin(instance_index)`.
//!
//! There is no depth here: the particles are drawn over nothing, and
//! compositing the image over the frame is the consumer's business — the same
//! split as the mirror's reflection, which the water mixes. v1 wired the
//! drawing (№722) and does not yet have a consumer in the frame: no part of
//! the product runs this pass yet, which the pass says honestly by skipping
//! when it has no system, and which the pixel test proves by running it.
//!
//! What this replaces: `ParticleSystem::render` was the dead twin of this — a
//! `pipeline` that was `None` forever, an early return that silently drew
//! nothing, and binds (`bind_uniform_buffer`, `bind_texture`) the encoder
//! does not even implement. The system keeps the data and the upload; the
//! drawing lives here, through descriptor sets like every other pass.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::Mat4;

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::meshes::{IndexType, Mesh};
use crate::render::particles::ParticleSystem;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::{
    Command, CommandEncoder, DescriptorInfo, DescriptorSet, DescriptorSetLayout, DescriptorWrite,
    Framebuffer, FramebufferAttachment, FramebufferDesc, GraphicsPipeline, Offset2D, Rect2D,
};

/// The camera block, matching `particle.wgsl`: `mat4 view_proj` +
/// `vec4 camera_right` + `vec4 camera_up` = 96 bytes.
const CAMERA_UNIFORM_SIZE: u64 = 16 * 4 + 4 * 4 * 2;

/// The particle drawing configuration.
#[derive(Debug, Clone, Copy)]
pub struct ParticleConfig {
    /// The width of the pass's own image.
    pub width: u32,
    /// The height of the pass's own image.
    pub height: u32,
}

impl Default for ParticleConfig {
    fn default() -> Self {
        Self {
            width: 256,
            height: 256,
        }
    }
}

/// Everything the pass needs to record, shared with the caller.
pub struct ParticlePassState {
    pub pipeline: GraphicsPipeline,
    pub descriptor_layout: DescriptorSetLayout,
    /// One set, written once: the camera's uniform and the particle system's
    /// buffer. Both are stable for the pass's life — the uniform's *content*
    /// is uploaded per frame, the buffer's content by the system itself —
    /// so the set is written once in `create` and rebound per frame.
    pub descriptor_set: DescriptorSet,
    pub camera_uniform: crate::rhi::Buffer,
    pub render_pass: crate::rhi::RenderPass,
    /// Where the particles are drawn: the pass's own image.
    pub output_texture: Option<crate::rhi::Texture>,
    pub output_view: Option<crate::rhi::TextureView>,
    pub framebuffer: Option<Framebuffer>,
    pub config: ParticleConfig,
    /// The particles: the caller owns the system, updates it and uploads from
    /// it; the pass reads the live count and the buffer under the lock.
    pub system: Option<Arc<Mutex<ParticleSystem>>>,
    /// The unit quad every particle is drawn as. The pass builds its own
    /// buffers for it — `Mesh::fullscreen_quad` builds no buffers, and the
    /// particle system no longer holds a mesh of its own.
    pub quad: Option<Mesh>,
}

/// Shared handle to the pass state.
pub type SharedState = Arc<Mutex<ParticlePassState>>;

/// The particle pass.
pub struct ParticlePass {
    base: BaseRenderPass,
    state: Option<SharedState>,
}

impl ParticlePass {
    /// Build a pass with no GPU objects yet.
    pub fn new() -> Self {
        let base = BaseRenderPass::new("particles");
        Self { base, state: None }
    }

    /// Build a pass around a new shared state and return both.
    pub fn with_state(state: ParticlePassState) -> (Self, SharedState) {
        let mut base = BaseRenderPass::new("particles");
        base.add_output("particles");
        let shared: SharedState = Arc::new(Mutex::new(state));
        let pass = Self {
            base,
            state: Some(shared.clone()),
        };
        (pass, shared)
    }

    /// The configuration the pass was built with.
    pub fn config(&self) -> Option<ParticleConfig> {
        self.state.as_ref().and_then(|state| {
            state.lock().ok().map(|state| state.config)
        })
    }

    /// Build a particle pass with real GPU objects.
    ///
    /// `system` is the particle system the pass draws: it is wrapped in a
    /// mutex because the caller updates it while the pass draws it, and the
    /// pass reads the live count and the buffer under that lock. If the
    /// system has not allocated its buffer yet, `create` does it here — a
    /// pass whose input buffer does not exist has nothing to draw, and
    /// finding that out in `execute` every frame is finding it out too late.
    pub fn create(
        device: &crate::rhi::Device,
        system: Arc<Mutex<ParticleSystem>>,
        config: ParticleConfig,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::{BlendFactor, BlendOp, PrimitiveTopology};
        use crate::rhi::{
            ColorBlendAttachment, ColorBlendState, ColorComponentFlags, GraphicsPipelineDesc,
            InputAssemblyState, MultisampleState, PipelineShaderStage, RasterizerState,
            ShaderFormat, ShaderModuleDesc,
        };

        let width = config.width.max(1);
        let height = config.height.max(1);

        // The system's buffer: allocated here if the caller has not yet, so
        // the descriptor below has something real to point at.
        let particle_buffer = {
            let mut system = system.lock().map_err(|_| {
                RhiError::BackendError("the particle system's lock is poisoned".into())
            })?;
            if system.particle_buffer().is_none() {
                system.initialize().map_err(|e| {
                    RhiError::BackendError(format!("the particle system's initialization: {e}"))
                })?;
            }
            system
                .particle_buffer()
                .cloned()
                .expect("just initialized or already initialized")
        };
        let particle_buffer_size = std::mem::size_of::<crate::render::particles::Particle>()
            as u64
            * {
                let system = system.lock().map_err(|_| {
                    RhiError::BackendError("the particle system's lock is poisoned".into())
                })?;
                system.max_particles() as u64
            };

        // The image the particles are drawn into.
        let output_texture = device
            .create_texture(
                width,
                height,
                1,
                crate::rhi::Format::RGBA8_UNORM,
                // SAMPLED: the composite pass binds this image as a sample
                // descriptor — an image without the usage bit cannot back a
                // sampled view, and the composite is the pass's consumer.
                crate::rhi::TextureUsage::COLOR_ATTACHMENT
                    | crate::rhi::TextureUsage::TRANSFER_SRC
                    | crate::rhi::TextureUsage::SAMPLED,
                1,
            )
            .map_err(|e| RhiError::BackendError(format!("the particle output texture: {e}")))?;
        let output_view = device
            .create_texture_view(
                &output_texture,
                &crate::rhi::TextureViewDesc {
                    texture: output_texture.clone(),
                    format: None,
                    view_type: crate::rhi::TextureViewType::D2,
                    aspects: crate::rhi::TextureAspectFlags::COLOR,
                    base_mip_level: 0,
                    mip_level_count: 1,
                    base_array_layer: 0,
                    array_layer_count: 1,
                },
            )
            .map_err(|e| RhiError::BackendError(format!("the particle output view: {e}")))?;

        // A colour-only render pass: the particles write no depth here, and
        // the pipeline below declares no depth state — a pipeline with a
        // depth state against a pass without the attachment is invalid.
        let render_pass = device
            .create_gpu_render_pass(&crate::rhi::RenderPassDesc {
                attachments: vec![crate::rhi::AttachmentDescription {
                    format: crate::rhi::Format::RGBA8_UNORM,
                    samples: crate::rhi::SampleCount::X1,
                    load_op: crate::rhi::LoadOp::Clear,
                    store_op: crate::rhi::StoreOp::Store,
                    stencil_load_op: crate::rhi::LoadOp::DontCare,
                    stencil_store_op: crate::rhi::StoreOp::DontCare,
                    initial_layout: crate::rhi::TextureLayout::Undefined,
                    final_layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
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
            .map_err(|e| RhiError::BackendError(format!("the particle render pass: {e}")))?;

        let framebuffer = device
            .create_gpu_framebuffer(&FramebufferDesc {
                render_pass: render_pass.clone(),
                attachments: vec![FramebufferAttachment {
                    texture_view: output_view.clone(),
                    layer: 0,
                    mip_level: 0,
                }],
                width,
                height,
                layers: 1,
            })
            .map_err(|e| RhiError::BackendError(format!("the particle framebuffer: {e}")))?;

        // The descriptor layout: the camera's uniform and the particle
        // system's buffer. The particles are read in the **vertex** stage —
        // the billboard's corners are walked out there — and not touched by
        // the fragment, so both bindings are vertex-stage only.
        let descriptor_layout = device.create_descriptor_set_layout(
            &crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
                bindings: vec![
                    // 0: the camera's uniform.
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 0,
                        ty: crate::rhi::DescriptorType::UniformBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::VERTEX,
                        immutable_samplers: None,
                    },
                    // 1: the particles, read-only.
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 1,
                        ty: crate::rhi::DescriptorType::StorageBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::VERTEX,
                        immutable_samplers: None,
                    },
                ],
            },
        )
        .map_err(|e| RhiError::BackendError(format!("the particle descriptor layout: {e}")))?;

        let camera_uniform = device
            .create_buffer(CAMERA_UNIFORM_SIZE, crate::rhi::BufferUsage::UNIFORM, true)
            .map_err(|e| RhiError::BackendError(format!("the particle camera uniform: {e}")))?;

        let descriptor_set = device.create_descriptor_set(&descriptor_layout)?;
        device.write_descriptors(
            &descriptor_set,
            &[
                DescriptorWrite {
                    dst_set: descriptor_set.clone(),
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(
                        camera_uniform.clone(),
                        0,
                        CAMERA_UNIFORM_SIZE,
                    )],
                },
                DescriptorWrite {
                    dst_set: descriptor_set.clone(),
                    dst_binding: 1,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(
                        particle_buffer.clone(),
                        0,
                        particle_buffer_size,
                    )],
                },
            ],
        )?;

        // The unit quad every particle is drawn as, with the buffers the
        // draw needs. `Mesh::fullscreen_quad` builds no buffers; without
        // these the draw binds nothing.
        let mut quad = Mesh::fullscreen_quad("particle_quad");
        quad.create_buffers(device)
            .map_err(|e| RhiError::BackendError(format!("the particle quad mesh: {e}")))?;

        let module = device
            .create_shader_module(&ShaderModuleDesc {
                code: spirv.to_vec(),
                format: ShaderFormat::SpirV,
                entry_point: Some("vs_main".into()),
                name: Some("particles".into()),
            })
            .map_err(|e| RhiError::BackendError(format!("the particle shader module: {e}")))?;

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
            // The quad is the mesh's standard vertex layout: the shader reads
            // only the corner (location 0) and the instance index, and the
            // unused attributes the pipeline declares are simply unused.
            vertex_input_state: Some(crate::render::passes::geometry::vertex_input_state()),
            input_assembly_state: InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            // A billboard may face any way after the camera's basis walks it
            // out; winding is not something the pass can promise.
            rasterizer_state: Some(RasterizerState::double_sided()),
            multisample_state: MultisampleState {
                sample_count: crate::rhi::SampleCount::X1,
                ..Default::default()
            },
            // The particles are translucent: the standard alpha blend, the
            // factors the transparent pass's `BlendMode::Alpha` uses. The
            // colour is `src * a + dst * (1 - a)`, and the alpha accumulates
            // `src.a + dst.a * (1 - a)` — the two factor pairs are separate,
            // because reusing the colour's factors for the alpha squares the
            // alpha and a half-transparent particle would be twice as
            // transparent as it was asked to be.
            color_blend_state: Some(ColorBlendState {
                attachments: vec![ColorBlendAttachment {
                    blend_enable: true,
                    src_color_blend_factor: BlendFactor::SrcAlpha,
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
            // No depth state: the render pass has no depth attachment.
            ..Default::default()
        };
        let pipeline =
            device.create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])?;

        Ok(Self::with_state(ParticlePassState {
            pipeline,
            descriptor_layout,
            descriptor_set,
            camera_uniform,
            render_pass,
            output_texture: Some(output_texture),
            output_view: Some(output_view),
            framebuffer: Some(framebuffer),
            config,
            system: Some(system),
            quad: Some(quad),
        }))
    }

    /// The camera the billboards face: the view-projection, and the camera's
    /// right and up axes in the world.
    ///
    /// The right and up come from the **view matrix's rows**: the rotation
    /// part of a view matrix has the camera's basis as its rows, and glam
    /// exposes columns (`x_axis`, ...), so a row is the same component of
    /// each column. A billboard's corners are walked along these, which is
    /// the whole reason the uniform carries them: the shader has no other way
    /// to know which way the camera faces.
    pub fn update_uniforms(
        state: &SharedState,
        device: &crate::rhi::Device,
        view: Mat4,
        projection: Mat4,
    ) -> bool {
        let Ok(state) = state.lock() else {
            return false;
        };
        let view_proj = projection * view;
        let right = Vec3Row {
            x: view.x_axis.x,
            y: view.y_axis.x,
            z: view.z_axis.x,
        };
        let up = Vec3Row {
            x: view.x_axis.y,
            y: view.y_axis.y,
            z: view.z_axis.y,
        };
        let mut data = [0f32; 24];
        data[..16].copy_from_slice(&view_proj.to_cols_array());
        data[16..20].copy_from_slice(&[right.x, right.y, right.z, 0.0]);
        data[20..24].copy_from_slice(&[up.x, up.y, up.z, 0.0]);
        device.upload_buffer(&state.camera_uniform, &data).is_ok()
    }

    /// The image the particles were drawn into.
    pub fn output_texture(state: &SharedState) -> Option<crate::rhi::Texture> {
        state
            .lock()
            .ok()
            .and_then(|state| state.output_texture.clone())
    }
}

/// Three floats, for the camera basis above. A named struct keeps the two
/// extractions from the view matrix's rows readable next to each other.
struct Vec3Row {
    x: f32,
    y: f32,
    z: f32,
}

impl RenderPass for ParticlePass {
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
        // Everything the pass needs was built with the device in `create`.
        // The particles are the caller's: the system is updated and uploaded
        // by whoever owns it, and the pass draws what is in the buffer now.
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        _context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) -> PassOutcome {
        if !self.base.is_enabled() {
            return PassOutcome::skipped("disabled");
        }
        let Some(state) = self.state.as_ref() else {
            return PassOutcome::skipped("the pass has no state");
        };
        let Ok(state) = state.lock() else {
            return PassOutcome::skipped("the state lock is poisoned");
        };
        let Some(framebuffer) = state.framebuffer.clone() else {
            return PassOutcome::skipped("no framebuffer");
        };
        let Some(output) = state.output_texture.as_ref() else {
            return PassOutcome::skipped("no output image");
        };
        let (width, height) = (output.width(), output.height());
        if width == 0 || height == 0 {
            return PassOutcome::skipped("the target has no size");
        }
        let Some(system) = state.system.as_ref() else {
            return PassOutcome::skipped("the pass has no particle system");
        };

        // The live count and the buffer are read under the caller's lock: the
        // caller updates and uploads the particles between frames, and a
        // count read without the lock could disagree with the buffer the
        // draw binds — instances the buffer does not hold.
        let (count, has_buffer) = {
            let Ok(system) = system.lock() else {
                return PassOutcome::skipped("the particle system's lock is poisoned");
            };
            (system.particle_count(), system.particle_buffer().is_some())
        };
        if count == 0 {
            return PassOutcome::skipped("no live particles");
        }
        // The descriptors were written once, with the system's buffer. A
        // system without one would mean a set pointing at nothing real — the
        // draw would read whatever the binding left, so the pass refuses.
        if !has_buffer {
            return PassOutcome::skipped("the particle system has no buffer");
        }

        let Some(quad) = state.quad.as_ref() else {
            return PassOutcome::skipped("the pass has no quad mesh");
        };
        let (Some(vertex_buffer), Some(index_buffer)) =
            (quad.vertex_buffer().cloned(), quad.index_buffer().cloned())
        else {
            return PassOutcome::skipped("the quad mesh has no buffers");
        };
        let index_type = match quad.index_type() {
            IndexType::U16 => crate::rhi::IndexType::U16,
            IndexType::U32 => crate::rhi::IndexType::U32,
            IndexType::U8 => return PassOutcome::skipped("8-bit indices are not supported"),
        };

        let mut commands: Vec<Command> = Vec::with_capacity(9);
        // The pass's own image is cleared: the particles are blended over
        // nothing, and what the last frame left there would show through.
        commands.push(Command::BeginRenderPass(
            crate::rhi::command::pass::render::RenderPassBeginInfo {
                render_pass: state.render_pass.clone(),
                framebuffer,
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D { width, height },
                },
                clear_values: vec![crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 0.0)],
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
        commands.push(Command::BindGraphicsPipeline(state.pipeline.clone()));
        commands.push(Command::BindDescriptorSets {
            pipeline: state.pipeline.clone(),
            first_set: 0,
            sets: vec![state.descriptor_set.clone()],
        });
        commands.push(Command::BindVertexBuffers {
            first_binding: 0,
            buffers: vec![((*vertex_buffer).clone(), 0)],
        });
        commands.push(Command::BindIndexBuffer {
            buffer: (*index_buffer).clone(),
            offset: 0,
            index_type,
        });
        // One instance per live particle: the vertex stage picks the
        // particle's data by `@builtin(instance_index)`, and the order the
        // system uploaded in is the order the instance index counts in.
        commands.push(Command::DrawIndexed {
            index_count: quad.index_count(),
            instance_count: count,
            first_index: 0,
            vertex_offset: 0,
            first_instance: 0,
        });
        commands.push(Command::EndRenderPass);

        for command in commands {
            if !encoder.record(command) {
                crate::error!("particles", "a command was rejected: the encoder is not recording");
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

impl Default for ParticlePass {
    fn default() -> Self {
        Self::new()
    }
}
