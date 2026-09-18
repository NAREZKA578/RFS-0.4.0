//! Ray Tracing Pipeline
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::resource::buffer::Buffer;
use crate::shader::ShaderModule;

/// Ray tracing shader type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RayTracingShaderType {
    RayGen,
    Miss,
    ClosestHit,
    AnyHit,
    Intersection,
    Callable,
}

/// Ray tracing shader
#[derive(Debug, Clone)]
pub struct RayTracingShader {
    pub module: ShaderModule,
    pub entry_point: String,
}

/// Ray tracing pipeline shader stage
#[derive(Debug, Clone)]
pub struct RayTracingPipelineShaderStage {
    pub ty: RayTracingShaderType,
    pub shader: RayTracingShader,
}

/// Ray tracing pipeline description
#[derive(Debug, Clone, Default)]
pub struct RayTracingPipelineDesc {
    pub stages: Vec<RayTracingPipelineShaderStage>,
    pub max_recursion_depth: u32,
}

/// Ray tracing pipeline
pub struct RayTracingPipeline {
    desc: RayTracingPipelineDesc,
}

impl RayTracingPipeline {
    pub fn new(desc: RayTracingPipelineDesc) -> Self {
        Self { desc }
    }

    pub fn desc(&self) -> &RayTracingPipelineDesc {
        &self.desc
    }
}

/// Shader binding table
pub struct ShaderBindingTable {
    pub raygen: Buffer,
    pub raygen_offset: u32,
    pub raygen_stride: u32,
    pub miss: Buffer,
    pub miss_offset: u32,
    pub miss_stride: u32,
    pub hit: Buffer,
    pub hit_offset: u32,
    pub hit_stride: u32,
    pub callable: Option<Buffer>,
    pub callable_offset: u32,
    pub callable_stride: u32,
}

/// Shader binding table entry
#[derive(Debug, Clone)]
pub struct ShaderBindingTableEntry {
    pub buffer: Buffer,
    pub offset: u32,
    pub size: u32,
    pub stride: u32,
}
