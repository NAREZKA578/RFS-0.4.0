//! Transparent Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This pass renders transparent objects (water, smoke, fire, glass) with proper blending.
//! Uses depth sorting to ensure correct rendering order.

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::graph::types::ResourceUsage;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::pipeline::graphics::PrimitiveTopology;
use crate::rhi::{CommandEncoder, Pipeline, TextureView};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

/// Transparent object for sorting
#[derive(Debug, Clone)]
pub struct TransparentObject {
    pub entity_id: usize,
    pub distance: f32,
    pub blend_mode: BlendMode,
}

/// Blend modes for transparent objects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum BlendMode {
    /// Standard alpha blending (src * alpha + dst * (1 - alpha))
    #[default]
    Alpha,
    /// Additive blending (src + dst)
    Additive,
    /// Multiplicative blending (src * dst)
    Multiplicative,
    /// Screen blending (src + dst - src * dst)
    Screen,
    /// Custom blending
    Custom,
}


/// Transparent pass configuration
#[derive(Debug, Clone)]
pub struct TransparentConfig {
    pub sort_mode: SortMode,
    pub max_transparent_objects: usize,
}

/// Sorting mode for transparent objects
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    /// Sort by distance from camera (back to front)
    BackToFront,
    /// Sort by distance from camera (front to back)
    FrontToBack,
    /// No sorting (use depth buffer)
    None,
}

impl Default for TransparentConfig {
    fn default() -> Self {
        Self {
            sort_mode: SortMode::BackToFront,
            max_transparent_objects: 1000,
        }
    }
}

/// Transparent pass
pub struct TransparentPass {
    base: BaseRenderPass,
    config: TransparentConfig,
    /// Pipeline for each blend mode
    pipelines: HashMap<BlendMode, Pipeline>,
    /// Output texture (accumulation buffer)
    output_texture: Option<crate::rhi::Texture>,
    output_view: Option<TextureView>,
    /// Depth texture (for depth testing)
    depth_texture: Option<crate::rhi::Texture>,
    depth_view: Option<TextureView>,
    /// Render pass
    render_pass: Option<crate::rhi::RenderPass>,
    /// Framebuffer
    framebuffer: Option<crate::rhi::Framebuffer>,
    /// Particle system (bug в„–211: was unreachable)
    particle_system: Option<Arc<crate::render::particles::ParticleSystem>>,
}

impl TransparentPass {
    pub fn new(config: TransparentConfig) -> Self {
        let mut base = BaseRenderPass::new("transparent");

        // Transparent pass depends on GBuffer depth
        base.add_dependency("gbuffer", "depth", ResourceUsage::Sampled);

        // Transparent pass depends on lighting
        base.add_dependency("lighting", "lighting", ResourceUsage::Sampled);

        // Transparent pass outputs to final color
        base.add_output("final");

        Self {
            base,
            config,
            pipelines: HashMap::new(),
            output_texture: None,
            output_view: None,
            depth_texture: None,
            depth_view: None,
            render_pass: None,
            framebuffer: None,
            particle_system: None,
        }
    }

    pub fn set_particle_system(&mut self, system: Arc<crate::render::particles::ParticleSystem>) {
        self.particle_system = Some(system);
    }

    pub fn config(&self) -> &TransparentConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut TransparentConfig {
        &mut self.config
    }

