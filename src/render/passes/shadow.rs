//! Shadow Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This pass renders the scene from the light's perspective to create shadow maps.
//! Supports:
//! - Cascaded Shadow Maps for directional lights
//! - Omnidirectional shadow maps for point lights
//! - Spot light shadow maps

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::{
    AttachmentDescription, AttachmentReference, CommandEncoder, FramebufferAttachment,
    FramebufferDesc, GraphicsPipeline, RenderPassDesc, SubpassDescription,
};
use std::collections::HashMap;

/// Shadow map configuration
#[derive(Debug, Clone)]
pub struct ShadowConfig {
    pub resolution: u32,
    pub cascade_count: usize,
    pub cascade_distances: Vec<f32>,
    pub bias: f32,
    pub normal_bias: f32,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            resolution: 2048,
            cascade_count: 4,
            cascade_distances: vec![10.0, 50.0, 100.0, 200.0],
            bias: 0.0001,
            normal_bias: 0.001,
        }
    }
}

/// Shadow pass for a single light
pub struct ShadowPass {
    base: BaseRenderPass,
    config: ShadowConfig,
    /// Shadow map textures (one per cascade for directional lights)
    shadow_maps: Vec<Option<crate::rhi::Texture>>,
    shadow_views: Vec<Option<crate::rhi::TextureView>>,
    /// Depth pipeline
    depth_pipeline: Option<GraphicsPipeline>,
    /// Render pass
    render_pass: Option<crate::rhi::RenderPass>,
    /// Framebuffers (one per cascade)
    framebuffers: Vec<Option<crate::rhi::Framebuffer>>,
}

impl ShadowPass {
    pub fn new(config: ShadowConfig) -> Self {
        let mut base = BaseRenderPass::new("shadow");
        let cascade_count = config.cascade_count;

        // Shadow pass outputs shadow maps
        for i in 0..cascade_count {
            base.add_output(&format!("shadow_map_cascade_{}", i));
        }

        // Shadow pass depends on scene being ready
        // (no actual pass dependencies, but needs scene data)

        Self {
            base,
            config,
            shadow_maps: vec![None; cascade_count],
            shadow_views: vec![None; cascade_count],
            depth_pipeline: None,
            render_pass: None,
            framebuffers: vec![None; cascade_count],
        }
    }

    pub fn config(&self) -> &ShadowConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut ShadowConfig {
        &mut self.config
    }

    fn create_pipeline(&self, device: &crate::rhi::Device) -> Option<GraphicsPipeline> {
        use crate::rhi::pipeline::graphics::PrimitiveTopology;

        let spirv = include_bytes!("../../../Tool/client_tests/assets/shaders/shadow.spv");
        let vs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("shadow_vs".into()),
        });
        let fs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("fs_main".into()),
            name: Some("shadow_fs".into()),
        });

        let render_pass = self.render_pass.clone()?;
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
                    attachments: vec![],
                    ..Default::default()
                }),
                depth_stencil_state: Some(crate::rhi::DepthStencilState::enabled()),
                ..Default::default()
            },
            &render_pass,
            &[],
        );
        pipeline.ok()
    }
}

impl RenderPass for ShadowPass {
    fn name(&self) -> &str {
        self.base.name()
    }

    fn dependencies(&self) -> &[PassDependency] {
        self.base.dependencies()
    }

    fn outputs(&self) -> &[String] {
        self.base.outputs()
    }

