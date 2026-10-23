//! Forward Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Draws the scene lit: the geometry, the lights, and the shadow map. It is the
//! forward path, and it is the consumer of the shadow map — without it the
//! shadow pass writes a picture nobody looks at.
//!
//! The lights come from the **clustering pass's own buffer**. That is the
//! whole point of the clustering: the clustering pass bins the lights into
//! the screen's clusters, and the indices it writes are indices into *that*
//! buffer. A forward pass with its own light buffer would have indices that
//! point at other lights, and the picture would be lit by whatever happened to
//! be at those places.
//!
//! The shadow is a **cascade**: a directional light's shadow is cut into
//! several maps by the distance from the camera, and the fragment picks the
//! map that covers its depth. The near cascade is detailed and the far one is
//! coarse, which is the whole reason a cascade exists.
//!
//! Two uniform blocks, because the two have different lifetimes: the camera
//! and the item's model matrix change per item, and the cascades and the grid
//! change per frame.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::{Mat4, Vec3};

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::passes::base::BaseRenderPass;
use crate::render::passes::cluster::{ClusterConfig, MAX_LIGHTS_PER_CLUSTER};
use crate::render::passes::depth_target::DepthTarget;
use crate::render::passes::geometry::GeometryItem;
use crate::render::scene::Scene;
use crate::rhi::{
    Command, CommandEncoder, DescriptorInfo, DescriptorSet, DescriptorSetLayout, DescriptorWrite,
    Framebuffer, FramebufferAttachment, FramebufferDesc, GraphicsPipeline, Offset2D, Rect2D,
};

/// The per-item uniform block, matching `forward.wgsl`'s `ItemUniforms`:
/// `mat4 view_projection` + `mat4 model` = 128 bytes.
const ITEM_UNIFORM_SIZE: u64 = 16 * 4 * 2;

/// The per-frame uniform block, matching `forward.wgsl`'s `FrameUniforms`:
/// `mat4 light_space[4]` + five `vec4` = 256 + 80 = 336 bytes.
///
/// The array of matrices is four `mat4`s, and a uniform array of matrices has
/// a stride of 64 bytes — the same as the matrix itself, so the offsets below
/// are the offsets in the shader.
const FRAME_UNIFORM_SIZE: u64 = 16 * 4 * 4 + 4 * 4 * 5;

/// The number of cascades the shader knows about.
///
/// The shader has four shadow map bindings, so it can name four cascades. The
/// light may have fewer: the count comes in the frame's uniform, and the
/// shader clamps to it, so a light with two cascades never sends a fragment to
/// a map that was never rendered.
pub const MAX_CASCADES: usize = 4;