    fn create_pipelines(&mut self, device: &crate::rhi::Device) {
        let spirv = include_bytes!("../../../Tool/client_tests/assets/shaders/transparent.spv");
        let vs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("transparent_vs".into()),
        });
        let fs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("fs_main".into()),
            name: Some("transparent_fs".into()),
        });

        let render_pass = match self.render_pass.clone() {
            Some(rp) => rp,
            None => return,
        };

        let pipeline = device.create_gpu_graphics_pipeline(
            &crate::rhi::GraphicsPipelineDesc {
                shader_stages: vec![
                    crate::rhi::PipelineShaderStage {
                        stage: crate::rhi::ShaderStage::VERTEX,
                        module: vs_module,
                        entry_point: "vs_main".into(),
                    },
                    crate::rhi::PipelineShaderStage {
                        stage: crate::rhi::ShaderStage::FRAGMENT,
                        module: fs_module,
                        entry_point: "fs_main".into(),
                    },
                ],
                vertex_input_state: Some(crate::rhi::VertexInputState {
                    bindings: vec![crate::rhi::VertexBinding {
                        binding: 0,
                        stride: std::mem::size_of::<crate::render::meshes::Vertex>() as u32,
                        input_rate: crate::rhi::VertexInputRate::Vertex,
                    }],
                    attributes: vec![
                        crate::rhi::VertexAttribute {
                            binding: 0,
                            location: 0,
                            format: crate::rhi::Format::R32G32B32_SFLOAT,
                            offset: std::mem::offset_of!(crate::render::meshes::Vertex, position) as u32,
                        },
                        crate::rhi::VertexAttribute {
                            binding: 0,
                            location: 1,
                            format: crate::rhi::Format::R32G32B32_SFLOAT,
                            offset: std::mem::offset_of!(crate::render::meshes::Vertex, normal) as u32,
                        },
                        crate::rhi::VertexAttribute {
                            binding: 0,
                            location: 2,
                            format: crate::rhi::Format::RGBA32_SFLOAT,
                            offset: std::mem::offset_of!(crate::render::meshes::Vertex, tangent) as u32,
                        },
                        crate::rhi::VertexAttribute {
                            binding: 0,
                            location: 3,
                            format: crate::rhi::Format::R32G32_SFLOAT,
                            offset: std::mem::offset_of!(crate::render::meshes::Vertex, tex_coord) as u32,
                        },
                        crate::rhi::VertexAttribute {
                            binding: 0,
                            location: 4,
                            format: crate::rhi::Format::RGBA32_SFLOAT,
                            offset: std::mem::offset_of!(crate::render::meshes::Vertex, color) as u32,
                        },
                    ],
                }),
                input_assembly_state: crate::rhi::InputAssemblyState {
                    topology: PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                rasterizer_state: Some(crate::rhi::RasterizerState::double_sided()),
                multisample_state: crate::rhi::MultisampleState {
                    sample_count: crate::rhi::SampleCount::X1,
                    ..Default::default()
                },
                color_blend_state: Some(crate::rhi::ColorBlendState {
                    attachments: vec![crate::rhi::ColorBlendAttachment {
                        blend_enable: true,
                        src_color_blend_factor: crate::rhi::BlendFactor::SrcAlpha,
                        dst_color_blend_factor: crate::rhi::BlendFactor::OneMinusSrcAlpha,
                        color_blend_op: crate::rhi::BlendOp::Add,
                        src_alpha_blend_factor: crate::rhi::BlendFactor::One,
                        dst_alpha_blend_factor: crate::rhi::BlendFactor::OneMinusSrcAlpha,
                        alpha_blend_op: crate::rhi::BlendOp::Add,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                depth_stencil_state: Some(crate::rhi::DepthStencilState {
                    depth_test_enable: true,
                    depth_write_enable: false,
                    ..Default::default()
                }),
                ..Default::default()
            },
            &render_pass,
            &[],
        );

        if let Ok(pipeline) = pipeline {
            self.pipelines.insert(BlendMode::Alpha, pipeline.clone());
            self.pipelines.insert(BlendMode::Additive, pipeline.clone());
            self.pipelines.insert(BlendMode::Multiplicative, pipeline.clone());
            self.pipelines.insert(BlendMode::Screen, pipeline);
        }
    }
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

    fn initialize(&mut self, device: &crate::rhi::Device, context: &RenderContext) {
        let width = context.resolution.width;
        let height = context.resolution.height;

        // Create output texture (accumulation buffer)
        let output_format = crate::rhi::Format::RGBA32_SFLOAT;
        self.output_texture = Some(device.create_texture(
            width,
            height,
            1,
            output_format,
            crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            1,
        ));
        // Bug в„–186: the view was built by unwrapping the texture that had just
        // been stored, so a failed `create_texture` aborted the process instead
        // of leaving the pass without a target. Build the view from the value
        // we actually hold.
        let Some(output_texture) = self.output_texture.take() else {
            eprintln!("[render] transparent: output texture is missing; resize aborted");
            return;
        };
        self.output_view = Some(output_texture.create_view(Default::default()));
        self.output_texture = Some(output_texture);

        // Create depth texture (copy from GBuffer)
        let depth_format = crate::rhi::Format::D32_SFLOAT;
        self.depth_texture = Some(device.create_texture(
            width,
            height,
            1,
            depth_format,
            crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            1,
        ));
        let Some(depth_texture) = self.depth_texture.take() else {
            eprintln!("[render] transparent: depth texture is missing; resize aborted");
            return;
        };
        self.depth_view = Some(depth_texture.create_view(Default::default()));
        self.depth_texture = Some(depth_texture);

        // Create render pass
        let attachments = vec![
            // Output (accumulation)
            crate::rhi::AttachmentDescription {
                format: output_format,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Load, // Load previous content
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
                final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
            },
            // Depth
            crate::rhi::AttachmentDescription {
                format: depth_format,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Load, // Load from GBuffer
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
                final_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
            },
        ];

        let subpass = crate::rhi::SubpassDescription {
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
        };

        let render_pass_desc = crate::rhi::RenderPassDesc {
            attachments,
            subpasses: vec![subpass],
            dependencies: vec![],
        };

        self.render_pass = Some(device.create_render_pass(&render_pass_desc));

        // Create pipelines for each blend mode
        self.create_pipelines(device);

        // Create framebuffer
        //
        // Bug в„–186: these three views were unwrapped. A missing target now
        // aborts the resize with a message instead of the process.
        let (Some(output_view), Some(depth_view), Some(render_pass)) = (
            self.output_view.clone(),
            self.depth_view.clone(),
            self.render_pass.clone(),
        ) else {
            eprintln!("[render] transparent: output/depth view or render pass missing; framebuffer not rebuilt");
            return;
        };
        let fb_attachments = vec![output_view, depth_view];

        self.framebuffer = Some(device.create_framebuffer(
            &render_pass,
            fb_attachments,
            width,
            height,
        ));
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        scene: &mut Scene,
        resources: &HashMap<String, GraphResource>,
    ) {
        if !self.base.is_enabled() {
            return;
        }

        let width = context.resolution.width;
        let height = context.resolution.height;

        // Get depth texture from GBuffer
        if let Some(depth_resource) = resources.get("depth") {
            if let Some(_depth_tex) = depth_resource.texture() {
                // Copy depth from GBuffer to our depth texture
                // This is a placeholder - in practice, we might share the same texture
            }
        }

        // Get lighting texture
        let _lighting_tex = super::base::get_texture(resources, "lighting");

        // Collect and sort transparent objects
        let mut transparent_objects: Vec<TransparentObject> = scene
            .entities()
            .iter()
            .filter(|e| e.is_transparent() && e.is_visible())
            .map(|e| {
                let distance = e.distance_from_camera(context);
                TransparentObject {
                    entity_id: e.id(),
                    distance,
                    blend_mode: match e.blend_mode() {
                        crate::render::scene::components::BlendMode::Opaque => BlendMode::Alpha,
                        crate::render::scene::components::BlendMode::Alpha => BlendMode::Alpha,
                        crate::render::scene::components::BlendMode::Additive => {
                            BlendMode::Additive
                        }
                        crate::render::scene::components::BlendMode::Multiplicative => {
                            BlendMode::Multiplicative
                        }
                        crate::render::scene::components::BlendMode::Screen => BlendMode::Screen,
                    },
                }
            })
            .collect();

        // Sort transparent objects
        match self.config.sort_mode {
            SortMode::BackToFront => {
                transparent_objects.sort_by(|a, b| {
                    b.distance
                        .partial_cmp(&a.distance)
                        .unwrap_or(Ordering::Equal)
                });
            }
            SortMode::FrontToBack => {
                transparent_objects.sort_by(|a, b| {
                    a.distance
                        .partial_cmp(&b.distance)
                        .unwrap_or(Ordering::Equal)
                });
            }
            SortMode::None => {}
        }

        // Begin render pass
        if let (Some(render_pass), Some(framebuffer)) = (&self.render_pass, &self.framebuffer) {
            // Clear color is not used because we're loading the existing content
            encoder.begin_render_pass(
                render_pass,
                framebuffer,
                crate::rhi::Rect2D {
                    offset: crate::rhi::Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D { width, height },
                },
                &[], // No color clears (we're accumulating)
                1.0,
                0,
            );

            // Set viewport
            encoder.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
            encoder.set_scissor(0, 0, width, height);

            // Render particles (bug в„–211: was unreachable)
            if let Some(ref ps) = self.particle_system {
                ps.render(encoder, context);
            }

            // Render each transparent object
            for obj in &transparent_objects {
                if let Some(entity) = scene.get_entity(obj.entity_id) {
                    if let Some(mesh_component) = entity.mesh() {
                        let mesh = mesh_component.get_lod_mesh(0);
                        if let Some(vertex_buffer) = mesh.vertex_buffer() {
                            if let Some(index_buffer) = mesh.index_buffer() {
                                if let Some(pipeline) = self.pipelines.get(&obj.blend_mode) {
                                    encoder.bind_pipeline(pipeline);
                                    encoder.bind_vertex_buffer(vertex_buffer);
                                    encoder.bind_index_buffer(index_buffer, crate::rhi::IndexType::U32);
                                    encoder.draw_indexed_instanced(
                                        mesh.index_count(),
                                        1,
                                        0,
                                        0,
                                        0,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // End render pass
            encoder.end_render_pass();
        }
    }

    fn resize(&mut self, _width: u32, _height: u32) {
        // Recreate textures with new dimensions
    }

    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }
}

impl Default for TransparentPass {
    fn default() -> Self {
        Self::new(TransparentConfig::default())
    }
}




