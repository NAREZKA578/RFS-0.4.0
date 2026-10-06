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

/// Create a GBuffer attachment through the device.
///
/// `Texture::new` is a CPU-side stub with no GPU backing. A framebuffer built
/// from its view therefore fails much later, at draw time, as "framebuffer
/// attachment has no GPU backing" — the error surfaces in a different place
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
        // Bug №186: each of these five `as_ref().unwrap()` calls aborted the
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
        // This would require shader modules to be loaded
        // For now, we'll leave it as None and create it when shaders are available
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        _context: &mut RenderContext,
        _scene: &mut Scene,
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
        encoder.begin_render_pass(
            render_pass,
            framebuffer,
            crate::rhi::Rect2D {
                offset: crate::rhi::Offset2D { x: 0, y: 0 },
                extent: crate::rhi::Extent2D {
                    width: _context.resolution.width,
                    height: _context.resolution.height,
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
