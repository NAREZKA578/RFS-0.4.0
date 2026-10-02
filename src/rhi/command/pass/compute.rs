//! Compute Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

/// Compute pass (placeholder)
pub struct ComputePass {
    // In Vulkan/D3D12/D3D11 this is just CommandEncoder in COMPUTE pipeline bind point
    // In OpenGL this may be a separate pass
}

impl Default for ComputePass {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputePass {
    pub fn new() -> Self {
        Self {}
    }
}
