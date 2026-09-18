//! Statistics
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

/// GPU statistics
#[derive(Debug, Clone, Default)]
pub struct GpuStats {
    pub frame_count: u64,
    pub draw_calls: u64,
    pub triangles: u64,
    pub vertices: u64,
    pub compute_dispatches: u64,
    pub ray_traces: u64,
    pub pipeline_creations: u64,
    pub buffer_creations: u64,
    pub texture_creations: u64,
    pub gpu_time_ns: u64,
    pub cpu_time_ns: u64,
}

/// Frame statistics
#[derive(Debug, Clone, Default)]
pub struct FrameStats {
    pub frame_index: u64,
    pub draw_calls: u32,
    pub triangles: u32,
    pub vertices: u32,
    pub compute_dispatches: u32,
    pub gpu_time_ns: u64,
    pub cpu_time_ns: u64,
}

/// Pipeline statistics
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    pub draw_calls: u64,
    pub triangles: u64,
    pub invocations: u64,
}
