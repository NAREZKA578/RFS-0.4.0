//! Device Limits
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::SampleCount;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Limits {
    // Texture
    pub max_texture_size: u32,
    pub max_texture_layers: u32,
    pub max_texture_mips: u32,

    // Buffer
    pub max_buffer_size: u64,
    pub max_uniform_buffer_size: u64,
    pub max_storage_buffer_size: u64,

    // Push Constants
    pub max_push_constants_size: u32,

    // Descriptors
    pub max_bound_descriptor_sets: u32,
    pub max_per_stage_descriptors: u32,

    // Attachments
    pub max_color_attachments: u32,
    pub max_sample_count: SampleCount,

    // Viewport
    pub max_viewports: u32,

    // Compute
    pub max_compute_work_group_count: [u32; 3],
    pub max_compute_work_group_size: [u32; 3],
}
