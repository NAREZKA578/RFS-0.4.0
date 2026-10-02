//! GBuffer Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This pass renders the scene geometry to multiple render targets (MRT):
//! - Position (XYZ + depth)
//! - Normal (XYZ)
//! - Albedo (RGB) + Roughness (A)
//! - Material (Metallic, AO, Emissive)

use std::collections::HashMap;

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::pipeline::graphics::PrimitiveTopology;
use crate::rhi::{
    AttachmentDescription, AttachmentReference, CommandEncoder, FramebufferAttachment,
    FramebufferDesc, GraphicsPipeline, RenderPassDesc, SubpassDescription,
};

/// GBuffer render pass
pub struct GBufferPass {
    base: BaseRenderPass,
    pipeline: Option<GraphicsPipeline>,
    /// GBuffer textures
    position_texture: Option<crate::rhi::Texture>,
    normal_texture: Option<crate::rhi::Texture>,
    albedo_texture: Option<crate::rhi::Texture>,
    material_texture: Option<crate::rhi::Texture>,
    /// Depth texture
    depth_texture: Option<crate::rhi::Texture>,
    /// Render pass
    render_pass: Option<crate::rhi::RenderPass>,
    /// Framebuffer
    framebuffer: Option<crate::rhi::Framebuffer>,
}

impl GBufferPass {
    pub fn new() -> Self {
        let mut base = BaseRenderPass::new("gbuffer");

        // Add outputs
        base.add_output("gbuffer_position");
        base.add_output("gbuffer_normal");
        base.add_output("gbuffer_albedo");
        base.add_output("gbuffer_material");
        base.add_output("depth");

        Self {
            base,
            pipeline: None,
            position_texture: None,
            normal_texture: None,
            albedo_texture: None,
            material_texture: None,
            depth_texture: None,
            render_pass: None,
            framebuffer: None,
        }
    }

    fn create_pipeline(&self, device: &crate::rhi::Device) -> Option<GraphicsPipeline> {
        let spirv = include_bytes!("../../../Tool/client_tests/assets/shaders/gbuffer.spv");
        let vs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("gbuffer_vs".into()),
        });
        let fs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("fs_main".into()),
            name: Some("gbuffer_fs".into()),
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
                    attachments: vec![
                        crate::rhi::ColorBlendAttachment::default(),
                        crate::rhi::ColorBlendAttachment::default(),
                        crate::rhi::ColorBlendAttachment::default(),
                        crate::rhi::ColorBlendAttachment::default(),
                    ],
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

