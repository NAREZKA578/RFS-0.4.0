//! UI Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This pass renders the 2D user interface (HUD, menus, etc.)
//! Uses orthographic projection and alpha blending.

use crate::render::camera::Camera;
use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::graph::types::ResourceUsage;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::pipeline::graphics::PrimitiveTopology;
use crate::rhi::{
    AttachmentDescription, CommandEncoder, LoadOp, Pipeline, PipelineBindPoint, Rect2D, StoreOp,
    SubpassDescription, TextureLayout, TextureView,
};
use crate::rhi::command::pass::render::{AttachmentReference, RenderPassDesc};
use crate::rhi::core::Device;
use crate::rhi::types::{Extent2D, Offset2D, SampleCount};
use std::collections::HashMap;

/// UI configuration
#[derive(Debug, Clone)]
pub struct UIConfig {
    pub resolution_scale: f32,
    pub enable_hud: bool,
    pub enable_menu: bool,
    pub enable_minimap: bool,
}

impl Default for UIConfig {
    fn default() -> Self {
        Self {
            resolution_scale: 1.0,
            enable_hud: true,
            enable_menu: true,
            enable_minimap: true,
        }
    }
}

/// UI pass
pub struct UIPass {
    base: BaseRenderPass,
    config: UIConfig,
    /// Orthographic camera for UI
    ui_camera: crate::render::camera::OrthographicCamera,
    /// UI pipeline
    ui_pipeline: Option<Pipeline>,
    /// Output texture (swapchain target)
    output_view: Option<TextureView>,
    /// Render pass
    render_pass: Option<crate::rhi::RenderPass>,
    /// Framebuffer
    framebuffer: Option<crate::rhi::Framebuffer>,
}

impl UIPass {
    pub fn new(config: UIConfig) -> Self {
        let mut base = BaseRenderPass::new("ui");

        // UI pass depends on all previous passes
        base.add_dependency("water", "water", ResourceUsage::Sampled);
        base.add_dependency("transparent", "final", ResourceUsage::Sampled);

        // UI pass outputs to swapchain
        base.add_output("swapchain");

        // Create orthographic camera for UI
        let ui_camera = crate::render::camera::OrthographicCamera::new(
            0.0, 1920.0, // left, right
            1080.0, 0.0, // bottom, top (inverted for screen space)
            -1.0, 1.0, // near, far
        );

        Self {
            base,
            config,
            ui_camera,
            ui_pipeline: None,
            output_view: None,
            render_pass: None,
            framebuffer: None,
        }
    }
}

impl UIPass {
    pub fn config(&self) -> &UIConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut UIConfig {
        &mut self.config
    }

    pub fn camera(&self) -> &crate::render::camera::OrthographicCamera {
        &self.ui_camera
    }

    pub fn camera_mut(&mut self) -> &mut crate::render::camera::OrthographicCamera {
        &mut self.ui_camera
    }

    fn create_pipeline(&self, device: &Device) -> Option<Pipeline> {
        let spirv = include_bytes!("../../../Tool/client_tests/assets/shaders/ui.spv");
        let vs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("ui_vs".into()),
        });
        let fs_module = device.create_shader_module(&crate::rhi::ShaderModuleDesc {
            code: spirv.to_vec(),
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: Some("fs_main".into()),
            name: Some("ui_fs".into()),
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
                        stride: 32,
                        input_rate: crate::rhi::VertexInputRate::Vertex,
                    }],
                    attributes: vec![
                        crate::rhi::VertexAttribute {
                            binding: 0,
                            location: 0,
                            format: crate::rhi::Format::R32G32_SFLOAT,
                            offset: 0,
                        },
                        crate::rhi::VertexAttribute {
                            binding: 0,
                            location: 1,
                            format: crate::rhi::Format::R32G32_SFLOAT,
                            offset: 8,
                        },
                        crate::rhi::VertexAttribute {
                            binding: 0,
                            location: 2,
                            format: crate::rhi::Format::RGBA32_SFLOAT,
                            offset: 16,
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
                depth_stencil_state: None,
                ..Default::default()
            },
            &render_pass,
            &[],
        );
        pipeline.ok()
    }
}

impl UIPass {
    /// Render HUD elements
    fn render_hud(&mut self, _encoder: &mut CommandEncoder, _context: &RenderContext, _scene: &Scene) {
        // HUD elements:
        // - Crosshair
        // - Ship information (HP, speed, fuel)
        // - Weapon information (ammo, cooldown)
        // - Player information (health, role)
        // - Objectives
        // - Compass

        // For now, this is a placeholder
    }

    /// Render menu
    fn render_menu(
        &mut self,
        _encoder: &mut CommandEncoder,
        _context: &RenderContext,
        _scene: &Scene,
    ) {
        // Menu elements:
        // - Main menu
        // - Settings menu
        // - Ship selection
        // - Loadout selection

        // For now, this is a placeholder
    }

