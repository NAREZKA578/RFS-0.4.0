//! Lighting Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This pass computes lighting using the GBuffer data.
//! Supports:
//! - PBR (Physically Based Rendering)
//! - Multiple light types (Directional, Point, Spot)
//! - Image-Based Lighting (IBL)
//! - Screen Space Ambient Occlusion (SSAO)
//! - Screen Space Reflections (SSR)

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::graph::types::ResourceUsage;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::pipeline::graphics::PrimitiveTopology;
use crate::rhi::{
    AttachmentDescription, AttachmentReference, CommandEncoder, FramebufferAttachment,
    FramebufferDesc, GraphicsPipeline, RenderPassDesc, SubpassDescription,
};
use std::collections::HashMap;

/// Lighting configuration
#[derive(Debug, Clone)]
pub struct LightingConfig {
    pub max_direction_lights: usize,
    pub max_point_lights: usize,
    pub max_spot_lights: usize,
    pub use_ibl: bool,
    pub use_ssao: bool,
    pub use_ssr: bool,
    pub ssao_radius: f32,
    pub ssao_bias: f32,
}

impl Default for LightingConfig {
    fn default() -> Self {
        Self {
            max_direction_lights: 4,
            max_point_lights: 16,
            max_spot_lights: 16,
            use_ibl: true,
            use_ssao: true,
            use_ssr: true,
            ssao_radius: 0.5,
            ssao_bias: 0.05,
        }
    }
}

/// Lighting pass
pub struct LightingPass {
    base: BaseRenderPass,
    config: LightingConfig,
    /// Lighting pipeline
    pipeline: Option<GraphicsPipeline>,
    /// SSAO pipeline (if enabled)
    _ssao_pipeline: Option<GraphicsPipeline>,
    /// SSR pipeline (if enabled)
    _ssr_pipeline: Option<GraphicsPipeline>,
    /// Output texture
    output_texture: Option<crate::rhi::Texture>,
    output_view: Option<crate::rhi::TextureView>,
    /// SSAO texture (if enabled)
    ssao_texture: Option<crate::rhi::Texture>,
    ssao_view: Option<crate::rhi::TextureView>,
    /// SSR texture (if enabled)
    ssr_texture: Option<crate::rhi::Texture>,
    ssr_view: Option<crate::rhi::TextureView>,
    /// Render pass
    render_pass: Option<crate::rhi::RenderPass>,
    /// Framebuffer
    framebuffer: Option<crate::rhi::Framebuffer>,
}

impl LightingPass {
    pub fn new(config: LightingConfig) -> Self {
        let mut base = BaseRenderPass::new("lighting");

        // Lighting pass depends on GBuffer
        base.add_dependency("gbuffer", "gbuffer_position", ResourceUsage::Sampled);
        base.add_dependency("gbuffer", "gbuffer_normal", ResourceUsage::Sampled);
        base.add_dependency("gbuffer", "gbuffer_albedo", ResourceUsage::Sampled);
        base.add_dependency("gbuffer", "gbuffer_material", ResourceUsage::Sampled);
        base.add_dependency("gbuffer", "depth", ResourceUsage::Sampled);

        // Lighting pass depends on shadow maps
        base.add_dependency("shadow", "shadow_map_cascade_0", ResourceUsage::Sampled);

        // Lighting pass outputs lighting texture
        base.add_output("lighting");

        if config.use_ssao {
            base.add_output("ssao");
        }

        if config.use_ssr {
            base.add_output("ssr");
        }

        Self {
            base,
            config,
            pipeline: None,
            _ssao_pipeline: None,
            _ssr_pipeline: None,
            output_texture: None,
            output_view: None,
            ssao_texture: None,
            ssao_view: None,
            ssr_texture: None,
            ssr_view: None,
            render_pass: None,
            framebuffer: None,
        }
    }

    pub fn config(&self) -> &LightingConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut LightingConfig {
        &mut self.config
    }

