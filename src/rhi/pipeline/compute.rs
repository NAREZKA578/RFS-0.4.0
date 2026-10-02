//! Compute Pipeline
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::PipelineShaderStage;
use crate::resource::GpuResource;

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

    /// Returns the entry point of the compute shader.
    pub fn entry_point(&self) -> &str {
        &self.shader.entry_point
    }

    /// Returns the compute shader module.
    pub fn shader_module(&self) -> &crate::shader::module::ShaderModule {
        &self.shader.module
    }
}

/// Compute pipeline
#[derive(Debug, Clone)]
pub struct ComputePipeline {
    desc: ComputePipelineDesc,
    pub(crate) backend: Option<GpuResource>,
}

impl ComputePipeline {
    pub fn new(desc: ComputePipelineDesc) -> Self {
        Self {
            desc,
            backend: None,
        }
    }

    /// Attaches native handles produced by the active backend. The `memory`
    /// field of the attachment carries the pipeline layout handle.
    pub(crate) fn set_backend(&mut self, backend: GpuResource) {
        self.backend = Some(backend);
    }

    /// Returns `true` when the pipeline has a native backend handle.
    pub fn has_gpu_backing(&self) -> bool {
        self.backend.is_some()
    }

    pub(crate) fn backend(&self) -> Option<GpuResource> {
        self.backend
    }

    pub fn desc(&self) -> &ComputePipelineDesc {
        &self.desc
    }
    pub fn shader_stage(&self) -> &PipelineShaderStage {
        &self.desc.shader
    }
}
