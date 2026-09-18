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
    _depth_pipeline: Option<GraphicsPipeline>,
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
            _depth_pipeline: None,
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

    fn initialize(&mut self, _device: &crate::rhi::Device, _context: &RenderContext) {
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
        // This would require a shadow shader
        // For now, we'll leave it as None
    }

    fn execute(
        &mut self,
        _encoder: &mut CommandEncoder,
        _context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) {
        if !self.base.is_enabled() {
            return;
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