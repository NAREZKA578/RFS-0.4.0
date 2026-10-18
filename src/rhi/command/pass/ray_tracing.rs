//! Ray Tracing Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

/// Ray tracing pass description
#[derive(Debug, Clone, Default)]
pub struct RayTracingPassDesc {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

/// Ray tracing pass
pub struct RayTracingPass {
    desc: RayTracingPassDesc,
}

impl RayTracingPass {
    pub fn new(desc: RayTracingPassDesc) -> Self {
        Self { desc }
    }

    pub fn desc(&self) -> &RayTracingPassDesc {
        &self.desc
    }
}
