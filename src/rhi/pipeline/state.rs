//! Pipeline State
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::descriptor::layout::DescriptorSetLayout;
use crate::types::*;

/// Pipeline layout
#[derive(Debug, Clone, Default)]
pub struct PipelineLayout {
    pub descriptor_set_layouts: Vec<DescriptorSetLayout>,
    pub push_constant_ranges: Vec<PushConstantRange>,
}

impl PipelineLayout {
    /// Returns the total number of bytes occupied by all push constant ranges
    /// (assuming non-overlapping ranges).
    pub fn push_constant_total_size(&self) -> u32 {
        self.push_constant_ranges.iter().map(|r| r.size).sum()
    }

    /// Returns the number of bound descriptor set layouts.
    pub fn descriptor_set_count(&self) -> usize {
        self.descriptor_set_layouts.len()
    }

    /// Returns `true` when the layout has no ranges or descriptor sets – i.e.
    /// it is a trivially empty layout.
    pub fn is_empty(&self) -> bool {
        self.descriptor_set_layouts.is_empty() && self.push_constant_ranges.is_empty()
    }
}

/// Push constant range
#[derive(Debug, Clone, Default)]
pub struct PushConstantRange {
    pub stage_flags: ShaderStage,
    pub offset: u32,
    pub size: u32,
}
