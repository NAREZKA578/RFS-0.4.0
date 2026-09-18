//! Render Core Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod context;
pub mod renderer;
pub mod resource_manager;
pub mod settings;
pub mod stats;

pub use context::RenderContext;
pub use renderer::Renderer;
pub use resource_manager::ResourceManager;
pub use settings::{
    GraphicsSettings, GraphicsSettingsManager, QualityPreset, RenderApi, RendererConfig,
    RendererSettings,
};
pub use stats::RenderStats;
