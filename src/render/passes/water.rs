//! Water Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This pass renders the water surface with:
//! - Dynamic waves (FFT or Gerstner)
//! - Reflections (planar or cube map)
//! - Refractions
//! - Foam (based on depth and velocity)
//! - Interaction with objects (ships, projectiles)

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::graph::types::ResourceUsage;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::pipeline::graphics::PrimitiveTopology;
use crate::rhi::{CommandEncoder, Pipeline, TextureView};
use std::collections::HashMap;

/// Water configuration
#[derive(Debug, Clone)]
pub struct WaterConfig {
    /// Wave simulation method
    pub wave_method: WaveMethod,
    /// Wave parameters
    pub wave_scale: f32,
    pub wave_speed: f32,
    pub wave_height: f32,
    /// Reflection quality
    pub reflection_quality: ReflectionQuality,
    /// Refraction enabled
    pub refraction_enabled: bool,
    /// Foam enabled
    pub foam_enabled: bool,
    /// Tessellation factor
    pub tessellation_factor: f32,
    /// Water color
    pub color: [f32; 4],
    /// Water depth (for refraction)
    pub depth: f32,
    /// Water clarity (transparency)
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

/// Wave simulation method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveMethod {
    /// No waves (flat water)
    Flat,
    /// Simple sine waves
    Simple,
    /// Gerstner waves (more realistic)
    Gerstner,
    /// FFT-based wave simulation (most realistic, GPU-intensive)
    FFT,
}

/// Reflection quality
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionQuality {
    /// No reflections
    None,
    /// Low quality (512x512)
    Low,
    /// Medium quality (1024x1024)
    Medium,
    /// High quality (2048x2048)
    High,
    /// Ultra quality (4096x4096 or ray traced)
    Ultra,
}

/// Water pass
pub struct WaterPass {
    base: BaseRenderPass,
    config: WaterConfig,
/// Water mesh
    _water_mesh: Option<crate::render::meshes::Mesh>,
    /// Reflection render target
    reflection_texture: Option<crate::rhi::Texture>,
    reflection_view: Option<TextureView>,
    /// Refraction render target
    refraction_texture: Option<crate::rhi::Texture>,
    refraction_view: Option<TextureView>,
/// Normal map for waves
    _normal_map: Option<crate::rhi::Texture>,
    _normal_map_view: Option<TextureView>,
    /// Foam texture
    _foam_texture: Option<crate::rhi::Texture>,
    _foam_view: Option<TextureView>,
    /// Water pipeline
    water_pipeline: Option<Pipeline>,
/// Reflection pipeline
    _reflection_pipeline: Option<Pipeline>,
    /// Render pass for reflection
    _reflection_render_pass: Option<crate::rhi::RenderPass>,
    /// Framebuffer for reflection
    _reflection_framebuffer: Option<crate::rhi::Framebuffer>,
    /// Render pass for water
    water_render_pass: Option<crate::rhi::RenderPass>,
    /// Framebuffer for water
    water_framebuffer: Option<crate::rhi::Framebuffer>,
    /// Output texture
    output_texture: Option<crate::rhi::Texture>,
    output_view: Option<TextureView>,
}

impl WaterPass {
    pub fn new(config: WaterConfig) -> Self {
        let mut base = BaseRenderPass::new("water");

        // Water pass depends on GBuffer depth
        base.add_dependency("gbuffer", "depth", ResourceUsage::Sampled);

        // Water pass depends on lighting
        base.add_dependency("lighting", "lighting", ResourceUsage::Sampled);

        // Water pass outputs water color
        base.add_output("water");

        // Water pass also outputs reflection (if enabled)
        if matches!(
            config.reflection_quality,
            ReflectionQuality::Low
                | ReflectionQuality::Medium
                | ReflectionQuality::High
                | ReflectionQuality::Ultra
        ) {
            base.add_output("water_reflection");
        }

        Self {
            base,
            config,
            _water_mesh: None,
            reflection_texture: None,
            reflection_view: None,
            refraction_texture: None,
            refraction_view: None,
_normal_map: None,
            _normal_map_view: None,
            _foam_texture: None,
            _foam_view: None,
            water_pipeline: None,
_reflection_pipeline: None,
            _reflection_render_pass: None,
            _reflection_framebuffer: None,
            water_render_pass: None,
            water_framebuffer: None,
            output_texture: None,
            output_view: None,
        }
    }

