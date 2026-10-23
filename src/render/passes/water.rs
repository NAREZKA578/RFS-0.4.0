//! Water Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Draws the water surface: a mesh displaced by waves, with the reflection
//! and the refraction mixed by the Fresnel term, and a normal map that breaks
//! up the surface.
//!
//! What this pass used to be: a `water_pipeline` that was `None` forever, a
//! normal map and a foam texture that were `None` forever, texture views
//! built without the device - so with no native handle - and an `execute`
//! whose drawing loop was a list of the things it would do "for now, just a
//! placeholder". It opened a render pass, drew nothing, and reported that it
//! had recorded work.
//!
//! The reflection and the refraction are **inputs**: the scene seen from the
//! mirrored camera, and the scene under the water. Who renders them is the
//! caller's business - a pass cannot render the scene, it can only draw what
//! it is given - so they are set on the pass, and the pass refuses to draw a
//! surface it cannot shade honestly.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::{Mat4, Vec3};

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::passes::base::BaseRenderPass;
use crate::render::passes::depth_target::DepthTarget;
use crate::render::passes::geometry::GeometryItem;
use crate::render::scene::Scene;
use crate::rhi::{
    Command, CommandEncoder, DescriptorInfo, DescriptorSet, DescriptorSetLayout, DescriptorWrite,
    Framebuffer, FramebufferAttachment, FramebufferDesc, GraphicsPipeline, Offset2D, Rect2D,
};

/// The uniform block, matching `water.wgsl`:
/// `mat4 view_proj` + `mat4 model` + `time` + `wave_scale` + `wave_speed` +
/// `wave_height` = 144 bytes.
const UNIFORM_SIZE: u64 = 16 * 4 * 2 + 4 * 4;

/// The size of the generated normal map.
const NORMAL_MAP_SIZE: u32 = 64;

/// Water configuration.
#[derive(Debug, Clone)]
pub struct WaterConfig {
    /// The wave method the shader implements.
    pub wave_method: WaveMethod,
    /// How fast the waves travel, and how far the wave pattern repeats.
    pub wave_scale: f32,
    pub wave_speed: f32,
    /// How high the waves are.
    pub wave_height: f32,
    /// The quality of the reflection.
    pub reflection_quality: ReflectionQuality,
    /// Whether the refraction is mixed in at all.
    pub refraction_enabled: bool,
    /// Whether the foam is drawn.
    pub foam_enabled: bool,
    /// The tessellation factor.
    pub tessellation_factor: f32,
    /// The water's colour.
    pub color: [f32; 4],
    /// The depth of the water, for the refraction.
    pub depth: f32,
    /// How clear the water is.
    pub clarity: f32,
}

impl Default for WaterConfig {
    fn default() -> Self {
        Self {
            wave_method: WaveMethod::Gerstner,
            wave_scale: 0.1,
            wave_speed: 0.5,
            wave_height: 0.2,
            reflection_quality: ReflectionQuality::Medium,
            refraction_enabled: true,
            foam_enabled: true,
            tessellation_factor: 8.0,
            color: [0.0, 0.1, 0.3, 0.8],
            depth: 100.0,
            clarity: 0.7,
        }
    }
}

/// The wave method the shader implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveMethod {
    /// No waves: a flat water.
    Flat,
    /// Simple sine waves.
    Simple,
    /// Gerstner waves.
    Gerstner,
    /// FFT-based wave simulation.
    FFT,
}

/// The quality of the reflection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionQuality {
    /// No reflections.
    None,
    /// Low quality (512x512).
    Low,
    /// Medium quality (1024x1024).
    Medium,
    /// High quality (2048x2048).
    High,
    /// Ultra quality (4096x4096 or ray traced).
    Ultra,
}

impl ReflectionQuality {
    /// The size of the reflection image, or zero when there is none.
    pub fn resolution(self) -> u32 {
        match self {
            ReflectionQuality::None => 0,
            ReflectionQuality::Low => 512,
            ReflectionQuality::Medium => 1024,
            ReflectionQuality::High => 2048,
            ReflectionQuality::Ultra => 4096,
        }
    }
}

