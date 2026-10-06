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
                // Bug №102: this used to be a `match` over `ResourceUsage`, which only worked
                // while the type was a single-valued enum. As a set it can hold
                // several usages at once — and `graph.rs` really does declare
                // `COLOR_ATTACHMENT | SAMPLED` — so the usage is accumulated bit
                // by bit instead of selected. Matching on the whole value would
                // miss every combination, which is the bug all over again.
                let mut rhi_usage = TextureUsage::empty();
                if self.usage.contains(ResourceUsage::RENDER_TARGET)
                    || self.usage.contains(ResourceUsage::COLOR_ATTACHMENT)
                {
                    rhi_usage |= TextureUsage::COLOR_ATTACHMENT;
                }
                if self.usage.contains(ResourceUsage::DEPTH_STENCIL) {
                    rhi_usage |= TextureUsage::DEPTH_STENCIL_ATTACHMENT;
                }
                if self.usage.contains(ResourceUsage::SAMPLED) {
                    rhi_usage |= TextureUsage::SAMPLED;
                }
                if self.usage.contains(ResourceUsage::STORAGE) {
                    rhi_usage |= TextureUsage::STORAGE;
                }
                if rhi_usage.is_empty() {
                    // A resource nobody declared a use for cannot be created
                    // without usage bits, and Vulkan would reject a zero-usage
                    // image. Saying so beats a validation error about an image
                    // that was never asked for.
                    eprintln!(
                        "[render] graph: resource {:?} has no usage declared; \
                         not creating a texture",
                        self.name
                    );
                    return;
                }
                // Sampled is implied whenever a texture is also written, because
                // every pass that writes a target reads it back later in the
                // chain (lighting, transparent, UI all declare a Sampled
                // dependency on what an earlier pass produced).
                if rhi_usage != TextureUsage::SAMPLED
                    && (rhi_usage.contains(TextureUsage::COLOR_ATTACHMENT)
                        || rhi_usage.contains(TextureUsage::DEPTH_STENCIL_ATTACHMENT))
                {
                    rhi_usage |= TextureUsage::SAMPLED;
                }

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
                // Bug №102: accumulated like the texture case above. A buffer that is both a
                // storage target and something sampled needs both usage bits, and
                // the old `match` could not express that at all.
                let mut rhi_usage = BufferUsage::empty();
                if self.usage.contains(ResourceUsage::SAMPLED) {
                    // A sampled buffer is read through a descriptor, which for a
                    // uniform is a UNIFORM buffer and for anything else is read
                    // as storage. Keeping both matches what the old code did for
                    // the single `Sampled` case.
                    rhi_usage |= BufferUsage::UNIFORM | BufferUsage::STORAGE;
                }
                if self.usage.contains(ResourceUsage::STORAGE) {
                    rhi_usage |= BufferUsage::STORAGE;
                }
                if self.usage.contains(ResourceUsage::RENDER_TARGET)
                    || self.usage.contains(ResourceUsage::COLOR_ATTACHMENT)
                    || self.usage.contains(ResourceUsage::DEPTH_STENCIL)
                {
                    // Attachments are textures, never buffers. Saying so beats the
                    // `unreachable!()` the old code had here, which would abort
                    // the process on a data-driven mistake rather than on a bug.
                    eprintln!(
                        "[render] graph: buffer {:?} declares an attachment usage \
                         ({}) but buffers cannot be attachments; not creating it",
                        self.name, self.usage
                    );
                    return;
                }
                if rhi_usage.is_empty() {
                    eprintln!(
                        "[render] graph: buffer {:?} has no usage declared; \
                         not creating a buffer",
                        self.name
                    );
                    return;
                }

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
