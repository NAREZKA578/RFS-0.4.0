//! UI Shader Manager
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Manages loading, compilation, and hot-reloading of UI shaders.

use std::path::PathBuf;
use std::sync::Arc;
use crate::Device;
use super::UiResult;

/// Shader type for UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiShaderType {
    /// UI vertex shader
    UiVertex,
    /// UI fragment shader
    UiFragment,
    /// Text vertex shader
    TextVertex,
    /// Text fragment shader
    TextFragment,
}

impl UiShaderType {
    /// Get the default shader file name for this type
    pub fn default_filename(&self) -> &'static str {
        match self {
            UiShaderType::UiVertex => "ui.vert",
            UiShaderType::UiFragment => "ui.frag",
            UiShaderType::TextVertex => "text.vert",
            UiShaderType::TextFragment => "text.frag",
        }
    }
    
    /// Get the shader stage name
    pub fn stage_name(&self) -> &'static str {
        match self {
            UiShaderType::UiVertex | UiShaderType::TextVertex => "vertex",
            UiShaderType::UiFragment | UiShaderType::TextFragment => "fragment",
        }
    }
}

/// Shader source code
#[derive(Debug, Clone)]
pub struct UiShaderSource {
    /// Source code
    pub code: String,
    /// File path (if loaded from file)
    pub path: Option<PathBuf>,
    /// Shader type
    pub shader_type: UiShaderType,
}

/// UI Shader Manager
/// 
/// This is a simplified shader manager that doesn't actually compile shaders.
/// In a full implementation, this would manage loading, compilation, and hot-reloading.
pub struct UiShaderManager {
    /// Device reference
    _device: Arc<Device>,
    /// Whether the manager is initialized
    initialized: bool,
}

impl UiShaderManager {
    /// Create a new shader manager
    pub fn new(device: Arc<Device>) -> Self {
        Self {
            _device: device,
            initialized: false,
        }
    }
    
    /// Initialize the shader manager
    pub fn initialize(&mut self, _device: &Arc<Device>) -> UiResult<()> {
        self.initialized = true;
        Ok(())
    }
    
    /// Check if the manager is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}