    /// Render minimap
    fn render_minimap(
        &mut self,
        _encoder: &mut CommandEncoder,
        _context: &RenderContext,
        _scene: &Scene,
    ) {
        // Minimap rendering:
        // - Top-down view of the battlefield
        // - Ship positions
        // - Objectives
        // - Fog of war

        // For now, this is a placeholder
    }
}

impl RenderPass for UIPass {
    fn name(&self) -> &str {
        self.base.name()
    }

    fn dependencies(&self) -> &[PassDependency] {
        self.base.dependencies()
    }

    fn outputs(&self) -> &[String] {
        self.base.outputs()
    }

    fn initialize(&mut self, device: &Device, context: &RenderContext) {
        let width = context.resolution.width;
        let height = context.resolution.height;

        // Update camera to match resolution
        self.ui_camera
            .set_viewport(0.0, width as f32, height as f32, 0.0);

        // Get the swapchain view for the image acquired for THIS frame.
        //
        // Bug в„–186: this took `images().first()`, i.e. always image 0, and
        // `unwrap_or_default()` silently produced a default (invalid) view when
        // the swapchain had no images. Both are fixed: the acquired index comes
        // from the context, and a missing image aborts the pass instead of
        // attaching a bogus view to the framebuffer.
        let images = context.swapchain.images();
        match images.get(context.swapchain_image_index as usize) {
            Some(image) => {
                self.output_view = Some(image.view.clone());
            }
            None => {
                eprintln!(
                    "[render] ui: swapchain image {} not available (have {}); pass skipped",
                    context.swapchain_image_index,
                    images.len()
                );
                self.output_view = None;
                return;
            }
        }

        // Create render pass (simple color pass with blending)
        let attachments = vec![
            // Output (swapchain)
            AttachmentDescription {
                format: context.swapchain.desc().format,
                samples: SampleCount::X1,
                load_op: LoadOp::Load, // Load from previous pass
                store_op: StoreOp::Store,
                stencil_load_op: LoadOp::DontCare,
                stencil_store_op: StoreOp::DontCare,
                initial_layout: TextureLayout::TransferDstOptimal,
                final_layout: TextureLayout::TransferSrcOptimal,
            },
        ];

        let subpass = SubpassDescription {
            pipeline_bind_point: PipelineBindPoint::Graphics,
            color_attachments: vec![AttachmentReference {
                attachment: 0,
                layout: TextureLayout::ColorAttachmentOptimal,
            }],
            depth_stencil_attachment: None,
            ..Default::default()
        };

        let render_pass_desc = RenderPassDesc {
            attachments,
            subpasses: vec![subpass],
            dependencies: vec![],
        };

        self.render_pass = Some(crate::rhi::RenderPass::new(render_pass_desc));

        // Create UI pipeline
        self.ui_pipeline = self.create_pipeline(device);

        // Create framebuffer with swapchain image
        if let Some(ref view) = self.output_view {
            let render_pass = self.render_pass.clone().unwrap_or_default();
            self.framebuffer = Some(crate::rhi::Framebuffer::new(
                crate::rhi::FramebufferDesc {
                    render_pass,
                    attachments: vec![crate::rhi::FramebufferAttachment {
                        texture_view: view.clone(),
                        layer: 0,
                        mip_level: 0,
                    }],
                    width,
                    height,
                    layers: 1,
                },
            ));
        }
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

        let width = context.resolution.width;
        let height = context.resolution.height;

        // Get the scene's UI elements
        // In the actual implementation, the scene would have UI elements
        // For now, we'll just render a simple HUD

        // Begin render pass
        if let (Some(render_pass), Some(framebuffer), Some(_output_view)) =
            (&self.render_pass, &self.framebuffer, &self.output_view)
        {
            // Clear color is not used because we're loading the existing content
            encoder.begin_render_pass(
                render_pass,
                framebuffer,
                Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: Extent2D { width, height },
                },
                &[], // No color clears
                1.0,
                0,
            );

            // Set viewport
            encoder.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
            encoder.set_scissor(0, 0, width, height);

            // Bind UI pipeline
            if let Some(pipeline) = &self.ui_pipeline {
                encoder.bind_pipeline(pipeline);
            }

            // Update camera matrices
            let _view_matrix = self.ui_camera.view_matrix();
            let _proj_matrix = self.ui_camera.projection_matrix();

            // Render HUD elements
            if self.config.enable_hud {
                self.render_hud(encoder, context, scene);
            }

            // Render menu (if active)
            if self.config.enable_menu {
                self.render_menu(encoder, context, scene);
            }

            // Render minimap
            if self.config.enable_minimap {
                self.render_minimap(encoder, context, scene);
            }

            // End render pass
            encoder.end_render_pass();
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        // Update camera
        self.ui_camera
            .set_viewport(0.0, width as f32, height as f32, 0.0);

        // Note: We need to recreate the framebuffer with the new swapchain image
        // This is handled by the swapchain resize
    }

    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }
}

impl Default for UIPass {
    fn default() -> Self {
        Self::new(UIConfig::default())
    }
}