/// Everything the pass needs to record, shared with the caller.
pub struct ForwardPassState {
    pub pipeline: GraphicsPipeline,
    pub descriptor_layout: DescriptorSetLayout,
    /// One descriptor set per item, in the same order as `items`.
    pub descriptor_sets: Vec<DescriptorSet>,
    /// One uniform buffer per item, in the same order as `items`.
    pub uniform_buffers: Vec<crate::rhi::Buffer>,
    /// The frame's uniform: the cascades, the grid, the ambient term.
    pub frame_uniform: crate::rhi::Buffer,
    pub items: Vec<GeometryItem>,
    pub render_pass: crate::rhi::RenderPass,
    /// The shadow maps the pass reads.
    ///
    /// The fragment picks the cascade by its depth, and the map has to be the
    /// one the shadow pass rendered into for that cascade — the two agree
    /// because they use the same matrix.
    pub shadow_views: Vec<crate::rhi::TextureView>,
    /// The comparison sampler the shadow maps are read with.
    ///
    /// A depth image may only be filtered by a comparison sampler, and the RHI
    /// refuses anything else (§683).
    pub shadow_sampler: Option<crate::rhi::Sampler>,
    /// The lights, from the clustering pass's buffer.
    pub lights_buffer: Option<crate::rhi::Buffer>,
    /// The cluster indices, from the clustering pass's buffer.
    pub cluster_indices_buffer: Option<crate::rhi::Buffer>,
    /// The grid the clusters are cut into. It has to be the clustering pass's
    /// own grid: the fragment computes the cluster's number from it, and the
    /// clustering pass wrote the light's index under that number.
    pub cluster_config: ClusterConfig,
    /// The light that casts the shadows: its index in the lights buffer.
    ///
    /// The other lights have no map, and a light with no map is not in shadow.
    pub shadow_light_index: u32,
    /// The camera's near and far planes.
    ///
    /// The cluster's slices and the cascades are cut in the distance between
    /// them, so the shader needs both to turn a depth into a slice.
    pub near: f32,
    pub far: f32,
    pub framebuffer: Option<Framebuffer>,
    pub framebuffer_pinned: bool,
    pub framebuffer_image: Option<u32>,
    pub framebuffer_extent: Option<(u32, u32)>,
    pub framebuffer_generation: Option<u64>,
    /// The generation of the shared depth the framebuffer was built with.
    pub framebuffer_depth_generation: Option<u64>,
    /// The depth the geometry is tested against, and the one the transparent
    /// and the water passes are tested against.
    ///
    /// It is the frame's shared depth, not the pass's own: the transparent
    /// objects and the water have to be in front of the *opaque* geometry, and
    /// a depth of their own would be a depth of nothing.
    pub depth: DepthTarget,
}
/// Shared handle to the pass state.
pub type SharedState = Arc<Mutex<ForwardPassState>>;

