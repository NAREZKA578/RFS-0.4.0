//! RHI (Rendering Hardware Interface) — абстракция над графическими API.
//!
//! Форк из RFS-0.3 (`src/rhi`): перенесены трейты и два бэкенда,
//! OpenGL 3.3+ (рабочий) и Vulkan 1.3+ (недоделан — см. TODO ниже).
//! DX11/DX12-заглушки из донора сюда НЕ переносились.
//!
//! TODO для доделки (из RFS-0.3/DOC TODO.md и BUGS.md):
//! - Vulkan: present-queue, uniform-дескрипторы (сейчас fallback через push),
//!   instance-буфер, pipeline cache, in-flight 3-4 + ring persistent,
//!   реальный `gpu_info`, runtime fallback на OpenGL.

pub mod backend;
pub mod backends;
pub mod buffer;
pub mod command;
pub mod device;
pub mod pipeline;
pub mod shader;
pub mod surface;
pub mod swapchain;
pub mod sync;
pub mod texture;
pub mod types;

// Re-exports для удобства
pub use backend::{Backend, BackendDescriptor, BackendType, GpuInfo};
pub use backends::opengl::{
    OpenGLBackend, OpenGLBuffer, OpenGLCommandEncoder, OpenGLDevice, OpenGLFence, OpenGLPipeline,
    OpenGLSemaphore, OpenGLShader, OpenGLSurface, OpenGLSwapChain, OpenGLTexture,
};
#[cfg(feature = "vulkan")]
pub use backends::vulkan::{
    VulkanBackend, VulkanBuffer, VulkanCommandEncoder, VulkanDevice, VulkanFence, VulkanPipeline,
    VulkanSemaphore, VulkanShader, VulkanSurface, VulkanSwapChain, VulkanTexture,
};
pub use buffer::{Buffer, BufferDescriptor, BufferUsage};
pub use command::CommandEncoder;
pub use device::{Device, DeviceDescriptor};
pub use pipeline::{BlendMode, CullMode, FrontFace, Pipeline, PipelineDescriptor, PolygonMode};
pub use shader::{Shader, ShaderDescriptor, ShaderStage};
pub use surface::{Surface, SurfaceDescriptor};
pub use swapchain::{SwapChain, SwapChainDescriptor};
pub use sync::{Fence, Semaphore};
pub use texture::{
    Filter, Texture, TextureDescriptor, TextureFormat, TextureType, TextureUsage, WrapMode,
};
pub use types::{
    Color, Extent2D, Extent3D, Offset2D, Offset3D, Rect, VertexAttribute, VertexFormat,
    VertexLayout,
};
