//! Forward Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Draws the scene lit: the geometry, the light, and the shadow map. It is the
//! forward path, and it is the consumer of the shadow map — without it the
//! shadow pass writes a picture nobody looks at.
//!
//! The light is one light for now, in the pass's own uniform block. The
//! clustered path (many lights, binned into screen clusters) is the next step
//! of the Forward+ plan, and it changes where the lights come from, not what
//! the shader does with one of them.
//!
//! The shadow map is an *input*: the shadow pass renders it, this pass reads
//! it, and the two agree because they use the same light-space matrix. That
//! matrix is the one thing that has to be the same, and it is uploaded into
//! both.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::{Mat4, Vec3};

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::passes::base::BaseRenderPass;
use crate::render::passes::geometry::GeometryItem;
use crate::render::scene::Scene;
use crate::rhi::{
    Command, CommandEncoder, DescriptorInfo, DescriptorSet, DescriptorSetLayout, DescriptorWrite,
    Framebuffer, FramebufferAttachment, FramebufferDesc, GraphicsPipeline, Offset2D, Rect2D,
};

/// The uniform block, matching `forward.wgsl`:
/// `mat4 mvp` + `mat4 model` + `mat4 light_space_matrix` + three `vec4` =
/// 192 + 48 = 240 bytes.
const UNIFORM_SIZE: u64 = 16 * 4 * 3 + 4 * 4 * 3;

/// Everything the pass needs to record, shared with the caller.
pub struct ForwardPassState {
    pub pipeline: GraphicsPipeline,
    pub descriptor_layout: DescriptorSetLayout,
    /// One descriptor set per item: the uniform is per item, and the set holds
    /// the uniform and the shadow map together.
    pub descriptor_sets: Vec<DescriptorSet>,
    /// One uniform buffer per item.
    pub uniform_buffers: Vec<crate::rhi::Buffer>,
    pub items: Vec<GeometryItem>,
    pub render_pass: crate::rhi::RenderPass,
    /// The shadow map the pass reads.
    pub shadow_view: Option<crate::rhi::TextureView>,
    /// The comparison sampler the shadow map is read with.
    ///
    /// A depth image may only be filtered by a comparison sampler, and the
    /// RHI refuses anything else (§683).
    pub shadow_sampler: Option<crate::rhi::Sampler>,
    pub framebuffer: Option<Framebuffer>,
    pub framebuffer_pinned: bool,
    pub framebuffer_image: Option<u32>,
    pub framebuffer_extent: Option<(u32, u32)>,
    pub framebuffer_generation: Option<u64>,
    /// The depth the geometry is tested against.
    pub depth_texture: Option<crate::rhi::Texture>,
    pub depth_view: Option<crate::rhi::TextureView>,
    /// The light's view-projection, the same one the shadow pass used.
    pub light_space_matrix: Mat4,
}

/// Shared handle to the pass state.
pub type SharedState = Arc<Mutex<ForwardPassState>>;

/// The light the pass lights the scene with.
#[derive(Debug, Clone, Copy)]
pub struct ForwardLight {
    pub position: Vec3,
    pub color: Vec3,
    pub intensity: f32,
    pub radius: f32,
}

impl Default for ForwardLight {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, 5.0, 5.0),
            color: Vec3::ONE,
            intensity: 1.0,
            radius: 50.0,
        }
    }
}

/// Forward pass.
pub struct ForwardPass {
    base: BaseRenderPass,
    state: Option<SharedState>,
}

impl ForwardPass {
    /// Build a pass with no GPU objects yet.
    pub fn new() -> Self {
        let mut base = BaseRenderPass::new("forward");
        // The shadow map is produced by the shadow pass, and the pass has to
        // come after it: the map is the whole point of the shadow pass.
        base.add_dependency("shadow", "shadow_map", crate::render::graph::types::ResourceUsage::Sampled);
        base.add_output("swapchain");
        Self {
            base,
            state: None,
        }
    }

