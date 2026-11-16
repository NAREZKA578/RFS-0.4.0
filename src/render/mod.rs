//! # Render Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! High-level rendering system built on top of RHI (Rendering Hardware
//! Interface). One backend renders today — Vulkan, Windows-only; the
//! D3D11/D3D12/OpenGL RHI backends are skeletons (bug №796).
//!
//! ## Architecture
//!
//! The render graph declares the passes below; the live frame is drawn by
//! the geometry pass with the water and sky draws on top of it, the rest
//! is scaffolding the live configuration does not execute yet:
//! - Geometry Pass (live)
//! - Water Pass (live)
//! - Sky (live)
//! - GBuffer / Shadow / Lighting / Transparent / Post-Process / UI —
//!   declared, not executed in the live configuration
//!
//! ## Features
//!
//! Declared surface; what executes today is marked (bug №796):
//! - Dynamic Water with Waves and Reflections (live)
//! - PBR Materials
//! - GPU-Driven Particle System
//! - Cascaded Shadow Mapping
//! - Screen Space Effects (SSAO, SSR)
//! - LOD System
//! - Frustum/Occlusion Culling
//! - Instanced Rendering
//!
//! ## Settings
//!
//! The toggles below are the declared settings surface; not every one
//! drives a live pass today:
//! - Shadow Quality (Low/Medium/High/Ultra)
//! - Anti-Aliasing (None/FXAA/TAA)
//! - Post-Processing Effects
//! - LOD Distance
//! - Particle Count

pub mod camera;
pub mod core;
pub mod diagnostics;
pub mod game_loop;
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
pub use graph::RenderGraph;
pub use lighting::{DirectionalLight, Light, LightProbe, PointLight, ShadowConfig, SpotLight};
pub use materials::{Material, MaterialLibrary, PbrMaterial};
pub use meshes::{Mesh, MeshLibrary};
pub use particles::{EmitterShape, ParticleEmitter, ParticleEmitterConfig, ParticleSystem};
pub use postprocess::{
    BloomConfig, BloomEffect, DepthOfFieldEffect, DofConfig, FXAAEffect, HDREffect, HdrConfig,
    MotionBlurConfig, MotionBlurEffect, PostProcessConfig, PostProcessManager, ToneMapping,
};
pub use passes::GeometryPass;
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
