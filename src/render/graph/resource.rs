//! Graph Resources
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::types::ResourceUsage;
use crate::rhi::{
    Buffer, BufferDesc, BufferUsage, Format, SampleCount, SharingMode, Texture, TextureDesc,
    TextureDimensions, TextureUsage, TextureView,
};

/// Resource type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Texture,
    Buffer,
    Sampler,
}

/// A resource in the render graph
pub struct GraphResource {
    /// Resource name
    pub name: String,
    /// Resource type
    pub resource_type: ResourceType,
    /// Usage flags
    pub usage: ResourceUsage,
    /// Width (for textures)
    pub width: u32,
    /// Height (for textures)
    pub height: u32,
    /// Depth (for 3D textures)
    pub depth: u32,
    /// Format (for textures)
    pub format: Format,
    /// Size in bytes (for buffers)
    pub size: u64,
    /// The actual RHI texture (if created)
    pub texture: Option<Texture>,
    /// The actual RHI buffer (if created)
    pub buffer: Option<Buffer>,
    /// Texture view (if needed)
    pub view: Option<TextureView>,
    /// Is this resource transient (created and destroyed every frame)?
    pub transient: bool,
    /// Current lifetime (for transient resources)
    pub lifetime: u32,
}

impl GraphResource {
    /// Create a new texture resource
    pub fn new_texture(
        name: String,
        width: u32,
        height: u32,
        format: Format,
        usage: ResourceUsage,
    ) -> Self {
        Self {
            name,
            resource_type: ResourceType::Texture,
            usage,
            width,
            height,
            depth: 1,
            format,
            size: 0,
            texture: None,
            buffer: None,
            view: None,
            transient: false,
            lifetime: 0,
        }
    }

    /// Create a new buffer resource
    pub fn new_buffer(name: String, size: u64, usage: ResourceUsage) -> Self {
        Self {
            name,
            resource_type: ResourceType::Buffer,
            usage,
            width: 0,
            height: 0,
            depth: 0,
            format: Format::RGBA8_UNORM,
            size,
            texture: None,
            buffer: None,
            view: None,
            transient: false,
            lifetime: 0,
        }
    }

    /// Create a new graph resource (generic constructor)
    pub fn new(
        name: String,
        resource_type: ResourceType,
        usage: ResourceUsage,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            name,
            resource_type,
            usage,
            width,
            height,
            depth: 1,
            format: Format::RGBA8_UNORM,
            size: 0,
            texture: None,
            buffer: None,
            view: None,
            transient: false,
            lifetime: 0,
        }
    }

    /// Create the actual RHI resource
    pub fn create(&mut self, _device: &crate::rhi::Device) {
        match self.resource_type {
            ResourceType::Texture => {
                let rhi_usage = match self.usage {
                    ResourceUsage::RenderTarget | ResourceUsage::ColorAttachment => {
                        TextureUsage::COLOR_ATTACHMENT | TextureUsage::SAMPLED
                    }
                    ResourceUsage::DepthStencil => {
                        TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::SAMPLED
                    }
                    ResourceUsage::Sampled => TextureUsage::SAMPLED,
                    ResourceUsage::Storage => TextureUsage::STORAGE | TextureUsage::SAMPLED,
                };

                self.texture = Some(Texture::new(TextureDesc {
                    width: self.width,
                    height: self.height,
                    depth: self.depth,
                    mip_levels: 1,
                    array_layers: 1,
                    format: self.format,
                    usage: rhi_usage,
                    sample_count: SampleCount::X1,
                    dimensions: TextureDimensions::D2,
                    sharing_mode: SharingMode::Exclusive,
                    queue_family_indices: vec![],
                }));

                // Create view if needed
                //
                // Bug №186: a RenderTarget deliberately gets no view, and the
                // `!=` guard above honours that — but for every other usage the
                // texture was unwrapped, so a failed `create_texture` aborted
                // the process. Build the view from the value we hold.
                if self.usage != ResourceUsage::RenderTarget {
                    let tex = self.texture.take();
                    match tex {
                        Some(tex) => {
                            self.view =
                                Some(tex.create_view(crate::rhi::TextureViewDesc::default()));
                            self.texture = Some(tex);
                        }
                        None => {
                            eprintln!("[render] graph: texture creation failed; resource has no view");
                        }
                    }
                }
            }
            ResourceType::Buffer => {
                let rhi_usage = match self.usage {
                    ResourceUsage::RenderTarget => unreachable!(),
                    ResourceUsage::ColorAttachment => unreachable!(),
                    ResourceUsage::DepthStencil => unreachable!(),
                    ResourceUsage::Sampled => BufferUsage::UNIFORM | BufferUsage::STORAGE,
                    ResourceUsage::Storage => BufferUsage::STORAGE,
                };

                self.buffer = Some(Buffer::new(BufferDesc {
                    size: self.size,
                    usage: rhi_usage,
                    ..Default::default()
                }));
            }
            ResourceType::Sampler => {
                // Samplers are created separately
            }
        }
    }

    /// Destroy the RHI resource
    pub fn destroy(&mut self) {
        self.texture = None;
        self.buffer = None;
        self.view = None;
    }

    /// Get the texture view (if available)
    pub fn view(&self) -> Option<&TextureView> {
        self.view.as_ref()
    }

    /// Get the texture (if available)
    pub fn texture(&self) -> Option<&Texture> {
        self.texture.as_ref()
    }

    /// Get the buffer (if available)
    pub fn buffer(&self) -> Option<&Buffer> {
        self.buffer.as_ref()
    }

    /// Check if resource is a texture
    pub fn is_texture(&self) -> bool {
        self.resource_type == ResourceType::Texture
    }

    /// Check if resource is a buffer
    pub fn is_buffer(&self) -> bool {
        self.resource_type == ResourceType::Buffer
    }

    /// Set as transient
    pub fn set_transient(&mut self, transient: bool) {
        self.transient = transient;
    }

    /// Increment lifetime
    pub fn increment_lifetime(&mut self) {
        self.lifetime += 1;
    }

    /// Check if resource should be destroyed (for transient resources)
    pub fn should_destroy(&self) -> bool {
        self.transient && self.lifetime > 1
    }
}
