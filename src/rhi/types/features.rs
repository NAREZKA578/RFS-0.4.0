//! Device Features
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Features {
    // Shaders
    pub geometry_shader: bool,
    pub tessellation_shader: bool,
    pub mesh_shader: bool,
    pub shader_float64: bool,
    pub shader_int64: bool,

    // Multi-sampling
    pub multi_draw_indirect: bool,

    // Depth/Stencil
    pub depth_bounds: bool,
    pub depth_clamp: bool,

    // Texture
    pub texture_compression_bc: bool,
    pub texture_compression_astc: bool,
    pub texture_compression_etc2: bool,
    pub sampler_anisotropy: bool,

    // Storage
    pub storage_buffer: bool,
    pub storage_image: bool,

    // Compute
    pub compute: bool,
    pub indirect_compute: bool,

    // Ray Tracing
    pub ray_tracing: bool,
    pub ray_query: bool,

    // Variable Rate Shading
    pub variable_rate_shading: bool,

    // Conservative Rasterization
    pub conservative_raster: bool,

    // Sparse Binding
    pub sparse_binding: bool,

    // Memory
    pub memory_budget: bool,

    // Descriptors
    pub descriptor_indexing: bool,

    // Buffer Device Address
    pub buffer_device_address: bool,
}