    pub fn config(&self) -> &WaterConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut WaterConfig {
        &mut self.config
    }

    fn create_pipeline(&self, device: &crate::rhi::Device) -> Option<Pipeline> {
        let spirv = include_bytes!("../../../Tool/client_tests/assets/shaders/water.spv");
        let vs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("water_vs".into()),
        });
        let fs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("fs_main".into()),
            name: Some("water_fs".into()),
        });

        let render_pass = self.water_render_pass.clone()?;
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
                depth_stencil_state: Some(crate::rhi::DepthStencilState::enabled()),
                ..Default::default()
            },
            &render_pass,
            &[],
        );
        pipeline.ok()
    }
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

    fn initialize(&mut self, device: &crate::rhi::Device, context: &RenderContext) {
        let width = context.resolution.width;
        let height = context.resolution.height;

        // Create water mesh (full-screen quad or tessellated mesh)
        // This would be a mesh covering the entire water surface

        // Create reflection texture
        let reflection_resolution = match self.config.reflection_quality {
            ReflectionQuality::None => 0,
            ReflectionQuality::Low => 512,
            ReflectionQuality::Medium => 1024,
            ReflectionQuality::High => 2048,
            ReflectionQuality::Ultra => 4096,
        };

        if reflection_resolution > 0 {
            self.reflection_texture = Some(device.create_texture(
                reflection_resolution,
                reflection_resolution,
                1,
                crate::rhi::Format::RGBA32_SFLOAT,
                crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
                1,
            ));
            // Bug в„–186: build the view from the value we hold instead of
            // unwrapping it back out of the Option.
            let tex = self.reflection_texture.take();
            match tex {
                Some(tex) => {
                    self.reflection_view = Some(tex.create_view(Default::default()));
                    self.reflection_texture = Some(tex);
                }
                None => {
                    eprintln!("[render] water: reflection texture is missing; reflections stay off");
                }
            }
        }

        // Create refraction texture
        if self.config.refraction_enabled {
            self.refraction_texture = Some(device.create_texture(
                width,
                height,
                1,
                crate::rhi::Format::RGBA32_SFLOAT,
                crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
                1,
            ));
            let tex = self.refraction_texture.take();
            match tex {
                Some(tex) => {
                    self.refraction_view = Some(tex.create_view(Default::default()));
                    self.refraction_texture = Some(tex);
                }
                None => {
                    eprintln!("[render] water: refraction texture is missing; refractions stay off");
                }
            }
        }

        // Create normal map (procedural or loaded)
        // This would be a texture with wave normals

        // Create foam texture
        // This would be a noise texture for foam

        // Create output texture
        self.output_texture = Some(device.create_texture(
            width,
            height,
            1,
            crate::rhi::Format::RGBA32_SFLOAT,
            crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            1,
        ));
        let tex = self.output_texture.take();
        match tex {
            Some(tex) => {
                self.output_view = Some(tex.create_view(Default::default()));
                self.output_texture = Some(tex);
            }
            None => {
                eprintln!("[render] water: output texture is missing; resize aborted");
                return;
            }
        }

        // Create render passes
        // This would include:
        // 1. Reflection render pass (renders scene from water's POV)
        // 2. Refraction render pass (renders underwater scene)
        // 3. Water render pass (combines reflection, refraction, waves)

        // For now, we'll just create a simple water render pass
        let attachments = vec![
            // Output
            crate::rhi::AttachmentDescription {
                format: crate::rhi::Format::RGBA32_SFLOAT,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::Undefined,
                final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
            },
            // Depth
            crate::rhi::AttachmentDescription {
                format: crate::rhi::Format::D32_SFLOAT,
                samples: crate::rhi::SampleCount::X1,
                load_op: crate::rhi::LoadOp::Clear,
                store_op: crate::rhi::StoreOp::Store,
                stencil_load_op: crate::rhi::LoadOp::DontCare,
                stencil_store_op: crate::rhi::StoreOp::DontCare,
                initial_layout: crate::rhi::TextureLayout::Undefined,
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

        self.water_render_pass = Some(device.create_render_pass(&render_pass_desc));

        // Create water pipeline
        self.water_pipeline = self.create_pipeline(device);

        // Create framebuffer
        let depth_texture = device.create_texture(
            width,
            height,
            1,
            crate::rhi::Format::D32_SFLOAT,
            crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT,
            1,
        );
        let depth_view = depth_texture.create_view(Default::default());

        // Bug в„–186: the render pass and output view were unwrapped here.
        let (Some(render_pass), Some(output_view)) =
            (self.water_render_pass.clone(), self.output_view.clone())
        else {
            eprintln!("[render] water: render pass or output view missing; framebuffer not rebuilt");
            return;
        };
        self.water_framebuffer = Some(device.create_framebuffer(
            &render_pass,
            vec![output_view, depth_view],
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

        // Step 1: Render reflection (if enabled)
        if self.config.reflection_quality != ReflectionQuality::None {
            self.render_reflection(encoder, context, scene, resources);
        }

        // Step 2: Render refraction (if enabled)
        if self.config.refraction_enabled {
            self.render_refraction(encoder, context, scene, resources);
        }

        // Step 3: Render water surface
        if let (Some(render_pass), Some(framebuffer)) =
            (&self.water_render_pass, &self.water_framebuffer)
        {
            // Get GBuffer depth
            let _depth_tex = super::base::get_texture(resources, "depth");

            // Get lighting
            let _lighting_tex = super::base::get_texture(resources, "lighting");

            // Begin water render pass
encoder.begin_render_pass(
                render_pass,
                framebuffer,
                crate::rhi::Rect2D {
                    offset: crate::rhi::Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D { width, height },
                },
                &[crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0)], // Clear to transparent
                1.0,
                0,
            );

            // Set viewport
            encoder.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
            encoder.set_scissor(0, 0, width, height);

            // Bind water pipeline
            if let Some(pipeline) = &self.water_pipeline {
                encoder.bind_pipeline(pipeline);
            }

            // Update wave simulation (if animated)
            self.update_waves(context);

            // Render water mesh
            // This would:
            // 1. Bind water mesh
            // 2. Bind normal map
            // 3. Bind reflection texture
            // 4. Bind refraction texture
            // 5. Bind foam texture
            // 6. Set water parameters (color, depth, clarity, etc.)
            // 7. Draw water mesh

            // For now, just a placeholder

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

impl WaterPass {
    /// Render reflection
    fn render_reflection(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) {
        let Some(ref view) = self.reflection_view else {
            return;
        };
        let Some(ref texture) = self.reflection_texture else {
            return;
        };
        let width = texture.width();
        let height = texture.height();

        let cam_pos = context.camera_position();
        let water_level = 0.0f32;
        let refl_pos = glam::Vec3::new(cam_pos.x, 2.0 * water_level - cam_pos.y, cam_pos.z);

        let _ = (scene, refl_pos, view, width, height, encoder);
    }

    /// Render refraction
    fn render_refraction(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) {
        let Some(ref view) = self.refraction_view else {
            return;
        };
        let Some(ref texture) = self.refraction_texture else {
            return;
        };
        let width = texture.width();
        let height = texture.height();

        let cam_pos = context.camera_position();

        let _ = (scene, cam_pos, view, width, height, encoder);
    }

    /// Update wave simulation
    fn update_waves(&mut self, context: &RenderContext) {
        let _time = context.total_time.as_secs_f32();
    }
}

impl Default for WaterPass {
    fn default() -> Self {
        Self::new(WaterConfig::default())
    }
}



