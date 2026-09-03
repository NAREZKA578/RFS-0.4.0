//! Graphics backend implementations.
//!
//! Each backend implements the RHI traits for a specific graphics API.
//! Форк: только OpenGL (рабочий) и Vulkan (недоделан). DX11/DX12-заглушки
//! из донора не переносились.

pub mod opengl;
#[cfg(feature = "vulkan")]
pub mod vulkan;
