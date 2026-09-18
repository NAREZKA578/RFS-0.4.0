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
use crate::rhi::{CommandEncoder, Pipeline, TextureView};
use std::cmp::Ordering;
use std::collections::HashMap;

/// Transparent object for sorting
#[derive(Debug, Clone)]
pub struct TransparentObject {
    pub entity_id: usize,
    pub distance: f32,
    pub blend_mode: BlendMode,
}

/// Blend modes for transparent objects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// Standard alpha blending (src * alpha + dst * (1 - alpha))
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

impl Default for BlendMode {
    fn default() -> Self {
        Self::Alpha
    }
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
        }
    }

    pub fn config(&self) -> &TransparentConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut TransparentConfig {
        &mut self.config
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
        self.output_view = Some(
            self.output_texture
                .as_ref()
                .unwrap()
                .create_view(Default::default()),
        );

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
        self.depth_view = Some(
            self.depth_texture
                .as_ref()
                .unwrap()
                .create_view(Default::default()),
        );

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

        // Create framebuffer
        let fb_attachments = vec![
            self.output_view.as_ref().unwrap().clone(),
            self.depth_view.as_ref().unwrap().clone(),
        ];

        self.framebuffer = Some(device.create_framebuffer(
            &self.render_pass.as_ref().unwrap(),
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

            // Render each transparent object
            for obj in &transparent_objects {
                if let Some(_entity) = scene.get_entity(obj.entity_id) {
                    // Get the appropriate pipeline for this blend mode
                    if let Some(pipeline) = self.pipelines.get(&obj.blend_mode) {
                        encoder.bind_pipeline(pipeline);
                    }

                    // Bind entity's mesh and material
                    // Draw the entity

                    // Placeholder: actual rendering would go here
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
