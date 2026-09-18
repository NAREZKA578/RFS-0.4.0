//! Pipeline Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

pub mod compute;
pub mod graphics;
pub mod mesh_shading;
pub mod ray_tracing;
pub mod state;

pub use compute::*;
pub use graphics::*;
pub use mesh_shading::*;
pub use ray_tracing::*;
pub use state::*;

/// Convenience alias so the render layer can use `Pipeline`/`PipelineDesc`.
pub type Pipeline = crate::pipeline::graphics::GraphicsPipeline;

/// Convenience alias so the render layer can use `Pipeline`/`PipelineDesc`.
pub type PipelineDesc = crate::pipeline::graphics::GraphicsPipelineDesc;