    fn initialize(&mut self, device: &crate::rhi::Device, _context: &RenderContext) {
        let resolution = self.config.resolution;
        let depth_format = crate::rhi::Format::D32_SFLOAT;

        // Create shadow map textures
        for i in 0..self.config.cascade_count {
            self.shadow_maps[i] = Some(crate::rhi::Texture::new(crate::rhi::TextureDesc {
                width: resolution,
                height: resolution,
                depth: 1,
                mip_levels: 1,
                array_layers: 1,
                format: depth_format,
                usage: crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT
                    | crate::rhi::TextureUsage::SAMPLED,
                sample_count: crate::rhi::SampleCount::X1,
                dimensions: crate::rhi::TextureDimensions::D2,
                ..Default::default()
            }));

            if let Some(ref texture) = self.shadow_maps[i] {
                self.shadow_views[i] =
                    Some(texture.create_view(crate::rhi::TextureViewDesc::default()));
            }
        }

        // Create render pass for shadow mapping
        let attachment = AttachmentDescription {
            format: depth_format,
            samples: crate::rhi::SampleCount::X1,
            load_op: crate::rhi::LoadOp::Clear,
            store_op: crate::rhi::StoreOp::Store,
            stencil_load_op: crate::rhi::LoadOp::DontCare,
            stencil_store_op: crate::rhi::StoreOp::DontCare,
            initial_layout: crate::rhi::TextureLayout::Undefined,
            final_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
        };

        let subpass = SubpassDescription {
            pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
            color_attachments: vec![], // No color attachments for shadow pass
            depth_stencil_attachment: Some(AttachmentReference {
                attachment: 0,
                layout: crate::rhi::TextureLayout::DepthStencilAttachmentOptimal,
            }),
            ..Default::default()
        };

        let render_pass_desc = RenderPassDesc {
            attachments: vec![attachment],
            subpasses: vec![subpass],
            dependencies: vec![],
        };

        self.render_pass = Some(crate::rhi::RenderPass::new(render_pass_desc));

        // Create framebuffers
        for i in 0..self.config.cascade_count {
            if let Some(ref view) = self.shadow_views[i] {
                self.framebuffers[i] = Some(crate::rhi::Framebuffer::new(FramebufferDesc {
                    render_pass: self.render_pass.as_ref().unwrap().clone(),
                    attachments: vec![FramebufferAttachment {
                        texture_view: view.clone(),
                        layer: 0,
                        mip_level: 0,
                    }],
                    width: resolution,
                    height: resolution,
                    layers: 1,
                }));
            }
        }

        // Create depth pipeline
        self.depth_pipeline = self.create_pipeline(device);
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        _context: &mut RenderContext,
        scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) {
        if !self.base.is_enabled() {
            return;
        }
        let Some(render_pass) = self.render_pass.as_ref() else {
            return;
        };
        let Some(pipeline) = self.depth_pipeline.as_ref() else {
            return;
        };

        for i in 0..self.config.cascade_count {
            let Some(framebuffer) = self.framebuffers[i].as_ref() else {
                continue;
            };

            encoder.begin_render_pass(
                render_pass,
                framebuffer,
                crate::rhi::Rect2D {
                    offset: crate::rhi::Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D {
                        width: self.config.resolution,
                        height: self.config.resolution,
                    },
                },
                &[crate::rhi::ClearValue::depth_stencil(1.0, 0)],
                1.0,
                0,
            );

            encoder.set_viewport(
                0.0,
                0.0,
                self.config.resolution as f32,
                self.config.resolution as f32,
                0.0,
                1.0,
            );
            encoder.set_scissor(
                0,
                0,
                self.config.resolution,
                self.config.resolution,
            );

            encoder.bind_pipeline(pipeline);

            for entity_id in scene.shadow_casting_entities() {
                if let Some(entity) = scene.get_entity(entity_id) {
                    if let Some(mesh_component) = entity.mesh() {
                        let mesh = mesh_component.get_lod_mesh(0);
                        if let Some(vertex_buffer) = mesh.vertex_buffer() {
                            if let Some(index_buffer) = mesh.index_buffer() {
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

            encoder.end_render_pass();
        }
    }

    fn resize(&mut self, _width: u32, _height: u32) {
        // For shadow maps, we typically don't resize based on screen resolution
        // The shadow map resolution is configured separately
    }

    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }
}

impl Default for ShadowPass {
    fn default() -> Self {
        Self::new(ShadowConfig::default())
    }
}

