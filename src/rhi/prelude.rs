//! Prelude module for convenient RHI imports
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub use crate::rhi::{
    // Config
    RhiConfig, MemoryAllocatorConfig, ValidationConfig, ValidationSeverity,
    // Core
    Rhi, Device, PhysicalDevice, PhysicalDeviceType, Queue, QueueFamily,
    DeviceDesc, DeviceCaps, MemoryProperties, MemoryType, MemoryHeap,
    // Types
    Format, SampleCount, Extent2D, Extent3D, Offset2D, Offset3D, Rect2D,
    Viewport, Scissor, ClearValue, PrimitiveTopology, IndexType,
    SharingMode, TextureDimensions, TextureLayout, TextureAspectFlags,
    FilterMode, AddressMode, CompareOp, BorderColor, PolygonMode, CullMode, FrontFace,
    QueueFlags, BufferUsage, TextureUsage, ShaderStage, PipelineStage, AccessFlags,
    MemoryPropertyFlags, MemoryHeapFlags, DynamicState, PipelineFlags,
    ColorComponentFlags, GraphicsApi,
    // Errors
    RhiError, RhiResult,
    // Backend
    Backend,
};
