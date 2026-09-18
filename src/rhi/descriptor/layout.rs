//! Descriptor Set Layout
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::resource::sampler::Sampler;
use crate::types::*;
use bitflags::bitflags;

/// Descriptor type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DescriptorType {
    UniformBuffer,
    DynamicUniformBuffer,
    StorageBuffer,
    DynamicStorageBuffer,
    SampledTexture,
    StorageTexture,
    UniformTexelBuffer,
    StorageTexelBuffer,
    CombinedTextureSampler,
    InputAttachment,
    InlineUniformBlock,
    AccelerationStructure,
}

/// Descriptor binding
#[derive(Debug, Clone)]
pub struct DescriptorBinding {
    pub binding: u32,
    pub ty: DescriptorType,
    pub count: u32,
    pub stages: ShaderStage,
    pub immutable_samplers: Option<Vec<Sampler>>,
}

/// Descriptor set layout description
#[derive(Debug, Clone, Default)]
pub struct DescriptorSetLayoutDesc {
    pub bindings: Vec<DescriptorBinding>,
}

// Descriptor set layout flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DescriptorSetLayoutFlags: u32 {
        const DESCRIPTOR_BUFFER = 1 << 0;
        const PUSH_DESCRIPTOR = 1 << 1;
        const UPDATE_AFTER_BIND = 1 << 2;
    }
}

/// Descriptor set layout
#[derive(Debug, Clone)]
pub struct DescriptorSetLayout {
    desc: DescriptorSetLayoutDesc,
}

impl DescriptorSetLayout {
    pub fn new(desc: DescriptorSetLayoutDesc) -> Self {
        Self { desc }
    }

    pub fn desc(&self) -> &DescriptorSetLayoutDesc {
        &self.desc
    }

    /// Returns the bindings in declaration order.
    pub fn bindings(&self) -> &[DescriptorBinding] {
        &self.desc.bindings
    }

    /// Returns the binding with the given index.
    pub fn binding(&self, binding: u32) -> Option<&DescriptorBinding> {
        self.desc.bindings.iter().find(|b| b.binding == binding)
    }

    /// Returns the number of declared bindings.
    pub fn binding_count(&self) -> usize {
        self.desc.bindings.len()
    }

    /// Returns the total number of descriptors required by all bindings.
    pub fn total_descriptors(&self) -> u32 {
        self.desc.bindings.iter().map(|b| b.count).sum()
    }
}