    /// Build a pass around a new shared state and return both.
    pub fn with_state(state: ForwardPassState) -> (Self, SharedState) {
        let mut base = BaseRenderPass::new("forward");
        base.add_dependency("shadow", "shadow_map", crate::render::graph::types::ResourceUsage::Sampled);
        base.add_output("swapchain");
        let shared: SharedState = Arc::new(Mutex::new(state));
        let pass = Self {
            base,
            state: Some(shared.clone()),
        };
        (pass, shared)
    }

    /// Build a forward pass with real GPU objects.
    pub fn create(
        device: &crate::rhi::Device,
        items: Vec<GeometryItem>,
        color_format: crate::rhi::Format,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::PrimitiveTopology;
        use crate::rhi::{
            DepthStencilState, GraphicsPipelineDesc, InputAssemblyState, MultisampleState,
            PipelineShaderStage, RasterizerState, ShaderFormat, ShaderModuleDesc,
        };

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
            .map_err(|e| RhiError::BackendError(format!("the forward render pass: {e}")))?;

        let descriptor_layout = device.create_descriptor_set_layout(
            &crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
                bindings: vec![
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 0,
                        ty: crate::rhi::DescriptorType::UniformBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::VERTEX | crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 1,
                        ty: crate::rhi::DescriptorType::SampledTexture,
                        count: 1,
                        stages: crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 2,
                        ty: crate::rhi::DescriptorType::Sampler,
                        count: 1,
                        stages: crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                ],
            },
        );

        // The comparison sampler: a depth image may only be filtered by one,
        // and the RHI refuses anything else for a depth view (§683).
        let shadow_sampler = device.create_sampler_from_desc(&crate::rhi::resource::SamplerDesc {
            mag_filter: crate::rhi::FilterMode::Linear,
            min_filter: crate::rhi::FilterMode::Linear,
            address_mode_u: crate::rhi::AddressMode::ClampToEdge,
            address_mode_v: crate::rhi::AddressMode::ClampToEdge,
            compare_enable: true,
            compare_op: crate::rhi::CompareOp::LessOrEqual,
            ..Default::default()
        })?;