/// Create a GBuffer attachment through the device.
///
/// `Texture::new` is a CPU-side stub with no GPU backing. A framebuffer built
/// from its view therefore fails much later, at draw time, as "framebuffer
/// attachment has no GPU backing" вЂ” the error surfaces in a different place
/// from the mistake, which is what #261 and #262 describe. Going through the
/// device is what makes the attachment real, and the reason is reported here
/// rather than swallowed: a silent `None` would leave the pass doing nothing
/// and looking like a pass that draws nothing.
fn attachment(
    device: &crate::rhi::Device,
    what: &'static str,
    desc: crate::rhi::TextureDesc,
) -> Option<crate::rhi::Texture> {
    match device.create_texture_from_desc(&desc) {
        Ok(texture) if texture.has_gpu_backing() => Some(texture),
        Ok(_) => {
            crate::error!("gbuffer", "{what}: created without GPU backing");
            None
        }
        Err(e) => {
            crate::error!("gbuffer", "{what} attachment: {e}");
            None
        }
    }
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

    fn initialize(&mut self, device: &crate::rhi::Device, context: &RenderContext) {
        let width = context.resolution.width;
        let height = context.resolution.height;

        // Create GBuffer textures
        let position_format = crate::rhi::Format::RGBA32_SFLOAT;
        let normal_format = crate::rhi::Format::RGBA32_SFLOAT;
        let albedo_format = crate::rhi::Format::RGBA8_UNORM;
        let material_format = crate::rhi::Format::RGBA8_UNORM;
        let depth_format = crate::rhi::Format::D32_SFLOAT;

        self.position_texture = attachment(device, "position", crate::rhi::TextureDesc {
            width,
            height,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: position_format,
            usage: crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            sample_count: crate::rhi::SampleCount::X1,
            dimensions: crate::rhi::TextureDimensions::D2,
            ..Default::default()
        });

        self.normal_texture = attachment(device, "normal", crate::rhi::TextureDesc {
            width,
            height,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: normal_format,
            usage: crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            sample_count: crate::rhi::SampleCount::X1,
            dimensions: crate::rhi::TextureDimensions::D2,
            ..Default::default()
        });

        self.albedo_texture = attachment(device, "albedo", crate::rhi::TextureDesc {
            width,
            height,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: albedo_format,
            usage: crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            sample_count: crate::rhi::SampleCount::X1,
            dimensions: crate::rhi::TextureDimensions::D2,
            ..Default::default()
        });

        self.material_texture = attachment(device, "material", crate::rhi::TextureDesc {
            width,
            height,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: material_format,
            usage: crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            sample_count: crate::rhi::SampleCount::X1,
            dimensions: crate::rhi::TextureDimensions::D2,
            ..Default::default()
        });

        self.depth_texture = attachment(device, "depth", crate::rhi::TextureDesc {
            width,
            height,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: depth_format,
            usage: crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT
                | crate::rhi::TextureUsage::SAMPLED,
            sample_count: crate::rhi::SampleCount::X1,
            dimensions: crate::rhi::TextureDimensions::D2,
            ..Default::default()
        });

        // Create render pass
        let attachments = vec![
            // Position
            AttachmentDescription {
                format: position_format,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
            },
            // Normal
            AttachmentDescription {
                format: normal_format,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
            },
            // Albedo
            AttachmentDescription {
                format: albedo_format,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
            },
            // Material
            AttachmentDescription {
                format: material_format,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
            },
            // Depth
            AttachmentDescription {
                format: depth_format,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::Clear,
                stencil_store_op: crate::rhi::StoreOp::Store,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
            },
        ];

        let subpass = SubpassDescription {
            pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
            color_attachments: vec![
                AttachmentReference {
                    attachment: 0,
                    layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                },
                AttachmentReference {
                    attachment: 1,
                    layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                },
                AttachmentReference {
                    attachment: 2,
                    layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                },
                AttachmentReference {
                    attachment: 3,
                    layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
                },
            ],
            depth_stencil_attachment: Some(AttachmentReference {
                attachment: 4,
                layout: crate::rhi::TextureLayout::DepthStencilAttachmentOptimal,
            }),
            ..Default::default()
        };

        let render_pass_desc = RenderPassDesc {
            attachments,
            subpasses: vec![subpass],
            dependencies: vec![],
        };

        self.render_pass = Some(crate::rhi::RenderPass::new(render_pass_desc));

        // Create framebuffer
        //
        // Bug в„–186: each of these five `as_ref().unwrap()` calls aborted the
        // process if a single G-buffer target had failed to be created. A
        // missing render target is a recoverable renderer state (a resize that
        // could not allocate), not a process-fatal condition, so the pass now
        // reports the missing target by name and returns.
        let missing = |name: &str| {
            eprintln!("[render] gbuffer: {name} is not created; framebuffer not rebuilt");
        };
        let (Some(position), Some(normal), Some(albedo), Some(material), Some(depth)) = (
            self.position_texture.as_ref(),
            self.normal_texture.as_ref(),
            self.albedo_texture.as_ref(),
            self.material_texture.as_ref(),
            self.depth_texture.as_ref(),
        ) else {
            if self.position_texture.is_none() {
                missing("position_texture");
            }
            if self.normal_texture.is_none() {
                missing("normal_texture");
            }
            if self.albedo_texture.is_none() {
                missing("albedo_texture");
            }
            if self.material_texture.is_none() {
                missing("material_texture");
            }
            if self.depth_texture.is_none() {
                missing("depth_texture");
            }
            return;
        };

        let position_view = position.create_view(Default::default());
        let normal_view = normal.create_view(Default::default());
        let albedo_view = albedo.create_view(Default::default());
        let material_view = material.create_view(Default::default());
        let depth_view = depth.create_view(Default::default());

        let Some(render_pass) = self.render_pass.clone() else {
            missing("render_pass");
            return;
        };

        self.framebuffer = Some(crate::rhi::Framebuffer::new(FramebufferDesc {
            render_pass,
            attachments: vec![
                FramebufferAttachment {
                    texture_view: position_view,
                    layer: 0,
                    mip_level: 0,
                },
                FramebufferAttachment {
                    texture_view: normal_view,
                    layer: 0,
                    mip_level: 0,
                },
                FramebufferAttachment {
                    texture_view: albedo_view,
                    layer: 0,
                    mip_level: 0,
                },
                FramebufferAttachment {
                    texture_view: material_view,
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
        }));

        // Create pipeline
        self.pipeline = self.create_pipeline(device);
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) {
        if !self.base.is_enabled() {
            return;
        }
        let Some(framebuffer) = self.framebuffer.as_ref() else {
            return;
        };
        let Some(render_pass) = self.render_pass.as_ref() else {
            return;
        };
        let Some(pipeline) = self.pipeline.as_ref() else {
            return;
        };

        encoder.begin_render_pass(
            render_pass,
            framebuffer,
            crate::rhi::Rect2D {
                offset: crate::rhi::Offset2D { x: 0, y: 0 },
                extent: crate::rhi::Extent2D {
                    width: context.resolution.width,
                    height: context.resolution.height,
                },
            },
            &[
                crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0),
                crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0),
                crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0),
                crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0),
                crate::rhi::ClearValue::depth_stencil(1.0, 0),
            ],
            1.0,
            0,
        );

        encoder.set_viewport(
            0.0,
            0.0,
            context.resolution.width as f32,
            context.resolution.height as f32,
            0.0,
            1.0,
        );
        encoder.set_scissor(
            0,
            0,
            context.resolution.width,
            context.resolution.height,
        );

        encoder.bind_pipeline(pipeline);

        let view = scene.camera_view_matrix();
        let proj = scene.camera_projection_matrix();
        let _mvp = proj * view;

        for entity_id in scene.opaque_entities() {
            if let Some(entity) = scene.get_entity(entity_id) {
                if let Some(mesh_component) = entity.mesh() {
                    let mesh = mesh_component.get_lod_mesh(0);
                    if let Some(vertex_buffer) = mesh.vertex_buffer() {
                        if let Some(index_buffer) = mesh.index_buffer() {
                            encoder.bind_vertex_buffer(vertex_buffer);
                            encoder.bind_index_buffer(index_buffer, crate::rhi::IndexType::U32);
                            encoder.draw_indexed_instanced(mesh.index_count(), 1, 0, 0, 0);
                        }
                    }
                }
            }
        }

        encoder.end_render_pass();
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






