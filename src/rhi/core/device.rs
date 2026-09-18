//! Device and PhysicalDevice
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::command::pass::render::{
    Framebuffer, FramebufferAttachment, FramebufferDesc, RenderPass, RenderPassDesc,
};
use crate::error::RhiResult;
use crate::pipeline::graphics::{GraphicsPipeline, GraphicsPipelineDesc};
use crate::resource::{
    AddressMode, Buffer, BufferDesc, FilterMode, Sampler, SamplerDesc, Texture, TextureDesc,
    TextureView,
};
use crate::types::*;
use serde::{Deserialize, Serialize};

/// Physical device type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PhysicalDeviceType {
    Discrete,
    Integrated,
    Virtual,
    Cpu,
}

/// Memory properties
#[derive(Debug, Clone, Default)]
pub struct MemoryProperties {
    pub memory_types: Vec<MemoryType>,
    pub memory_heaps: Vec<MemoryHeap>,
}

/// Memory type
#[derive(Debug, Clone)]
pub struct MemoryType {
    pub flags: MemoryPropertyFlags,
    pub heap_index: u32,
}

/// Memory heap
#[derive(Debug, Clone)]
pub struct MemoryHeap {
    pub size: u64,
    pub flags: MemoryHeapFlags,
}

/// Queue family
#[derive(Debug, Clone)]
pub struct QueueFamily {
    pub index: u32,
    pub flags: QueueFlags,
    pub queue_count: u32,
    pub timestamp_valid_bits: u32,
    pub min_image_transfer_granularity: Extent3D,
}

/// Physical GPU device
#[derive(Debug, Clone)]
pub struct PhysicalDevice {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: PhysicalDeviceType,
    pub features: Features,
    pub limits: Limits,
    pub memory_properties: MemoryProperties,
    pub queue_families: Vec<QueueFamily>,
}

impl PhysicalDevice {
    pub fn get_queue_family(&self, flags: QueueFlags) -> Option<&QueueFamily> {
        self.queue_families.iter().find(|q| q.flags.contains(flags))
    }
}

/// Queue
#[derive(Debug, Clone)]
pub struct Queue {
    pub family_index: u32,
    pub index: u32,
    pub flags: QueueFlags,
}

impl Queue {
    pub fn supports_graphics(&self) -> bool {
        self.flags.contains(QueueFlags::GRAPHICS)
    }
    pub fn supports_compute(&self) -> bool {
        self.flags.contains(QueueFlags::COMPUTE)
    }
    pub fn supports_transfer(&self) -> bool {
        self.flags.contains(QueueFlags::TRANSFER)
    }
}

/// Device creation description
#[derive(Debug, Clone, Default)]
pub struct DeviceDesc {
    pub features: Features,
    pub queue_family_indices: Vec<u32>,
    pub validation: bool,
}

/// Logical device
pub struct Device {
    physical_device: PhysicalDevice,
    queues: Vec<Queue>,
}

impl Device {
    pub fn physical_device(&self) -> &PhysicalDevice {
        &self.physical_device
    }
    pub fn queues(&self) -> &[Queue] {
        &self.queues
    }
    pub fn graphics_queue(&self) -> Option<&Queue> {
        self.queues.iter().find(|q| q.supports_graphics())
    }
    pub fn wait_idle(&self) -> RhiResult<()> {
        // No active GPU work is tracked in the stub backends, so the device
        // is always considered idle.
        Ok(())
    }

    /// Create a graphics pipeline from a description.
    pub fn create_graphics_pipeline(&self, desc: &GraphicsPipelineDesc) -> RhiResult<GraphicsPipeline> {
        Ok(GraphicsPipeline::new(desc.clone()))
    }

    /// Create a render pass from a description.
    pub fn create_render_pass(&self, desc: &RenderPassDesc) -> RenderPass {
        RenderPass::new(desc.clone())
    }

    /// Create a framebuffer from the given render pass and texture views.
    pub fn create_framebuffer(
        &self,
        render_pass: &RenderPass,
        attachments: Vec<TextureView>,
        width: u32,
        height: u32,
    ) -> Framebuffer {
        let attachments = attachments
            .into_iter()
            .map(|texture_view| FramebufferAttachment {
                texture_view,
                layer: 0,
                mip_level: 0,
            })
            .collect();
        Framebuffer::new(FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments,
            width,
            height,
            layers: 1,
        })
    }

    /// Create a sampler with uniform wrap/filter modes.
    pub fn create_sampler(&self, wrap_mode: AddressMode, filter_mode: FilterMode, anisotropy: f32) -> Sampler {
        Sampler::new(SamplerDesc {
            mag_filter: filter_mode,
            min_filter: filter_mode,
            mipmap_mode: filter_mode,
            address_mode_u: wrap_mode,
            address_mode_v: wrap_mode,
            address_mode_w: wrap_mode,
            max_anisotropy: anisotropy,
            ..Default::default()
        })
    }

    /// Create a 2D/3D texture (convenience used by the legacy client render code).
    pub fn create_texture(
        &self,
        width: u32,
        height: u32,
        depth: u32,
        format: Format,
        usage: TextureUsage,
        mip_levels: u32,
    ) -> Texture {
        Texture::new(TextureDesc {
            width,
            height,
            depth,
            mip_levels,
            array_layers: 1,
            format,
            usage,
            sample_count: SampleCount::X1,
            dimensions: TextureDimensions::D2,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: Vec::new(),
        })
    }

    /// Create a buffer (convenience used by the legacy client render code).
    pub fn create_buffer(&self, size: u64, usage: BufferUsage, cpu_visible: bool) -> Buffer {
        let memory_flags = if cpu_visible {
            MemoryPropertyFlags::HOST_VISIBLE | MemoryPropertyFlags::HOST_COHERENT
        } else {
            MemoryPropertyFlags::DEVICE_LOCAL
        };
        Buffer::new(BufferDesc {
            size,
            usage,
            memory_flags,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: Vec::new(),
        })
    }

    /// Upload data to a buffer (placeholder).
    pub fn upload_buffer<T>(&self, _buffer: &Buffer, _data: &[T]) {
        // Placeholder for actual upload
    }

    /// Upload data to a texture (placeholder).
    pub fn upload_texture(&self, _texture: &Texture, _data: &[u8]) {
        // Placeholder for actual upload
    }
}