        let mut uniform_buffers = Vec::with_capacity(items.len());
        let mut descriptor_sets = Vec::with_capacity(items.len());
        for _ in 0..items.len() {
            let uniform = device.create_buffer(UNIFORM_SIZE, crate::rhi::BufferUsage::UNIFORM, true);
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

        let module = device.create_shader_module(&ShaderModuleDesc {
            code: spirv.to_vec(),
            format: ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("forward".into()),
        });
        if !module.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the forward shader module was not created on the device".into(),
            ));
        }

        let mut desc = GraphicsPipelineDesc {
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
            // The scene is opaque: the forward pass writes the colour, it does
            // not mix it. The blend state still has to declare the attachment
            // the render pass has, though — a pipeline whose colour blend
            // state declares no attachments against a render pass with one is
            // invalid, and an invalid pipeline draws nothing.
            color_blend_state: Some(crate::rhi::ColorBlendState {
                attachments: vec![crate::rhi::ColorBlendAttachment {
                    blend_enable: false,
                    color_write_mask: crate::rhi::ColorComponentFlags::R
                        | crate::rhi::ColorComponentFlags::G
                        | crate::rhi::ColorComponentFlags::B
                        | crate::rhi::ColorComponentFlags::A,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        desc.depth_stencil_state = Some(DepthStencilState::enabled());
        let pipeline = device.create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])?;
        if !pipeline.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the forward graphics pipeline was not created on the device".into(),
            ));
        }

        Ok(Self::with_state(ForwardPassState {
            pipeline,
            descriptor_layout,
            descriptor_sets,
            uniform_buffers,
            items,
            render_pass,
            shadow_view: None,
            shadow_sampler: Some(shadow_sampler),
            framebuffer: None,
            framebuffer_pinned: false,
            framebuffer_image: None,
            framebuffer_extent: None,
            framebuffer_generation: None,
            depth_texture: None,
            depth_view: None,
            light_space_matrix: Mat4::IDENTITY,
        }))
    }

    /// The shadow map the pass reads.
    pub fn set_shadow_map(state: &SharedState, view: Option<crate::rhi::TextureView>) {
        let Ok(mut state) = state.lock() else {
            return;
        };
        state.shadow_view = view;
    }

    /// Pin the framebuffer the scene is drawn into.
    pub fn set_framebuffer(state: &SharedState, framebuffer: Option<Framebuffer>) {
        if let Ok(mut state) = state.lock() {
            let pinned = framebuffer.is_some();
            state.framebuffer = framebuffer;
            state.framebuffer_pinned = pinned;
        }
    }

    /// The camera matrices, the light and the light-space matrix.
    ///
    /// The light-space matrix is the shadow pass's, so that the depth this
    /// shader compares against the map with is the depth the map holds.
    pub fn update_uniforms(
        state: &SharedState,
        device: &crate::rhi::Device,
        view: Mat4,
        projection: Mat4,
        light: ForwardLight,
        light_space_matrix: Mat4,
        ambient: Vec3,
    ) -> bool {
        let Ok(mut state) = state.lock() else {
            return false;
        };
        state.light_space_matrix = light_space_matrix;
        let mvp = projection * view;
        let mut ok = true;
        for (index, item) in state.items.iter().enumerate() {
            let Some(buffer) = state.uniform_buffers.get(index) else {
                continue;
            };
            let mut data = [0f32; 60];
            data[..16].copy_from_slice(&mvp.to_cols_array());
            data[16..32].copy_from_slice(&item.model.to_cols_array());
            data[32..48].copy_from_slice(&light_space_matrix.to_cols_array());
            data[48..51].copy_from_slice(&light.position.to_array());
            data[51] = light.radius;
            data[52..55].copy_from_slice(&light.color.to_array());
            data[55] = light.intensity;
            data[56..59].copy_from_slice(&ambient.to_array());
            data[59] = 0.0;
            if device.upload_buffer(buffer, &data).is_err() {
                ok = false;
            }
        }
        ok
    }
}

/// The framebuffer the scene is drawn into this frame.
fn resolve_target(
    state: &mut ForwardPassState,
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
    // The depth attachment is the pass's own, and it is rebuilt when the size
    // changes: a framebuffer's attachments must all be the size of the render
    // area, and the old framebuffer holds the old depth's view.
    let depth_stale = state
        .depth_texture
        .as_ref()
        .is_none_or(|texture| texture.width() != width || texture.height() != height);
    if depth_stale {
        let depth = device.create_texture(
            width,
            height,
            1,
            crate::rhi::Format::D32_SFLOAT,
            crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT,
            1,
        );
        let depth_view = match device.create_texture_view(
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
                crate::error!("forward", "the depth view could not be created: {e}");
                return None;
            }
        };
        state.depth_texture = Some(depth);
        state.depth_view = Some(depth_view);
        state.framebuffer = None;
    }
    let Some(depth_view) = state.depth_view.clone() else {
        return None;
    };
    if state.framebuffer_image == Some(index) {
        if let Some(framebuffer) = state.framebuffer.clone() {
            return Some(framebuffer);
        }
    }
    let framebuffer = match device.create_gpu_framebuffer(&FramebufferDesc {
        render_pass: state.render_pass.clone(),
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
            crate::error!("forward", "the framebuffer could not be created: {e}");
            return None;
        }
    };
    state.framebuffer_image = Some(index);
    state.framebuffer = Some(framebuffer.clone());
    Some(framebuffer)
}