/// The light the pass lights the scene with.
///
/// The light's numbers live in the clustering pass's buffer, so this is only
/// the description of the light — what the caller writes into that buffer and
/// what the shadow's matrix is computed from.
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
        // The clusters are produced by the clustering pass, and the pass reads
        // the light's index out of them.
        base.add_dependency(
            "cluster",
            "clusters",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
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
        base.add_dependency(
            "cluster",
            "clusters",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
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

        // The descriptor layout: the item's uniform, the frame's uniform, the
        // four cascade maps, the comparison sampler, the lights and the
        // cluster indices.
        let descriptor_layout = device.create_descriptor_set_layout(
            &crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
                bindings: vec![
                    // 0: the item's uniform.
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 0,
                        ty: crate::rhi::DescriptorType::UniformBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::VERTEX | crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    // 1: the frame's uniform.
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 1,
                        ty: crate::rhi::DescriptorType::UniformBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::VERTEX | crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    // 2..=5: the cascade maps.
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 2,
                        ty: crate::rhi::DescriptorType::SampledTexture,
                        count: 1,
                        stages: crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 3,
                        ty: crate::rhi::DescriptorType::SampledTexture,
                        count: 1,
                        stages: crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 4,
                        ty: crate::rhi::DescriptorType::SampledTexture,
                        count: 1,
                        stages: crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 5,
                        ty: crate::rhi::DescriptorType::SampledTexture,
                        count: 1,
                        stages: crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    // 6: the comparison sampler.
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 6,
                        ty: crate::rhi::DescriptorType::Sampler,
                        count: 1,
                        stages: crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    // 7: the lights.
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 7,
                        ty: crate::rhi::DescriptorType::StorageBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::FRAGMENT,
                        immutable_samplers: None,
                    },
                    // 8: the cluster indices.
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 8,
                        ty: crate::rhi::DescriptorType::StorageBuffer,
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

        let frame_uniform = device.create_buffer(
            FRAME_UNIFORM_SIZE,
            crate::rhi::BufferUsage::UNIFORM,
            true,
        );

        let mut uniform_buffers = Vec::with_capacity(items.len());
        let mut descriptor_sets = Vec::with_capacity(items.len());
        for _ in 0..items.len() {
            let uniform = device.create_buffer(ITEM_UNIFORM_SIZE, crate::rhi::BufferUsage::UNIFORM, true);
            let set = device.create_descriptor_set(&descriptor_layout)?;
            device.write_descriptors(
                &set,
                &[
                    DescriptorWrite {
                        dst_set: set.clone(),
                        dst_binding: 0,
                        dst_array_element: 0,
                        descriptors: vec![DescriptorInfo::Buffer(uniform.clone(), 0, ITEM_UNIFORM_SIZE)],
                    },
                    DescriptorWrite {
                        dst_set: set.clone(),
                        dst_binding: 1,
                        dst_array_element: 0,
                        descriptors: vec![DescriptorInfo::Buffer(
                            frame_uniform.clone(),
                            0,
                            FRAME_UNIFORM_SIZE,
                        )],
                    },
                ],
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
            frame_uniform,
            items,
            render_pass,
            shadow_views: Vec::new(),
            shadow_sampler: Some(shadow_sampler),
            lights_buffer: None,
            cluster_indices_buffer: None,
            cluster_config: ClusterConfig::default(),
            shadow_light_index: 0,
            near: 0.1,
            far: 100.0,
            framebuffer: None,
            framebuffer_pinned: false,
            framebuffer_image: None,
            framebuffer_extent: None,
            framebuffer_generation: None,
            framebuffer_depth_generation: None,
            depth: DepthTarget::new(),
        }))
    }

    /// The shadow maps the pass reads, one per cascade.
    ///
    /// The maps have to be the ones the shadow pass rendered into, in the same
    /// order: the fragment picks the cascade by its depth and the map has to
    /// be the one that covers that depth.
    pub fn set_shadow_maps(state: &SharedState, views: Vec<crate::rhi::TextureView>) {
        let Ok(mut state) = state.lock() else {
            return;
        };
        state.shadow_views = views;
    }

    /// The clustering pass's buffers: the lights and the cluster indices.
    ///
    /// The forward pass reads the light's numbers out of the **same** buffer
    /// the clustering binned from, because the indices in the cluster are
    /// indices into that buffer. A different buffer would give a picture lit by
    /// whatever happens to be at those places.
    pub fn set_cluster_buffers(
        state: &SharedState,
        lights: crate::rhi::Buffer,
        indices: crate::rhi::Buffer,
        config: ClusterConfig,
    ) {
        let Ok(mut state) = state.lock() else {
            return;
        };
        state.lights_buffer = Some(lights);
        state.cluster_indices_buffer = Some(indices);
        state.cluster_config = config;
    }

    /// Pin the framebuffer the scene is drawn into.
    pub fn set_framebuffer(state: &SharedState, framebuffer: Option<Framebuffer>) {
        if let Ok(mut state) = state.lock() {
            let pinned = framebuffer.is_some();
            state.framebuffer = framebuffer;
            state.framebuffer_pinned = pinned;
        }
    }

    /// The frame's shared depth: the one the opaque geometry's depth goes
    /// into, and the one the transparent objects and the water are tested
    /// against.
    ///
    /// The caller creates it and hands the same handle to every pass that
    /// needs it, because the passes cannot find each other's depth: the
    /// forward pass writes it, and the transparent and the water passes read
    /// it, and a pass that read a *different* depth would be testing against
    /// nothing.
    pub fn set_depth_target(state: &SharedState, depth: DepthTarget) {
        if let Ok(mut state) = state.lock() {
            state.depth = depth;
        }
    }

    /// The camera's matrices, the cascades and the ambient term.
    ///
    /// The `light_space` matrices are the shadow pass's own: the fragment
    /// turns its world position into the light's space with them, and the
    /// shadow map holds the depth in that space, so the two have to be the
    /// same matrices.
    ///
    /// The `splits` are the distances at which the cascades end, and the
    /// `cascade_count` is how many of them there are.
    #[allow(clippy::too_many_arguments)]
    pub fn update_uniforms(
        state: &SharedState,
        device: &crate::rhi::Device,
        view: Mat4,
        projection: Mat4,
        light_space: &[Mat4],
        splits: &[f32],
        cascade_count: usize,
        shadow_light_index: u32,
        near: f32,
        far: f32,
        ambient: Vec3,
    ) -> bool {
        let Ok(state) = state.lock() else {
            return false;
        };
        let view_projection = projection * view;

        // The frame's uniform: the cascades, the grid, the ambient term.
        //
        // The floats and the integers share the buffer, and the integers are
        // written as the *bits* of a float: writing `1.0` puts 0x3F800000
        // there, and the shader would read 1065353216.
        let mut frame = [0u32; (FRAME_UNIFORM_SIZE / 4) as usize];
        for (index, matrix) in light_space.iter().take(MAX_CASCADES).enumerate() {
            let start = index * 16;
            for (offset, value) in matrix.to_cols_array().iter().enumerate() {
                frame[start + offset] = value.to_bits();
            }
        }
        // The grid: the tiles in x and y, the depth slices, the near plane.
        //
        // The offsets are the ones the shader's struct produces: the array of
        // matrices takes 4 * 64 bytes, and each `vec4` after it takes 16.
        let grid = [
            state.cluster_config.tiles_x as f32,
            state.cluster_config.tiles_y as f32,
            state.cluster_config.depth_slices as f32,
            near,
        ];
        for (offset, value) in grid.iter().enumerate() {
            frame[64 + offset] = value.to_bits();
        }
        // The planes: the far plane.
        let planes = [far, 0.0, 0.0, 0.0];
        for (offset, value) in planes.iter().enumerate() {
            frame[68 + offset] = value.to_bits();
        }
        // The splits: the distances at which the cascades end.
        for (offset, value) in splits.iter().take(4).enumerate() {
            frame[72 + offset] = value.to_bits();
        }
        // The ambient term.
        let ambient = [ambient.x, ambient.y, ambient.z, 1.0];
        for (offset, value) in ambient.iter().enumerate() {
            frame[76 + offset] = value.to_bits();
        }
        // The control: the light that casts the shadows, and the count again.
        frame[80] = shadow_light_index;
        frame[81] = cascade_count as u32;

        let mut ok = true;
        if device.upload_buffer(&state.frame_uniform, &frame).is_err() {
            ok = false;
        }

        for (index, item) in state.items.iter().enumerate() {
            let Some(buffer) = state.uniform_buffers.get(index) else {
                continue;
            };
            let mut data = [0u32; (ITEM_UNIFORM_SIZE / 4) as usize];
            for (offset, value) in view_projection.to_cols_array().iter().enumerate() {
                data[offset] = value.to_bits();
            }
            for (offset, value) in item.model.to_cols_array().iter().enumerate() {
                data[16 + offset] = value.to_bits();
            }
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
    // The depth attachment is the frame's shared depth, and it is rebuilt
    // when the size changes: a framebuffer's attachments must all be the size
    // of the render area, and the old framebuffer holds the old depth's view.
    let Some(depth_view) = state.depth.view(device, width, height) else {
        return None;
    };
    // The framebuffer is rebuilt when the depth was rebuilt: the old one holds
    // the old depth's view, and a framebuffer's attachments must all be the
    // size of the render area.
    let depth_generation = state.depth.generation();
    if state.framebuffer_depth_generation != Some(depth_generation) {
        state.framebuffer = None;
        state.framebuffer_depth_generation = Some(depth_generation);
    }
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
        // shadow maps and the clustering pass's buffers are set by the caller,
        // because only the caller knows which passes produced them.
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
        // Without the shadow maps there is nothing to compare the fragment's
        // depth against, and the shadow map is the point of the pass.
        if state.shadow_views.is_empty() {
            return PassOutcome::skipped("no shadow map: the shadow pass has to run first");
        }
        // Without the clustering pass's buffers there are no lights to look
        // at: the indices in the cluster are indices into the lights buffer,
        // and without both of them the fragment has nothing to read.
        let Some(lights_buffer) = state.lights_buffer.clone() else {
            return PassOutcome::skipped("no lights: the clustering pass has to run first");
        };
        let Some(cluster_indices_buffer) = state.cluster_indices_buffer.clone() else {
            return PassOutcome::skipped("no clusters: the clustering pass has to run first");
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

        // The barrier before the shadow maps are read.
        //
        // The graph orders the passes, and an order is not a memory dependency:
        // without this the shader compares the fragment's depth against a map
        // that is still being written, and the shadow moves with the frame.
        let mut texture_barriers = Vec::new();
        for view in state.shadow_views.iter() {
            let texture = view.texture().clone();
            texture_barriers.push(crate::rhi::sync::barrier::TextureBarrier {
                texture,
                src_access: crate::rhi::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                dst_access: crate::rhi::AccessFlags::SHADER_READ,
                old_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
                new_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
            });
        }
        if !texture_barriers.is_empty() {
            commands.push(Command::PipelineBarrier {
                src_stage: crate::rhi::PipelineStage::LATE_FRAGMENT_TESTS,
                dst_stage: crate::rhi::PipelineStage::FRAGMENT_SHADER,
                memory_barriers: vec![],
                buffer_barriers: vec![],
                texture_barriers,
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
            // The shadow maps, the sampler, the lights and the cluster indices
            // are the same for every item, and they are written into every
            // set: a descriptor set is per item, so a set that does not have
            // them is not the set the shader reads.
            let mut writes = Vec::with_capacity(4 + MAX_CASCADES);
            // All four cascade slots are written, even when the light has
            // fewer cascades: the shader names all four maps, and a descriptor
            // the shader uses but nobody wrote is a validation error and an
            // undefined read.
            //
            // The extra slots get the **first** map, and the shader never
            // samples them: the cascade index is clamped to the cascade count,
            // so a light with two cascades cannot send a fragment to the third
            // map.
            let first_shadow = state.shadow_views.first().cloned();
            for cascade in 0..MAX_CASCADES {
                let Some(view) = state
                    .shadow_views
                    .get(cascade)
                    .cloned()
                    .or_else(|| first_shadow.clone())
                else {
                    continue;
                };
                writes.push(DescriptorWrite {
                    dst_set: descriptor_set.clone(),
                    dst_binding: 2 + cascade as u32,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Texture(view, None)],
                });
            }
            writes.push(DescriptorWrite {
                dst_set: descriptor_set.clone(),
                dst_binding: 6,
                dst_array_element: 0,
                descriptors: vec![DescriptorInfo::Sampler(shadow_sampler.clone())],
            });
            writes.push(DescriptorWrite {
                dst_set: descriptor_set.clone(),
                dst_binding: 7,
                dst_array_element: 0,
                descriptors: vec![DescriptorInfo::Buffer(
                    lights_buffer.clone(),
                    0,
                    lights_buffer.size(),
                )],
            });
            writes.push(DescriptorWrite {
                dst_set: descriptor_set.clone(),
                dst_binding: 8,
                dst_array_element: 0,
                descriptors: vec![DescriptorInfo::Buffer(
                    cluster_indices_buffer.clone(),
                    0,
                    cluster_indices_buffer.size(),
                )],
            });
            let _ = device.write_descriptors(descriptor_set, &writes);
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
        // The depth now holds the opaque geometry's, and the transparent and
        // the water passes are tested against it.
        state.depth.mark_written();
        PassOutcome::recorded()
    }

    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }
}

/// The number of slots a cluster holds, as the shader knows it.
///
/// The shader walks the slots of a cluster and skips the marked ones, so the
/// number has to be the same on both sides: a shader that walked more slots
/// than the buffer had would read past its end.
pub const SLOTS_PER_CLUSTER: u32 = MAX_LIGHTS_PER_CLUSTER as u32;
