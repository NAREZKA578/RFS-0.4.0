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

    /// Create the actual RHI resource.
    ///
    /// Bug №652: this used to build CPU-side descriptions and never call the
    /// backend, so every graph resource was a descriptor with `texture: None`.
    /// The passes that look resources up at execute time — `transparent` asks
    /// for "depth", `water` for "depth" and "lighting", `lighting` binds every
    /// view it can find — found nothing, and the graph's resource map never fed
    /// a single pass.
    ///
    /// The resource is created through the device, so a failure is a failure
    /// and not a stand-in that looks like a texture. The size comes from the
    /// declaration, which is sized from the physical extent (Bug №651).
    pub fn create(&mut self, device: &crate::rhi::Device) {
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
                // Transfer source, so a frame can be read back.
                //
                // Without it, reading a graph resource is a validation error
                // (`VUID-vkCmdCopyImageToBuffer-srcImage-00186`: the image was
                // created without TRANSFER_SRC) rather than a read. And reading a
                // frame is not a test-only luxury: the offscreen PNG path
                // exists to capture frames, and a screenshot of the final image
                // is exactly this.
                rhi_usage |= TextureUsage::TRANSFER_SRC;

                let desc = TextureDesc {
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
                };
                match device.create_texture_from_desc(&desc) {
                    Ok(texture) => {
                        // A render target deliberately gets no view: the
                        // producer builds the framebuffer, and the view it
                        // needs. Every other usage is read back later, so it
                        // needs one.
                        if self.usage != ResourceUsage::RenderTarget {
                            match device
                                .create_texture_view(&texture, texture.create_default_view().desc())
                            {
                                Ok(view) => self.view = Some(view),
                                Err(e) => eprintln!(
                                    "[render] graph: resource {:?} has no view: {e}",
                                    self.name
                                ),
                            }
                        }
                        self.texture = Some(texture);
                    }
                    Err(e) => eprintln!(
                        "[render] graph: resource {:?} could not be created: {e}",
                        self.name
                    ),
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

                // Bug №208: the refusal reasons live in `creation_error` and
                // are shared with the CPU-side test. A data-driven mistake is
                // a logged refusal, not an abort and not a silent no-op.
                if let Some(reason) = self.creation_error() {
                    eprintln!("[render] graph: {reason}");
                    return;
                }

                match device.create_buffer_from_desc(&BufferDesc {
                    size: self.size,
                    usage: rhi_usage,
                    ..Default::default()
                }) {
                    Ok(buffer) => self.buffer = Some(buffer),
                    Err(e) => eprintln!(
                        "[render] graph: buffer {:?} could not be created: {e}",
                        self.name
                    ),
                }
            }
            ResourceType::Sampler => {
                // Bug №208: this branch was a silent no-op — the constructor
                // made not creating look like a decision nobody made. The
                // refusal goes through the same channel as the buffer branch.
                if let Some(reason) = self.creation_error() {
                    eprintln!("[render] graph: {reason}");
                }
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

    /// CPU-side refusal reasons for `create`, so graph tests can probe the
    /// paths that used to be an `unreachable!()` without owning a device.
    /// `create` reports these and does not materialize the resource. Bug №208.
    pub(crate) fn creation_error(&self) -> Option<String> {
        match self.resource_type {
            ResourceType::Buffer => {
                if self.usage.contains(ResourceUsage::RENDER_TARGET)
                    || self.usage.contains(ResourceUsage::COLOR_ATTACHMENT)
                    || self.usage.contains(ResourceUsage::DEPTH_STENCIL)
                {
                    // Attachments are textures, never buffers.
                    return Some(format!(
                        "buffer {:?} declares an attachment usage ({}) but \
                         buffers cannot be attachments; not creating it",
                        self.name, self.usage
                    ));
                }
                if !self.usage.contains(ResourceUsage::SAMPLED)
                    && !self.usage.contains(ResourceUsage::STORAGE)
                {
                    return Some(format!(
                        "buffer {:?} has no usage declared; not creating a buffer",
                        self.name
                    ));
                }
                None
            }
            ResourceType::Texture => {
                // The texture branch refuses only inside `create` (its usage
                // set is accumulated there); no CPU-side reason here.
                None
            }
            ResourceType::Sampler => {
                // A sampler is created by a device, never by the graph; the
                // constructor must not make that look like a working branch.
                Some(format!(
                    "sampler {:?} is not created by the graph; samplers belong \
                     to the device, so buffer() and texture() will stay None",
                    self.name
                ))
            }
        }
    }
}

#[cfg(test)]
mod bug_208 {
    use super::*;

    // Bug №208 (NC): a buffer with an attachment usage hit an `unreachable!()`
    // on a reachable public-API path.
    #[test]
    fn buffer_with_attachment_usage_is_refused_not_aborted() {
        let resource =
            GraphResource::new_buffer("probe".to_string(), 64, ResourceUsage::DepthStencil);
        assert!(
            resource.creation_error().is_some(),
            "an attachment usage on a buffer must be refused"
        );
    }

    #[test]
    fn buffer_without_any_usage_is_refused() {
        let resource =
            GraphResource::new_buffer("probe".to_string(), 64, ResourceUsage::NONE);
        assert!(resource.creation_error().is_some());
    }

    #[test]
    fn buffer_with_data_usage_passes_the_cpu_gate() {
        let resource =
            GraphResource::new_buffer("probe".to_string(), 64, ResourceUsage::Sampled);
        assert!(resource.creation_error().is_none());
    }

    // Bug №208 (NC): a sampler resource used to be a silent no-op — the caller
    // had no way to learn that nothing was made.
    #[test]
    fn sampler_creation_is_reported_not_silent() {
        let resource = GraphResource::new(
            "probe".to_string(),
            ResourceType::Sampler,
            ResourceUsage::Sampled,
            1,
            1,
        );
        let reason = resource
            .creation_error()
            .expect("a sampler must be refused with a reason");
        assert!(reason.contains("sampler"));
    }
}