impl RenderPass for ForwardPass {
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
        // Everything the pass needs was built with the device in `create`. The
        // shadow map is set by the caller, because only the caller knows which
        // shadow pass rendered it.
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
        let Ok(mut state) = state.lock() else {
            return PassOutcome::skipped("the state lock is poisoned");
        };
        if state.items.is_empty() {
            return PassOutcome::skipped("no items to draw");
        }
        // Without the shadow map there is nothing to compare the fragment's
        // depth against, and the shadow map is the point of the pass.
        let Some(shadow_view) = state.shadow_view.clone() else {
            return PassOutcome::skipped("no shadow map: the shadow pass has to run first");
        };
        let Some(shadow_sampler) = state.shadow_sampler.clone() else {
            return PassOutcome::skipped("no comparison sampler for the shadow map");
        };
        let device = context.device().clone();
        let Some(framebuffer) = resolve_target(&mut state, context, device.as_ref()) else {
            return PassOutcome::skipped("no target for the acquired image");
        };

        // The size of the target, not of the window.
        let desc = framebuffer.desc();
        let (width, height) = (desc.width, desc.height);
        if width == 0 || height == 0 {
            return PassOutcome::skipped("the target has no size");
        }

        let mut commands: Vec<Command> = Vec::with_capacity(9 + state.items.len() * 4);

        // The barrier before the shadow map is read.
        //
        // The graph orders the passes, and an order is not a memory dependency:
        // without this the shader compares the fragment's depth against a map
        // that is still being written, and the shadow moves with the frame.
        let Some(shadow_texture) = shadow_texture_of(&shadow_view) else {
            return PassOutcome::skipped("the shadow map's texture is unknown");
        };
        commands.push(Command::PipelineBarrier {
            src_stage: crate::rhi::PipelineStage::LATE_FRAGMENT_TESTS,
            dst_stage: crate::rhi::PipelineStage::FRAGMENT_SHADER,
            memory_barriers: vec![],
            buffer_barriers: vec![],
            texture_barriers: vec![crate::rhi::sync::barrier::TextureBarrier {
                texture: shadow_texture,
                src_access: crate::rhi::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                dst_access: crate::rhi::AccessFlags::SHADER_READ,
                old_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
                new_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
            }],
        });

        commands.push(Command::BeginRenderPass(
            crate::rhi::command::pass::render::RenderPassBeginInfo {
                render_pass: state.render_pass.clone(),
                framebuffer: framebuffer.clone(),
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D { width, height },
                },
                clear_values: vec![
                    crate::rhi::ClearValue::color(0.02, 0.02, 0.03, 1.0),
                    crate::rhi::ClearValue::depth_stencil(1.0, 0),
                ],
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

        for (index, item) in state.items.iter().enumerate() {
            let Some(descriptor_set) = state.descriptor_sets.get(index) else {
                continue;
            };
            // The shadow map and the comparison sampler are in every set: they
            // are the same for every item.
            let _ = device.write_descriptors(
                descriptor_set,
                &[
                    DescriptorWrite {
                        dst_set: descriptor_set.clone(),
                        dst_binding: 1,
                        dst_array_element: 0,
                        descriptors: vec![DescriptorInfo::Texture(shadow_view.clone(), None)],
                    },
                    DescriptorWrite {
                        dst_set: descriptor_set.clone(),
                        dst_binding: 2,
                        dst_array_element: 0,
                        descriptors: vec![DescriptorInfo::Sampler(shadow_sampler.clone())],
                    },
                ],
            );
            commands.push(Command::BindDescriptorSets {
                pipeline: state.pipeline.clone(),
                first_set: 0,
                sets: vec![descriptor_set.clone()],
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
                        crate::warn!("forward", "{}: 8-bit indices are not supported", item.name);
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
                crate::error!("forward", "a command was rejected: the encoder is not recording");
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

/// The texture behind a view, for the barrier that names it.
fn shadow_texture_of(view: &crate::rhi::TextureView) -> Option<crate::rhi::Texture> {
    Some(view.texture().clone())
}