    fn create_pipeline(&self, device: &crate::rhi::Device) -> Option<GraphicsPipeline> {
        let spirv = include_bytes!("../../../Tool/client_tests/assets/shaders/lighting.spv");
        let vs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("lighting_vs".into()),
        });
        let fs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("fs_main".into()),
            name: Some("lighting_fs".into()),
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
                        stride: 12,
                        input_rate: crate::rhi::VertexInputRate::Vertex,
                    }],
                    attributes: vec![
                        crate::rhi::VertexAttribute {
                            binding: 0,
                            location: 0,
                            format: crate::rhi::Format::R32G32B32_SFLOAT,
                            offset: 0,
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
                    attachments: vec![crate::rhi::ColorBlendAttachment::default()],
                    ..Default::default()
                }),
                depth_stencil_state: None,
                ..Default::default()
            },
            &render_pass,
            &[],
        );
        pipeline.ok()
    }
}

impl RenderPass for LightingPass {
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

        // Create output texture (HDR lighting)
        let output_format = crate::rhi::Format::RGBA16_SFLOAT;
        self.output_texture = Some(crate::rhi::Texture::new(crate::rhi::TextureDesc {
            width,
            height,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: output_format,
            usage: crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            sample_count: crate::rhi::SampleCount::X1,
            dimensions: crate::rhi::TextureDimensions::D2,
            ..Default::default()
        }));
        // Bug в„–186: an absent target aborted the process. Create the view from
        // the value we just built instead of looking it up and unwrapping it.
        let output_texture = self.output_texture.take();
        match output_texture {
            Some(tex) => {
                self.output_view = Some(tex.create_view(Default::default()));
                self.output_texture = Some(tex);
            }
            None => {
                eprintln!("[render] lighting: output texture creation failed; view not built");
                return;
            }
        }

        // Create SSAO texture if enabled
        if self.config.use_ssao {
            self.ssao_texture = Some(crate::rhi::Texture::new(crate::rhi::TextureDesc {
                width,
                height,
                depth: 1,
                mip_levels: 1,
                array_layers: 1,
                format: crate::rhi::Format::R8_UNORM,
                usage: crate::rhi::TextureUsage::COLOR_ATTACHMENT
                    | crate::rhi::TextureUsage::SAMPLED,
                sample_count: crate::rhi::SampleCount::X1,
                dimensions: crate::rhi::TextureDimensions::D2,
                ..Default::default()
            }));
            let tex = self.ssao_texture.take();
            match tex {
                Some(tex) => {
                    self.ssao_view = Some(tex.create_view(Default::default()));
                    self.ssao_texture = Some(tex);
                }
                None => {
                    eprintln!("[render] lighting: SSAO texture creation failed; view not built");
                    return;
                }
            }
        }

        // Create SSR texture if enabled
        if self.config.use_ssr {
            self.ssr_texture = Some(crate::rhi::Texture::new(crate::rhi::TextureDesc {
                width,
                height,
                depth: 1,
                mip_levels: 1,
                array_layers: 1,
                format: crate::rhi::Format::RGBA8_UNORM,
                usage: crate::rhi::TextureUsage::COLOR_ATTACHMENT
                    | crate::rhi::TextureUsage::SAMPLED,
                sample_count: crate::rhi::SampleCount::X1,
                dimensions: crate::rhi::TextureDimensions::D2,
                ..Default::default()
            }));
            let tex = self.ssr_texture.take();
            match tex {
                Some(tex) => {
                    self.ssr_view = Some(tex.create_view(Default::default()));
                    self.ssr_texture = Some(tex);
                }
                None => {
                    eprintln!("[render] lighting: SSR texture creation failed; view not built");
                    return;
                }
            }
        }

        // Create render pass for lighting
        let mut attachments = vec![
            // Output (lighting)
            AttachmentDescription {
                format: output_format,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
            },
        ];

        let mut color_attachments = vec![AttachmentReference {
            attachment: 0,
            layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
        }];

