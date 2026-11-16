//! Bindless Resources
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::resource::{Sampler, TextureView};

/// Bindless descriptor set description
#[derive(Debug, Clone, Default)]
pub struct BindlessDescriptorSetDesc {
    pub max_uniform_buffers: u32,
    pub max_storage_buffers: u32,
    pub max_sampled_textures: u32,
    pub max_storage_textures: u32,
    pub max_samplers: u32,
    pub max_acceleration_structures: u32,
}

/// Bindless descriptor set
pub struct BindlessDescriptorSet {
    // Bug №788: kept for the device-side bindless allocation, which is not
    // connected — creation is the neutral API's only live half today. Kept per
    // the dead-is-not-removed policy.
    #[allow(dead_code)]
    desc: BindlessDescriptorSetDesc,
}

impl BindlessDescriptorSet {
    pub fn new(desc: BindlessDescriptorSetDesc) -> Self {
        Self { desc }
    }
}

/// Bindless texture array
pub struct BindlessTextureArray {
    pub textures: Vec<TextureView>,
    pub samplers: Vec<Sampler>,
}
