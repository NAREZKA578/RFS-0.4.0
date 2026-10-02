//! UI System for RHI
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! 
//! Combined UI system that provides:
//! - Immediate mode UI rendering
//! - Retained mode UI elements
//! - HUD (Heads-Up Display) support
//! - Menu system
//! - Minimap rendering
//! - Debug overlays
//!
//! # Architecture
//!
//! The UI system is designed to work with all RHI backends (Vulkan, D3D12, D3D11, OpenGL).
//! It provides both immediate mode (for simple debugging) and retained mode (for complex UIs).

pub mod context;
pub mod elements;
pub mod layout;
pub mod pass;
pub mod renderer;
pub mod shaders;

pub use context::{UiContext, UiState, UiModifiers};
pub use elements::{UiElement, UiElementType, UiElementId, UiVisualStyle, UiElementData, SliderOrientation, UiElementBuilder};
pub use layout::{UiLayout, UiConstraints, UiAnchor, UiLayoutResult};
pub use pass::UiRenderPass;
pub use renderer::{UiRenderer, UiRendererConfig, UiRenderStats, UiVertex, UiGeometryBatch, UiFont, UiFontCharacter};
pub use shaders::{UiShaderManager, UiShaderType, UiShaderSource};

/// UI command for immediate mode rendering
#[derive(Debug, Clone, PartialEq)]
pub enum UiCommand {
    Text { content: String, x: f32, y: f32, size: f32, color: [f32; 4], font: Option<String> },
    Rectangle { x: f32, y: f32, width: f32, height: f32, color: [f32; 4], corner_radius: f32 },
    RectangleOutline { x: f32, y: f32, width: f32, height: f32, color: [f32; 4], thickness: f32, corner_radius: f32 },
    Image { texture_id: u64, x: f32, y: f32, width: f32, height: f32, uv_min: [f32; 2], uv_max: [f32; 2], color: [f32; 4] },
    Line { x1: f32, y1: f32, x2: f32, y2: f32, color: [f32; 4], thickness: f32 },
    ProgressBar { x: f32, y: f32, width: f32, height: f32, progress: f32, background_color: [f32; 4], fill_color: [f32; 4], corner_radius: f32 },
    CircularProgress { center_x: f32, center_y: f32, radius: f32, progress: f32, color: [f32; 4], background_color: [f32; 4], thickness: f32 },
    ScissorBegin { x: f32, y: f32, width: f32, height: f32 },
    ScissorEnd,
    GroupBegin { transform: [f32; 6] },
    GroupEnd,
    SetCursor { x: f32, y: f32 },
    PushCursor,
    PopCursor,
}

/// UI configuration
#[derive(Debug, Clone)]
pub struct UiConfig {
    pub screen_width: u32,
    pub screen_height: u32,
    pub dpi_scale: f32,
    pub default_font_size: f32,
    pub default_font_path: Option<String>,
    pub debug_overlays: bool,
    pub hot_reload_shaders: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            screen_width: 1920,
            screen_height: 1080,
            dpi_scale: 1.0,
            default_font_size: 16.0,
            default_font_path: None,
            debug_overlays: false,
            hot_reload_shaders: true,
        }
    }
}

/// Color utilities
pub mod color {
    pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    pub const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    pub const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    pub const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
    pub const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    pub const YELLOW: [f32; 4] = [1.0, 1.0, 0.0, 1.0];
    pub const CYAN: [f32; 4] = [0.0, 1.0, 1.0, 1.0];
    pub const MAGENTA: [f32; 4] = [1.0, 0.0, 1.0, 1.0];
    pub const TRANSPARENT: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
    pub const GRAY: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
    pub const LIGHT_GRAY: [f32; 4] = [0.8, 0.8, 0.8, 1.0];
    pub const DARK_GRAY: [f32; 4] = [0.2, 0.2, 0.2, 1.0];
    pub const ORANGE: [f32; 4] = [1.0, 0.5, 0.0, 1.0];
    pub const PURPLE: [f32; 4] = [0.5, 0.0, 0.5, 1.0];
    pub const BROWN: [f32; 4] = [0.6, 0.4, 0.2, 1.0];
    pub const GOLD: [f32; 4] = [1.0, 0.84, 0.0, 1.0];
    pub const SILVER: [f32; 4] = [0.75, 0.75, 0.75, 1.0];
}

/// Alignment options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UiAlignment {
    #[default] TopLeft, TopCenter, TopRight,
    CenterLeft, Center, CenterRight,
    BottomLeft, BottomCenter, BottomRight,
}

impl UiAlignment {
    pub fn to_anchor(&self) -> (f32, f32) {
        match self {
            UiAlignment::TopLeft => (0.0, 0.0), UiAlignment::TopCenter => (0.5, 0.0), UiAlignment::TopRight => (1.0, 0.0),
            UiAlignment::CenterLeft => (0.0, 0.5), UiAlignment::Center => (0.5, 0.5), UiAlignment::CenterRight => (1.0, 0.5),
            UiAlignment::BottomLeft => (0.0, 1.0), UiAlignment::BottomCenter => (0.5, 1.0), UiAlignment::BottomRight => (1.0, 1.0),
        }
    }
}

/// UI error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum UiError {
    #[error("UI renderer not initialized")] RendererNotInitialized,
    #[error("Shader compilation failed: {0}")] ShaderCompilationFailed(String),
    #[error("Texture not found: {0}")] TextureNotFound(u64),
    #[error("Font not found: {0}")] FontNotFound(String),
    #[error("UI context already in frame")] ContextAlreadyInFrame,
    #[error("UI context not in frame")] ContextNotInFrame,
    #[error("Invalid UI command: {0}")] InvalidCommand(String),
    #[error("Backend error: {0}")] BackendError(String),
    #[error("Allocation failed")] AllocationFailed,
    #[error("Invalid dimensions")] InvalidDimensions,
}

pub type UiResult<T> = std::result::Result<T, UiError>;
