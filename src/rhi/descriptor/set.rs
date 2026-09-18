//! Descriptor Set
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::layout::DescriptorSetLayout;
use crate::resource::acceleration::AccelerationStructure;
use crate::resource::{Buffer, Sampler, TextureView};

/// Descriptor set
#[derive(Debug, Clone)]
pub struct DescriptorSet {
    layout: DescriptorSetLayout,
}

impl DescriptorSet {
    pub fn new(layout: DescriptorSetLayout) -> Self {
        Self { layout }
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
    Buffer(Buffer, u64, u64),
    Texture(TextureView, Option<Sampler>),
    AccelerationStructure(AccelerationStructure),
    InlineUniformBlock(Vec<u8>),
}
