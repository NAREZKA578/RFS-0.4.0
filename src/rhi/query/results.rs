//! Query Results
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

/// Query result
#[derive(Debug, Clone)]
pub enum QueryResult {
    Occlusion(bool),
    Timestamp(u64),
    PipelineStatistics(PipelineStatistics),
    CompactionSize(u64),
}

/// Pipeline statistics
#[derive(Debug, Clone, Default)]
pub struct PipelineStatistics {
    pub input_assembly_vertices: u64,
    pub input_assembly_primitives: u64,
    pub vertex_shader_invocations: u64,
    pub fragment_shader_invocations: u64,
    pub compute_shader_invocations: u64,
}
