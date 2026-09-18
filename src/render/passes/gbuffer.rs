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
use crate::rhi::{
    AttachmentDescription, AttachmentReference, CommandEncoder, FramebufferAttachment,
    FramebufferDesc, GraphicsPipeline, RenderPassDesc, SubpassDescription,
};

/// GBuffer render pass
pub struct GBufferPass {
    base: BaseRenderPass,
    _pipeline: Option<GraphicsPipeline>,
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
            _pipeline: None,
            position_texture: None,
            normal_texture: None,
            albedo_texture: None,
            material_texture: None,
            depth_texture: None,
            render_pass: None,
            framebuffer: None,
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

    fn initialize(&mut self, _device: &crate::rhi::Device, context: &RenderContext) {
        let width = context.resolution.width;
        let height = context.resolution.height;

        // Create GBuffer textures
        let position_format = crate::rhi::Format::RGBA32_SFLOAT;
        let normal_format = crate::rhi::Format::RGBA32_SFLOAT;
        let albedo_format = crate::rhi::Format::RGBA8_UNORM;
        let material_format = crate::rhi::Format::RGBA8_UNORM;
        let depth_format = crate::rhi::Format::D32_SFLOAT;

        self.position_texture = Some(crate::rhi::Texture::new(crate::rhi::TextureDesc {
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
        }));

        self.normal_texture = Some(crate::rhi::Texture::new(crate::rhi::TextureDesc {
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
        }));

        self.albedo_texture = Some(crate::rhi::Texture::new(crate::rhi::TextureDesc {
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
        }));

        self.material_texture = Some(crate::rhi::Texture::new(crate::rhi::TextureDesc {
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
        }));

        self.depth_texture = Some(crate::rhi::Texture::new(crate::rhi::TextureDesc {
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
        }));

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
        let position_view = self
            .position_texture
            .as_ref()
            .unwrap()
            .create_view(Default::default());
        let normal_view = self
            .normal_texture
            .as_ref()
            .unwrap()
            .create_view(Default::default());
        let albedo_view = self
            .albedo_texture
            .as_ref()
            .unwrap()
            .create_view(Default::default());
        let material_view = self
            .material_texture
            .as_ref()
            .unwrap()
            .create_view(Default::default());
        let depth_view = self
            .depth_texture
            .as_ref()
            .unwrap()
            .create_view(Default::default());

        self.framebuffer = Some(crate::rhi::Framebuffer::new(FramebufferDesc {
            render_pass: self.render_pass.as_ref().unwrap().clone(),
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
        // This would require shader modules to be loaded
        // For now, we'll leave it as None and create it when shaders are available
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
