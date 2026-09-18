//! Mesh Shading Pipeline
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::PipelineShaderStage;

/// Mesh pipeline description
#[derive(Debug, Clone, Default)]
pub struct MeshPipelineDesc {
    pub task_shader: Option<PipelineShaderStage>,
    pub mesh_shader: PipelineShaderStage,
    pub fragment_shader: Option<PipelineShaderStage>,
}

/// Mesh pipeline
pub struct MeshPipeline {
    desc: MeshPipelineDesc,
}

impl MeshPipeline {
    pub fn new(desc: MeshPipelineDesc) -> Self {
        Self { desc }
    }

    pub fn desc(&self) -> &MeshPipelineDesc {
        &self.desc
    }
}
