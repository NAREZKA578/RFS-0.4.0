//! # RFS Client Library
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This library contains the client-side rendering system for the RFS naval combat game.
//! It includes:
//! - RHI (Rendering Hardware Interface) - Low-level graphics API abstraction
//! - Render Module - High-level rendering system built on RHI
//!
//! ## Supported Graphics APIs
//! - Vulkan 1.3 (Primary, cross-platform)
//! - Direct3D 12 (Windows primary)
//! - Direct3D 11 (Windows fallback)
//! - OpenGL 4.6 (Fallback)
//!
//! ## Architecture
//! The rendering system uses a RenderGraph approach with multiple render passes:
//! - GBuffer Pass (Deferred Rendering)
//! - Shadow Pass (Cascaded Shadow Maps)
//! - Lighting Pass (PBR Lighting)
//! - Transparent Pass (Water, Smoke, Fire)
//! - Water Pass (Reflections, Refractions)
//! - Post-Process Pass (Bloom, HDR, FXAA, Motion Blur, DoF)
//! - UI Pass (HUD, Menu)

// Re-export RHI module
pub mod rhi;

// Re-export Render module
pub mod render;

// Re-export common types from render for convenience
pub use render::{
    Camera, CameraController, Entity, Light, LightProbe, Material, MaterialLibrary, Mesh,
    MeshLibrary, OrthographicCamera, ParticleEffect, ParticleEmitter, ParticleSystem,
    PerspectiveCamera, PostProcessConfig, RenderContext, RenderGraph, RenderStats, Renderer,
    RendererConfig, Scene, ShadowConfig, Transform, WaterConfig, WaterRenderer, HUD,
    MAX_LOD_LEVELS,
};

// Re-export RHI types for convenience
pub use rhi::{
    Backend, Buffer, BufferDesc, CommandBuffer, CommandEncoder, Device, DeviceCaps, DeviceDesc,
    Features, Format, GraphicsApi, GraphicsPipeline, GraphicsPipelineDesc, PhysicalDeviceType,
    PipelineLayout, RhiError, RhiResult, Sampler, SamplerDesc, ShaderModule, ShaderModuleDesc,
    ShaderStage, ShaderTargetEnv, Surface, SurfaceDesc, SwapChain, SwapChainDesc, Texture,
    TextureDesc, TextureUsage,
};

/// Client version
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Game name
pub const GAME_NAME: &str = "RFS Naval Combat";