/// Everything the pass needs to record, shared with the caller.
pub struct WaterPassState {
    pub pipeline: GraphicsPipeline,
    /// The pipeline for the shared-depth mode.
    ///
    /// A second one, because a pipeline belongs to a render pass and the two
    /// modes are two render passes: the standalone one clears the colour and
    /// the depth and writes the depth, and the shared one loads both and
    /// writes neither.
    pub shared_pipeline: Option<GraphicsPipeline>,
    pub descriptor_layout: DescriptorSetLayout,
    /// One descriptor set per item.
    pub descriptor_sets: Vec<DescriptorSet>,
    /// One uniform buffer per item.
    pub uniform_buffers: Vec<crate::rhi::Buffer>,
    pub items: Vec<GeometryItem>,
    pub render_pass: crate::rhi::RenderPass,
    /// The render pass for the shared-depth mode.
    pub shared_render_pass: Option<crate::rhi::RenderPass>,
    /// Where the water is drawn: the pass's own image.
    pub output_texture: Option<crate::rhi::Texture>,
    pub output_view: Option<crate::rhi::TextureView>,
    pub depth_texture: Option<crate::rhi::Texture>,
    pub depth_view: Option<crate::rhi::TextureView>,
    /// The frame's shared depth, when the caller gave one.
    ///
    /// The water is then tested against the **opaque** geometry's depth,
    /// which is the whole point: a water surface behind a wall must not be
    /// drawn over the wall.
    pub depth_target: Option<DepthTarget>,
    /// The wave normals, generated once.
    pub normal_map: Option<crate::rhi::Texture>,
    pub normal_map_view: Option<crate::rhi::TextureView>,
    /// The scene seen from the mirrored camera.
    pub reflection_view: Option<crate::rhi::TextureView>,
    /// The scene seen through the water.
    pub refraction_view: Option<crate::rhi::TextureView>,
    pub config: WaterConfig,
    /// The framebuffer the water is drawn into, in the standalone mode.
    pub framebuffer: Option<Framebuffer>,
    pub framebuffer_pinned: bool,
    /// The framebuffer the water is drawn into, in the shared-depth mode.
    ///
    /// A separate one: the standalone framebuffer holds the water's own
    /// image and the water's own depth, and the shared one holds the image
    /// the renderer acquired and the frame's shared depth. They are different
    /// pictures, and a framebuffer's attachments must all be the size of the
    /// render area — so they cannot be the same framebuffer.
    pub shared_framebuffer: Option<Framebuffer>,
    /// The generation of the shared depth the shared framebuffer was built
    /// with.
    pub shared_framebuffer_depth_generation: Option<u64>,
}

/// Shared handle to the pass state.
pub type SharedState = Arc<Mutex<WaterPassState>>;

/// The water pass.
pub struct WaterPass {
    base: BaseRenderPass,
    config: WaterConfig,
    state: Option<SharedState>,
}

