//! Descriptor Set
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::layout::DescriptorSetLayout;
use crate::resource::acceleration::AccelerationStructure;
use crate::resource::{Buffer, GpuResource, Sampler, TextureView};

/// Descriptor set
#[derive(Debug, Clone)]
pub struct DescriptorSet {
    layout: DescriptorSetLayout,
    pub(crate) backend: Option<GpuResource>,
}

impl DescriptorSet {
    pub fn new(layout: DescriptorSetLayout) -> Self {
        Self {
            layout,
            backend: None,
        }
    }

    /// Attaches native handles produced by the active backend. The `memory`
    /// field of the attachment carries the owning descriptor pool handle.
    pub(crate) fn set_backend(&mut self, backend: GpuResource) {
        self.backend = Some(backend);
    }

    /// Returns `true` when the set has been allocated on the backend.
    pub fn has_gpu_backing(&self) -> bool {
        self.backend.is_some()
    }

    pub(crate) fn backend(&self) -> Option<GpuResource> {
        self.backend
    }

    pub fn layout(&self) -> &DescriptorSetLayout {
        &self.layout
    }
}

/// Descriptor write
#[derive(Debug, Clone)]
pub struct DescriptorWrite {
    pub dst_set: DescriptorSet,
    pub dst_binding: u32,
    pub dst_array_element: u32,
    pub descriptors: Vec<DescriptorInfo>,
}

/// Descriptor info
#[derive(Debug, Clone)]
pub enum DescriptorInfo {
    Sampler(Sampler),
    Buffer(Buffer, u64, u64),
    Texture(TextureView, Option<Sampler>),
    AccelerationStructure(AccelerationStructure),
    InlineUniformBlock(Vec<u8>),
}
