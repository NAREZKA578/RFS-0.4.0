//! Полный RHI-слой движка: трейты + бэкенды OpenGL и Vulkan.
//!
//! Прямой перенос `RFS-0.3/src/rhi` (трейты — целиком, бэкенды `opengl` и
//! `vulkan` — как есть, со статусом «OpenGL рабочий / Vulkan недоделан»).
//! DX11/DX12-заглушки не переносились.
//!
//! ```rust,no_run
//! // OpenGL (дефолт):
//! // let backend = rfs_engine_rhi::OpenGLBackend::new(gl_context)?;
//! // Vulkan (фича `vulkan`):
//! // let backend = rfs_engine_rhi::VulkanBackend::new(&desc)?;
//! ```

pub mod rhi;

pub use rhi::*;