impl WaterPass {
    /// Build a pass with no GPU objects yet.
    pub fn new(config: WaterConfig) -> Self {
        let mut base = BaseRenderPass::new("water");
        // The water is drawn over the picture the opaque pass produced, so the
        // pass comes after it, and the depth it is tested against is the
        // opaque pass's depth.
        //
        // The dependency names the pass that writes the depth, because the
        // graph orders by that name. The resource name is not in the graph's
        // catalogue, so nothing is created for it — the ordering is the point.
        base.add_dependency(
            "forward",
            "shared_depth",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
        base.add_output("water");
        Self {
            base,
            config,
            state: None,
        }
    }

    /// Build a pass around a new shared state and return both.
    pub fn with_state(state: WaterPassState) -> (Self, SharedState) {
        let mut base = BaseRenderPass::new("water");
        base.add_dependency(
            "forward",
            "shared_depth",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
        base.add_output("water");
        let config = state.config.clone();
        let shared: SharedState = Arc::new(Mutex::new(state));
        let pass = Self {
            base,
            config,
            state: Some(shared.clone()),
        };
        (pass, shared)
    }

    /// The configuration the pass was built with.
    pub fn config(&self) -> &WaterConfig {
        &self.config
    }

    /// Build a water pass with real GPU objects.
    ///
    /// `items` are the scene's water surfaces — the same meshes the geometry
    /// pass draws, cloned — because the water is geometry.
    pub fn create(
        device: &crate::rhi::Device,
        items: Vec<GeometryItem>,
        config: WaterConfig,
        color_format: crate::rhi::Format,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::PrimitiveTopology;
        use crate::rhi::{
            ColorBlendAttachment, ColorBlendState, ColorComponentFlags,
            DepthStencilState, GraphicsPipelineDesc, InputAssemblyState, MultisampleState,
            PipelineShaderStage, RasterizerState, ShaderFormat, ShaderModuleDesc,
        };

        let resolution = config.reflection_quality.resolution().max(256);
        let mut config = config;
        config.reflection_quality = ReflectionQuality::Low;

        // The image the water is drawn into, and the depth it is tested
        // against.
        let output_texture = device
            .create_texture(
                resolution,
                resolution,
                1,
                color_format,
                crate::rhi::TextureUsage::COLOR_ATTACHMENT
                    | crate::rhi::TextureUsage::SAMPLED
                    | crate::rhi::TextureUsage::TRANSFER_SRC,
                1,
            )
            .map_err(|e| RhiError::BackendError(format!("the water's output texture: {e}")))?;
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
            .map_err(|e| RhiError::BackendError(format!("the water's output view: {e}")))?;

        let depth_texture = device
            .create_texture(
                resolution,
                resolution,
                1,
                crate::rhi::Format::D32_SFLOAT,
                crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT,
                1,
            )
            .map_err(|e| RhiError::BackendError(format!("the water's depth texture: {e}")))?;
        let depth_view = device
            .create_texture_view(
                &depth_texture,
                &crate::rhi::TextureViewDesc {
                    texture: depth_texture.clone(),
                    format: None,
                    view_type: crate::rhi::TextureViewType::D2,
                    aspects: crate::rhi::TextureAspectFlags::DEPTH,
                    base_mip_level: 0,
                    mip_level_count: 1,
                    base_array_layer: 0,
                    array_layer_count: 1,
                },
            )
            .map_err(|e| RhiError::BackendError(format!("the water's depth view: {e}")))?;

        // The wave normals, generated from the same height field the vertex
        // stage displaces the surface by, so the normals agree with the waves.
        let (normal_map, normal_map_view) = create_normal_map(device, &config)?;

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
            .map_err(|e| RhiError::BackendError(format!("the water render pass: {e}")))?;

        // The shared-depth render pass.
        //
        // - the colour is **loaded**, not cleared: the opaque picture is
        //   already in it, and clearing it would throw the picture away and
        //   draw the water over black;
        // - the depth is **loaded** too, and the water does not write it: the
        //   water is tested against the opaque geometry's depth, and a water
        //   surface that wrote depth would hide the transparent objects
        //   behind it, and the depth would no longer be the opaque
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
                RhiError::BackendError(format!("the water shared-depth render pass: {e}"))
            })?;

        let framebuffer = device
            .create_gpu_framebuffer(&FramebufferDesc {
                render_pass: render_pass.clone(),
                attachments: vec![
                    FramebufferAttachment {
                        texture_view: output_view.clone(),
                        layer: 0,
                        mip_level: 0,
                    },
                    FramebufferAttachment {
                        texture_view: depth_view.clone(),
                        layer: 0,
                        mip_level: 0,
                    },
                ],
                width: resolution,
                height: resolution,
                layers: 1,
            })
            .map_err(|e| RhiError::BackendError(format!("the water framebuffer: {e}")))?;

        // The descriptor layout: the uniform and the three textures.
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
                ],
            },
        )
        .map_err(|e| RhiError::BackendError(format!("the water descriptor layout: {e}")))?;

        let mut uniform_buffers = Vec::with_capacity(items.len());
        let mut descriptor_sets = Vec::with_capacity(items.len());
        for _ in 0..items.len() {
            let uniform = device
                .create_buffer(UNIFORM_SIZE, crate::rhi::BufferUsage::UNIFORM, true)
                .map_err(|e| RhiError::BackendError(format!("the water uniform buffer: {e}")))?;
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
                name: Some("water".into()),
            })
            .map_err(|e| RhiError::BackendError(format!("the water shader module: {e}")))?;

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
            // The water is translucent: the shader writes an alpha of 0.85, and
            // the blend is what shows the scene through it.
            color_blend_state: Some(ColorBlendState {
                attachments: vec![ColorBlendAttachment {
                    blend_enable: true,
                    src_color_blend_factor: crate::rhi::pipeline::graphics::BlendFactor::SrcAlpha,
                    dst_color_blend_factor: crate::rhi::pipeline::graphics::BlendFactor::OneMinusSrcAlpha,
                    color_blend_op: crate::rhi::pipeline::graphics::BlendOp::Add,
                    src_alpha_blend_factor: crate::rhi::pipeline::graphics::BlendFactor::One,
                    dst_alpha_blend_factor: crate::rhi::pipeline::graphics::BlendFactor::OneMinusSrcAlpha,
                    alpha_blend_op: crate::rhi::pipeline::graphics::BlendOp::Add,
                    color_write_mask: ColorComponentFlags::R
                        | ColorComponentFlags::G
                        | ColorComponentFlags::B
                        | ColorComponentFlags::A,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        desc.depth_stencil_state = Some(DepthStencilState::less_equal());
        let pipeline = device.create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])?;
        if !pipeline.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the water graphics pipeline was not created on the device".into(),
            ));
        }

        // The shared-depth pipeline: the depth is tested against the opaque
        // geometry's and not written. A second one, because a pipeline belongs
        // to a render pass, and the shared-depth mode is a different render
        // pass with a different depth state.
        desc.depth_stencil_state = Some(DepthStencilState::read_only());
        let shared_pipeline = device
            .create_gpu_graphics_pipeline(&desc, &shared_render_pass, &[&descriptor_layout])
            .map_err(|e| RhiError::BackendError(format!("the water shared-depth pipeline: {e}")))?;
        if !shared_pipeline.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the water shared-depth graphics pipeline was not created on the device".into(),
            ));
        }

        Ok(Self::with_state(WaterPassState {
            pipeline,
            shared_pipeline: Some(shared_pipeline),
            descriptor_layout,
            descriptor_sets,
            uniform_buffers,
            items,
            render_pass,
            shared_render_pass: Some(shared_render_pass),
            output_texture: Some(output_texture),
            output_view: Some(output_view),
            depth_texture: Some(depth_texture),
            depth_view: Some(depth_view),
            depth_target: None,
            normal_map: Some(normal_map),
            normal_map_view: Some(normal_map_view),
            reflection_view: None,
            refraction_view: None,
            config,
            framebuffer: Some(framebuffer),
            framebuffer_pinned: false,
            shared_framebuffer: None,
            shared_framebuffer_depth_generation: None,
        }))
    }

    /// The frame's shared depth.
    ///
    /// The water is then tested against the **opaque** geometry's depth, which
    /// is the whole point: a water surface behind a wall must not be drawn
    /// over the wall. A depth of its own is a depth of nothing — the pass
    /// would clear it to the far plane, and the water would be in front of
    /// everything.
    pub fn set_depth_target(state: &SharedState, depth: DepthTarget) {
        if let Ok(mut state) = state.lock() {
            state.depth_target = Some(depth);
        }
    }

    /// The scene seen from the mirrored camera, and the scene under the water.
    ///
    /// Both are inputs: who renders them is the caller's business, and the
    /// pass refuses to draw without a reflection rather than shading the water
    /// with whatever the driver left in an unbound texture.
    pub fn set_textures(
        state: &SharedState,
        device: &crate::rhi::Device,
        reflection: Option<crate::rhi::TextureView>,
        refraction: Option<crate::rhi::TextureView>,
    ) {
        let Ok(mut state) = state.lock() else {
            return;
        };
        state.reflection_view = reflection.clone();
        state.refraction_view = refraction.clone();
        let normal_map = state.normal_map_view.clone();
        for set in state.descriptor_sets.iter() {
            for (binding, texture) in [
                (1u32, reflection.clone()),
                (2, refraction.clone().or_else(|| reflection.clone())),
                (3, normal_map.clone()),
            ] {
                let Some(texture) = texture else {
                    continue;
                };
                let _ = device.write_descriptors(
                    set,
                    &[DescriptorWrite {
                        dst_set: set.clone(),
                        dst_binding: binding,
                        dst_array_element: 0,
                        descriptors: vec![DescriptorInfo::Texture(texture, None)],
                    }],
                );
            }
        }
    }

    /// Pin the framebuffer the water is drawn into.
    pub fn set_framebuffer(state: &SharedState, framebuffer: Option<Framebuffer>) {
        if let Ok(mut state) = state.lock() {
            let pinned = framebuffer.is_some();
            state.framebuffer = framebuffer;
            state.framebuffer_pinned = pinned;
        }
    }

    /// The camera matrices and the wave parameters.
    pub fn update_uniforms(
        state: &SharedState,
        device: &crate::rhi::Device,
        view: Mat4,
        projection: Mat4,
        time: f32,
    ) -> bool {
        let Ok(state) = state.lock() else {
            return false;
        };
        let config = state.config.clone();
        let view_proj = projection * view;
        let mut ok = true;
        for (index, item) in state.items.iter().enumerate() {
            let Some(buffer) = state.uniform_buffers.get(index) else {
                continue;
            };
            let mut data = [0f32; 36];
            data[..16].copy_from_slice(&view_proj.to_cols_array());
            data[16..32].copy_from_slice(&item.model.to_cols_array());
            data[32] = time;
            data[33] = config.wave_scale;
            data[34] = config.wave_speed;
            data[35] = config.wave_height;
            if device.upload_buffer(buffer, &data).is_err() {
                ok = false;
            }
        }
        ok
    }
}

