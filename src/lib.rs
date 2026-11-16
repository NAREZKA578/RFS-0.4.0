//! # RFS Client Library
//!
//! **CLIENT-SIDE, WITH ONE SERVER LINK**
//!
//! This library contains the client-side systems for the RFS naval combat
//! game. It includes:
//! - RHI (Rendering Hardware Interface) - Low-level graphics API abstraction
//! - Render Module - High-level rendering system built on RHI
//! - Net Module - The bridge that connects the game loop to the dedicated
//!   server (`crate::net`); the only place in this crate that speaks the
//!   server protocol. Everything else stays client-side.
//!
//! ## Supported Graphics APIs (the honest list, bug №796)
//! - Vulkan 1.3 — the only backend that renders; Windows-only.
//! - Direct3D 12 / Direct3D 11 — skeletons returning NotSupported.
//! - OpenGL — stub, slated for removal by owner decision (bug №719).
//!
//! ## Architecture
//! The render graph declares many passes; the live frame today is drawn
//! by the geometry pass with the water and sky draws on top of it. The
//! other passes listed in `src/render/mod.rs` are declared scaffolding
//! the live configuration does not execute yet.

// Re-export RHI module
pub mod rhi;

// Re-export Render module
pub mod render;

// The server link: a bridge between the game loop and `rfs_net`'s client.
// The render thread never awaits; the net thread owns the reactor.
pub mod net;

// The server's world as the render loop sees it: the snapshot -> scene
// mapping that makes connecting actually change what is on the screen.
pub mod net_world;

// Re-export common types from render for convenience
pub use render::{
    Camera, CameraController, Entity, Light, LightProbe, Material, MaterialLibrary, Mesh,
    MeshLibrary, OrthographicCamera, ParticleEmitter, ParticleSystem,
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
