//! RHI Configuration Settings
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::types::GraphicsApi;
use serde::{Deserialize, Serialize};

/// Main configuration for RHI initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RhiConfig {
    /// Primary graphics API backend
    pub api: GraphicsApi,

    /// Enable validation layers
    pub validation: bool,

    /// Enable debug markers
    pub debug_markers: bool,

    /// Enable ray tracing
    pub enable_ray_tracing: bool,

    /// Enable mesh shading
    pub enable_mesh_shading: bool,

    /// Enable variable rate shading
    pub enable_vrs: bool,

    /// Max frames in flight
    pub max_frames_in_flight: u32,

    /// Preferred GPU name
    pub preferred_gpu: Option<String>,

    /// Memory allocator config
    pub memory_allocator: MemoryAllocatorConfig,
}

impl Default for RhiConfig {
    fn default() -> Self {
        Self {
            api: GraphicsApi::Vulkan,
            validation: false,
            debug_markers: false,
            enable_ray_tracing: false,
            enable_mesh_shading: false,
            enable_vrs: false,
            max_frames_in_flight: 2,
            preferred_gpu: None,
            memory_allocator: MemoryAllocatorConfig::default(),
        }
    }
}

/// Memory allocator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAllocatorConfig {
    pub use_buddy_for_buffers: bool,
    pub use_linear_for_transient: bool,
    pub use_pool_for_small: bool,
    pub small_resource_threshold: u64,
    pub min_alignment: u64,
}

impl Default for MemoryAllocatorConfig {
    fn default() -> Self {
        Self {
            use_buddy_for_buffers: true,
            use_linear_for_transient: true,
            use_pool_for_small: true,
            small_resource_threshold: 1024 * 1024,
            min_alignment: 16,
        }
    }
}

/// Validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    pub gpu_assisted: bool,
    pub shader_validation: bool,
    pub sync_validation: bool,
    pub min_severity: ValidationSeverity,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            gpu_assisted: false,
            shader_validation: true,
            sync_validation: true,
            min_severity: ValidationSeverity::Error,
        }
    }
}

/// Validation severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[derive(Default)]
pub enum ValidationSeverity {
    Info,
    Warning,
    #[default]
    Error,
    Verbose,
}

