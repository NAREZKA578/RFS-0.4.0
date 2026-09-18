//! Compute Pipeline
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::PipelineShaderStage;

/// Compute pipeline description
#[derive(Debug, Clone, Default)]
pub struct ComputePipelineDesc {
    pub shader: PipelineShaderStage,
}

impl ComputePipelineDesc {
    /// Returns `true` when the description contains at least one stage.
    pub fn is_valid(&self) -> bool {
        self.shader.stage == crate::types::ShaderStage::COMPUTE
    }
}

/// Compute pipeline
#[derive(Debug, Clone)]
pub struct ComputePipeline {
    desc: ComputePipelineDesc,
}

impl ComputePipeline {
    pub fn new(desc: ComputePipelineDesc) -> Self {
        Self { desc }
    }
    pub fn desc(&self) -> &ComputePipelineDesc {
        &self.desc
    }
    pub fn shader_stage(&self) -> &PipelineShaderStage {
        &self.desc.shader
    }
}
