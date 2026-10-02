//! UI Renderer
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The UI renderer handles the actual rendering of UI commands and elements.

use std::collections::HashMap;
use std::sync::Arc;
use crate::{Device, Extent3D, Queue, Texture};
use super::{UiCommand, UiConfig, UiError, UiResult, UiElement, UiElementId};
use super::pass::UiRenderPass;
use super::shaders::UiShaderManager;

/// UI Renderer Configuration
#[derive(Debug, Clone)]
pub struct UiRendererConfig {
    /// Maximum number of vertices per batch
    pub max_vertices_per_batch: usize,
    /// Maximum number of indices per batch
    pub max_indices_per_batch: usize,
    /// Maximum number of texture units
    pub max_texture_units: usize,
    /// Enable instanced rendering
    pub instanced_rendering: bool,
    /// Enable batching
    pub batching_enabled: bool,
    /// Enable culling
    pub culling_enabled: bool,
}

impl Default for UiRendererConfig {
    fn default() -> Self {
        Self {
            max_vertices_per_batch: 65535,
            max_indices_per_batch: 65535,
            max_texture_units: 16,
            instanced_rendering: true,
            batching_enabled: true,
            culling_enabled: false,
        }
    }
}

/// UI Render Statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct UiRenderStats {
    /// Number of draw calls
    pub draw_calls: u32,
    /// Number of triangles rendered
    pub triangles: u32,
    /// Number of vertices processed
    pub vertices: u32,
    /// Number of batches
    pub batches: u32,
    /// Number of textures bound
    pub textures_bound: u32,
    /// Number of commands processed
    pub commands_processed: u32,
    /// Number of elements rendered
    pub elements_rendered: u32,
    /// Render time in milliseconds
    pub render_time_ms: f32,
}

/// Vertex format for UI rendering
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct UiVertex {
    /// Position (x, y)
    pub position: [f32; 2],
    /// Texture coordinates (u, v)
    pub tex_coords: [f32; 2],
    /// Color (r, g, b, a)
    pub color: [f32; 4],
}

impl Default for UiVertex {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0],
            tex_coords: [0.0, 0.0],
            color: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// Geometry batch for UI rendering
#[derive(Debug, Clone, Default)]
pub struct UiGeometryBatch {
    /// Vertex data
    pub vertices: Vec<UiVertex>,
    /// Index data
    pub indices: Vec<u32>,
    /// Texture ID (0 = no texture)
    pub texture_id: u64,
}

/// Font for text rendering
#[derive(Debug, Clone)]
pub struct UiFont {
    /// Font name
    pub name: String,
    /// Font path
    pub path: String,
    /// Font size
    pub size: f32,
    /// Font texture atlas
    pub atlas: Option<Arc<Texture>>,
    /// Character information
    pub characters: HashMap<char, UiFontCharacter>,
}

/// Font character information
#[derive(Debug, Clone)]
pub struct UiFontCharacter {
    /// UV coordinates in atlas
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// Size of character
    pub size: [f32; 2],
    /// Offset from baseline
    pub offset: [f32; 2],
    /// Advance to next character
    pub advance: f32,
}

/// UI Renderer
/// 
/// This is a simplified UI renderer that processes commands but doesn't actually
/// render them. In a full implementation, this would handle the actual rendering.
pub struct UiRenderer {
    /// Device reference
    _device: Arc<Device>,
    /// Graphics queue
    _queue: Arc<Queue>,
    /// UI render pass
    render_pass: UiRenderPass,
    /// Shader manager
    _shader_manager: UiShaderManager,
    /// Configuration
    _config: UiRendererConfig,
    /// UI configuration
    ui_config: UiConfig,
    /// Command queue
    command_queue: Vec<UiCommand>,
    /// Texture cache
    _texture_cache: HashMap<u64, Arc<Texture>>,
    /// Font cache
    _font_cache: HashMap<String, UiFont>,
    /// Retained mode elements
    _elements: HashMap<UiElementId, UiElement>,
    /// Render statistics
    stats: UiRenderStats,
    /// Whether the renderer is initialized
    initialized: bool,
    /// Current frame number
    frame_number: u64,
}

impl UiRenderer {
    /// Create a new UI renderer
    pub fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        _config: UiRendererConfig,
    ) -> Self {
        Self {
            _device: device.clone(),
            _queue: queue,
            render_pass: UiRenderPass::new(),
            _shader_manager: UiShaderManager::new(device),
            _config: UiRendererConfig::default(),
            ui_config: UiConfig::default(),
            command_queue: Vec::new(),
            _texture_cache: HashMap::new(),
            _font_cache: HashMap::new(),
            _elements: HashMap::new(),
            stats: UiRenderStats::default(),
            initialized: false,
            frame_number: 0,
        }
    }
    
    /// Initialize the renderer
    pub fn initialize(
        &mut self,
        swapchain_extent: Extent3D,
        ui_config: UiConfig,
    ) -> UiResult<()> {
        self.render_pass.initialize(&self._device, swapchain_extent, ui_config.clone())?;
        self.ui_config = ui_config;
        self.initialized = true;
        Ok(())
    }
    
    /// Submit UI commands for rendering
    pub fn submit_commands(&mut self, commands: Vec<UiCommand>) -> UiResult<()> {
        if !self.initialized {
            return Err(UiError::RendererNotInitialized);
        }
        self.command_queue.extend(commands);
        Ok(())
    }
    
    /// Submit a retained mode element for rendering
    pub fn submit_element(&mut self, element: UiElement) -> UiResult<()> {
        if !self.initialized {
            return Err(UiError::RendererNotInitialized);
        }
        self._elements.insert(element.id, element);
        Ok(())
    }
    
    /// Remove a retained mode element
    pub fn remove_element(&mut self, id: UiElementId) -> UiResult<()> {
        self._elements.remove(&id);
        Ok(())
    }
    
    /// Clear all retained mode elements
    pub fn clear_elements(&mut self) {
        self._elements.clear();
    }
    
    /// Get render statistics
    pub fn stats(&self) -> UiRenderStats {
        self.stats
    }
    
    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = UiRenderStats::default();
    }
    
    /// Get current frame number
    pub fn frame_number(&self) -> u64 {
        self.frame_number
    }
    
    /// Check if the renderer is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
    
    /// Get the UI configuration
    pub fn ui_config(&self) -> &UiConfig {
        &self.ui_config
    }
    
    /// Update UI configuration
    pub fn update_ui_config(&mut self, config: UiConfig) -> UiResult<()> {
        self.ui_config = config.clone();
        if self.initialized {
            self.render_pass.update_config(config)?;
        }
        Ok(())
    }
    
    /// Resize the renderer
    pub fn resize(&mut self, width: u32, height: u32) -> UiResult<()> {
        self.render_pass.resize(&self._device, width, height)?;
        self.ui_config.screen_width = width;
        self.ui_config.screen_height = height;
        Ok(())
    }
}