/// The generated wave normals.
///
/// The normals come from the same height field the vertex stage displaces the
/// surface by, so they agree with the waves. A noise texture would not.
fn create_normal_map(
    device: &crate::rhi::Device,
    config: &WaterConfig,
) -> crate::rhi::error::RhiResult<(crate::rhi::Texture, crate::rhi::TextureView)> {
    let size = NORMAL_MAP_SIZE as usize;
    let mut pixels = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;
            let height = |u: f32, v: f32| {
                (u * config.wave_scale * 16.0).sin() * config.wave_height
                    + (v * config.wave_scale * 27.2).sin() * config.wave_height * 0.6
            };
            let dx = (height(u + 0.01, v) - height(u - 0.01, v)) / 0.02;
            let dy = (height(u, v + 0.01) - height(u, v - 0.01)) / 0.02;
            let normal = Vec3::new(-dx, 1.0, -dy).normalize();
            let offset = (y * size + x) * 4;
            pixels[offset] = ((normal.x * 0.5 + 0.5) * 255.0) as u8;
            pixels[offset + 1] = ((normal.y * 0.5 + 0.5) * 255.0) as u8;
            pixels[offset + 2] = ((normal.z * 0.5 + 0.5) * 255.0) as u8;
            pixels[offset + 3] = 255;
        }
    }
    let texture = device
        .create_texture(
            NORMAL_MAP_SIZE,
            NORMAL_MAP_SIZE,
            1,
            crate::rhi::Format::RGBA8_UNORM,
            crate::rhi::TextureUsage::SAMPLED | crate::rhi::TextureUsage::TRANSFER_DST,
            1,
        )
        .map_err(|e| crate::rhi::error::RhiError::BackendError(format!("the water's normal map texture: {e}")))?;
    device.upload_texture(&texture, &pixels).map_err(|e| {
        crate::rhi::error::RhiError::BackendError(format!("the water's normal map: {e}"))
    })?;
    let view = device
        .create_texture_view(
            &texture,
            &crate::rhi::TextureViewDesc {
                texture: texture.clone(),
                format: None,
                view_type: crate::rhi::TextureViewType::D2,
                aspects: crate::rhi::TextureAspectFlags::COLOR,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .map_err(|e| {
            crate::rhi::error::RhiError::BackendError(format!("the water's normal map view: {e}"))
        })?;
    Ok((texture, view))
}

/// The framebuffer the water is drawn into in the shared-depth mode.
///
/// The water is drawn into the image the renderer acquired — the same image
/// the opaque pass drew into — and against the frame's shared depth. The
/// water's own image is a different size (it is the reflection's size), and a
/// framebuffer's attachments must all be the size of the render area, so the
/// two cannot be one framebuffer.
///
/// A pinned framebuffer wins, as it does everywhere else: the caller said
/// exactly where to draw, and the framebuffer it pinned was built for the
/// shared-depth render pass.
///
/// The framebuffer is cached and rebuilt when the shared depth is: a
/// framebuffer holds the depth's *view*, and a view of a destroyed image is
/// not a view of anything.
fn resolve_shared_target(
    state: &mut WaterPassState,
    context: &RenderContext,
    device: &crate::rhi::Device,
) -> Option<Framebuffer> {
    if state.framebuffer_pinned {
        return state.framebuffer.clone();
    }
    let target = state.depth_target.clone()?;
    let index = context.swapchain_image_index;
    let view = context
        .presentation_texture_views
        .get(index as usize)?
        .clone();
    let (width, height) = context.drawable_size();
    if width == 0 || height == 0 {
        return None;
    }
    let Some(depth_view) = target.view(device, width, height) else {
        return None;
    };
    // The framebuffer is rebuilt when the depth was: the old one holds the
    // old depth's view.
    let depth_generation = target.generation();
    if state.shared_framebuffer_depth_generation != Some(depth_generation) {
        state.shared_framebuffer = None;
        state.shared_framebuffer_depth_generation = Some(depth_generation);
    }
    if let Some(framebuffer) = state.shared_framebuffer.clone() {
        return Some(framebuffer);
    }
    let render_pass = state.shared_render_pass.clone()?;
    let framebuffer = match device.create_gpu_framebuffer(&FramebufferDesc {
        render_pass,
        attachments: vec![
            FramebufferAttachment {
                texture_view: view,
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
            crate::error!("water", "the framebuffer could not be created: {e}");
            return None;
        }
    };
    state.shared_framebuffer = Some(framebuffer.clone());
    Some(framebuffer)
}

impl RenderPass for WaterPass {
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
        // reflection and the refraction are set by the caller, because only the
        // caller knows what the scene looked like from the mirrored camera.
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
            return PassOutcome::skipped("no water surface to draw");
        }
        let device = context.device().clone();
        // Without a reflection the shader has nothing to mix, and a surface
        // that samples an unbound texture shows whatever the driver left there.
        if state.reflection_view.is_none() {
            return PassOutcome::skipped(
                "no reflection: the scene has to be rendered from the mirrored camera first",
            );
        }
        // The shared-depth mode: the water is drawn over the picture the
        // opaque pass produced, and tested against the opaque geometry's depth.
        //
        // The water's **own** depth would be a depth of nothing: the pass
        // would clear it to the far plane, and the water would be in front of
        // every wall in the scene.
        let shared = state.depth_target.is_some();

        // The target.
        //
        // In the shared mode the water is drawn into the image the renderer
        // acquired — the same image the opaque pass drew into, and the same
        // depth. The water's own image is a different size (it is the
        // reflection's size), and a framebuffer's attachments must all be the
        // size of the render area, so the two cannot share one framebuffer.
        let (framebuffer, render_pass, pipeline, width, height) = if shared {
            let Some(framebuffer) = resolve_shared_target(&mut state, context, device.as_ref())
            else {
                return PassOutcome::skipped("no target for the acquired image");
            };
            let Some(render_pass) = state.shared_render_pass.clone() else {
                return PassOutcome::skipped("the shared-depth render pass is missing");
            };
            let Some(pipeline) = state.shared_pipeline.clone() else {
                return PassOutcome::skipped("the shared-depth pipeline is missing");
            };
            let (width, height) = {
                let desc = framebuffer.desc();
                (desc.width, desc.height)
            };
            (framebuffer, render_pass, pipeline, width, height)
        } else {
            let Some(framebuffer) = state.framebuffer.clone() else {
                return PassOutcome::skipped("no framebuffer");
            };
            let resolution = state
                .output_texture
                .as_ref()
                .map(|texture| (texture.width(), texture.height()))
                .unwrap_or((0, 0));
            let (width, height) = resolution;
            (
                framebuffer,
                state.render_pass.clone(),
                state.pipeline.clone(),
                width,
                height,
            )
        };
        if width == 0 || height == 0 {
            return PassOutcome::skipped("the target has no size");
        }

        let mut commands: Vec<Command> = Vec::with_capacity(8 + state.items.len() * 4);
        if shared {
            // The barrier between the opaque pass and this one.
            //
            // The order of the passes is not a memory dependency: the forward
            // pass wrote the depth and the colour, and without this the water
            // pass would read them without being guaranteed to see them. A
            // render pass's own dependencies do not cover the gap between two
            // render passes — the second pass's "external" subpass
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
        commands.push(Command::BeginRenderPass(
            crate::rhi::command::pass::render::RenderPassBeginInfo {
                render_pass,
                framebuffer,
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
        commands.push(Command::BindGraphicsPipeline(pipeline.clone()));

        for (index, item) in state.items.iter().enumerate() {
            let Some(descriptor_set) = state.descriptor_sets.get(index) else {
                continue;
            };
            commands.push(Command::BindDescriptorSets {
                pipeline: pipeline.clone(),
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
                        crate::warn!("water", "{}: 8-bit indices are not supported", item.name);
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
                crate::error!("water", "a command was rejected: the encoder is not recording");
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

impl Default for WaterPass {
    fn default() -> Self {
        Self::new(WaterConfig::default())
    }
}