        // Add SSAO attachment if enabled
        if self.config.use_ssao {
            attachments.push(AttachmentDescription {
                format: crate::rhi::Format::R8_UNORM,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
            });
            color_attachments.push(AttachmentReference {
                attachment: 1,
                layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
            });
        }

        // Add SSR attachment if enabled
        if self.config.use_ssr {
            attachments.push(AttachmentDescription {
                format: crate::rhi::Format::RGBA8_UNORM,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
            });
            color_attachments.push(AttachmentReference {
                attachment: (attachments.len().saturating_sub(1)) as u32,
                layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
            });
        }

        let subpass = SubpassDescription {
            pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
            color_attachments,
            depth_stencil_attachment: None, // No depth for lighting pass
            ..Default::default()
        };

        let render_pass_desc = RenderPassDesc {
            attachments,
            subpasses: vec![subpass],
            dependencies: vec![],
        };

        self.render_pass = Some(crate::rhi::RenderPass::new(render_pass_desc));

        // Create pipeline
        self.pipeline = self.create_pipeline(device);

        // Create framebuffer
        //
        // Bug в„–186: the SSAO/SSR views were unwrapped whenever the config flag
        // was set, even though the flag only says "wanted" вЂ” the view exists
        // only if the texture was created. A config that enabled SSAO while
        // texture creation failed therefore aborted the process instead of
        // degrading. Each optional view is now checked and simply omitted.
        let Some(output_view) = self.output_view.clone() else {
            eprintln!("[render] lighting: output view missing; framebuffer not rebuilt");
            return;
        };
        let mut fb_attachments = vec![FramebufferAttachment {
            texture_view: output_view,
            layer: 0,
            mip_level: 0,
        }];
        if self.config.use_ssao {
            match self.ssao_view.clone() {
                Some(view) => fb_attachments.push(FramebufferAttachment {
                    texture_view: view,
                    layer: 0,
                    mip_level: 0,
                }),
                None => {
                    eprintln!("[render] lighting: SSAO enabled but its view is missing; attaching without it");
                }
            }
        }
        if self.config.use_ssr {
            match self.ssr_view.clone() {
                Some(view) => fb_attachments.push(FramebufferAttachment {
                    texture_view: view,
                    layer: 0,
                    mip_level: 0,
                }),
                None => {
                    eprintln!("[render] lighting: SSR enabled but its view is missing; attaching without it");
                }
            }
        }

        let Some(render_pass) = self.render_pass.clone() else {
            eprintln!("[render] lighting: render pass missing; framebuffer not rebuilt");
            return;
        };

        self.framebuffer = Some(crate::rhi::Framebuffer::new(FramebufferDesc {
            render_pass,
            attachments: fb_attachments,
            width,
            height,
            layers: 1,
        }));
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        _scene: &mut Scene,
        resources: &HashMap<String, GraphResource>,
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

        let width = context.resolution.width;
        let height = context.resolution.height;

        let mut clear_values = vec![crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0)];
        if self.config.use_ssao {
            clear_values.push(crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0));
        }
        if self.config.use_ssr {
            clear_values.push(crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0));
        }

        encoder.begin_render_pass(
            render_pass,
            framebuffer,
            crate::rhi::Rect2D {
                offset: crate::rhi::Offset2D { x: 0, y: 0 },
                extent: crate::rhi::Extent2D { width, height },
            },
            &clear_values,
            1.0,
            0,
        );

        encoder.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
        encoder.set_scissor(0, 0, width, height);

        encoder.bind_pipeline(pipeline);

        let mut binding = 0;
        for resource in resources.values() {
            if let Some(view) = resource.view() {
                encoder.bind_texture(view, binding);
                binding += 1;
            }
        }

        encoder.draw_indexed_instanced(3, 1, 0, 0, 0);

        encoder.end_render_pass();
    }

    fn resize(&mut self, _width: u32, _height: u32) {
        // Recreate textures with new dimensions
        // Implementation similar to GBufferPass
    }

    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }
}

impl Default for LightingPass {
    fn default() -> Self {
        Self::new(LightingConfig::default())
    }
}


