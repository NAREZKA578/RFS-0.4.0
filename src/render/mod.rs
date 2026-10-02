//! # Render Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! High-level rendering system built on top of RHI (Rendering Hardware Interface).
//! Supports Vulkan, Direct3D 12, Direct3D 11, and OpenGL backends.
//!
//! ## Architecture
//!
//! The render module uses a **RenderGraph** approach with the following passes:
//! - GBuffer Pass (Deferred Rendering)
//! - Shadow Pass (Cascaded Shadow Maps)
//! - Lighting Pass (PBR Lighting)
//! - Transparent Pass (Water, Smoke, Fire)
//! - Water Pass (Reflections, Refractions)
//! - Post-Process Pass (Bloom, HDR, FXAA, Motion Blur, DoF)
//! - UI Pass (HUD, Menu)
//!
//! ## Features
//! - PBR Materials
//! - GPU-Driven Particle System
//! - Dynamic Water with Waves and Reflections
//! - Cascaded Shadow Mapping
//! - Screen Space Effects (SSAO, SSR)
//! - LOD System
//! - Frustum/Occlusion Culling
//! - Instanced Rendering
//!
//! ## Settings
//!
//! All graphics settings can be toggled at runtime:
//! - Ray Tracing (if supported by hardware)
//! - Shadow Quality (Low/Medium/High/Ultra)
//! - Anti-Aliasing (None/FXAA/TAA)
//! - Post-Processing Effects
//! - LOD Distance
//! - Particle Count

pub mod camera;
pub mod core;
pub mod diagnostics;
pub mod effects;
pub mod graph;
pub mod lighting;
pub mod materials;
pub mod meshes;
pub mod particles;
pub mod passes;
pub mod postprocess;
pub mod scene;
pub mod textures;
pub mod ui;
pub mod water;

// Re-export commonly used types
pub use camera::{Camera, CameraController, OrthographicCamera, PerspectiveCamera};
pub use core::{RenderContext, RenderStats, Renderer, RendererConfig};
pub use effects::{
    EffectManager, EffectQuality, EffectSettings, GlobalEffectSettings, VisualEffectType,
};
pub use graph::RenderGraph;
pub use lighting::{DirectionalLight, Light, LightProbe, PointLight, ShadowConfig, SpotLight};
pub use materials::{Material, MaterialLibrary, PbrMaterial};
pub use meshes::{Mesh, MeshLibrary};
pub use particles::{
    EmitterShape, ParticleEffect, ParticleEmitter, ParticleEmitterConfig, ParticleSystem,
};
pub use postprocess::{
    BloomConfig, BloomEffect, DepthOfFieldEffect, DofConfig, FXAAEffect, HDREffect, HdrConfig,
    MotionBlurConfig, MotionBlurEffect, PostProcessConfig, PostProcessManager, ToneMapping,
};
pub use scene::{Entity, Renderable, Scene, Transform};
pub use textures::{SamplerDesc, Texture, TextureLibrary};
pub use ui::{HUDAlignment, Menu, MenuItem, MenuItemType, MenuType, Minimap, HUD};
pub use water::{WaterConfig, WaterRenderer, WaveMethod, WaveSystem};

/// Maximum number of ships visible on screen
pub const MAX_VISIBLE_SHIPS: usize = 30;

/// Maximum number of players in a match
pub const MAX_PLAYERS: usize = 200;

/// Maximum LOD levels
pub const MAX_LOD_LEVELS: usize = 4;
