//! UI Context
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The UI context manages the state of UI rendering, including:
//! - Command buffer for immediate mode
//! - Element hierarchy for retained mode
//! - State stack for transforms and styles
//! - Input handling

use super::{UiCommand, UiConfig, UiError, UiResult};
use std::collections::HashMap;

/// UI State for retained mode elements
#[derive(Debug, Clone)]
pub struct UiState {
    /// Current frame number
    pub frame_number: u64,
    /// Time since start in seconds
    pub time: f64,
    /// Delta time since last frame in seconds
    pub delta_time: f32,
    /// Mouse position in screen coordinates
    pub mouse_position: (f32, f32),
    /// Mouse position in normalized coordinates (0-1)
    pub mouse_normalized: (f32, f32),
    /// Mouse buttons state
    pub mouse_buttons: [bool; 3],
    /// Keyboard modifier states
    pub modifiers: UiModifiers,
    /// Current hover element
    pub hover_element: Option<u64>,
    /// Current active element
    pub active_element: Option<u64>,
    /// Current focus element
    pub focus_element: Option<u64>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            frame_number: 0,
            time: 0.0,
            delta_time: 0.0,
            mouse_position: (0.0, 0.0),
            mouse_normalized: (0.0, 0.0),
            mouse_buttons: [false; 3],
            modifiers: UiModifiers::default(),
            hover_element: None,
            active_element: None,
            focus_element: None,
        }
    }
}

/// Keyboard modifier states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UiModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

/// UI Context for building UI
pub struct UiContext {
    /// Configuration
    config: UiConfig,
    /// Current state
    state: UiState,
    /// Command buffer for current frame
    commands: Vec<UiCommand>,
    /// Command stack for groups
    command_stack: Vec<Vec<UiCommand>>,
    /// Cursor position stack
    cursor_stack: Vec<(f32, f32)>,
    /// Current cursor position
    cursor: (f32, f32),
    /// Transform stack
    transform_stack: Vec<[f32; 6]>,
    /// Current transform
    transform: [f32; 6],
    /// Scissor stack
    scissor_stack: Vec<(f32, f32, f32, f32)>,
    /// Current scissor
    scissor: Option<(f32, f32, f32, f32)>,
    /// Whether we're in a frame
    in_frame: bool,
    /// Retained mode elements
    elements: HashMap<u64, super::elements::UiElement>,
    /// Next element ID
    next_element_id: u64,
    /// Clipboard content
    clipboard: Option<String>,
}

impl UiContext {
    /// Create a new UI context with default configuration
    pub fn new() -> Self {
        Self::with_config(UiConfig::default())
    }
    
    /// Create a new UI context with custom configuration
    pub fn with_config(config: UiConfig) -> Self {
        Self {
            config,
            state: UiState::default(),
            commands: Vec::new(),
            command_stack: Vec::new(),
            cursor_stack: Vec::new(),
            cursor: (0.0, 0.0),
            transform_stack: Vec::new(),
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], // Identity matrix
            scissor_stack: Vec::new(),
            scissor: None,
            in_frame: false,
            elements: HashMap::new(),
            next_element_id: 1,
            clipboard: None,
        }
    }
    
    /// Get the current configuration
    pub fn config(&self) -> &UiConfig {
        &self.config
    }
    
    /// Get mutable configuration
    pub fn config_mut(&mut self) -> &mut UiConfig {
        &mut self.config
    }
    
    /// Get the current state
    pub fn state(&self) -> &UiState {
        &self.state
    }
    
    /// Get mutable state
    pub fn state_mut(&mut self) -> &mut UiState {
        &mut self.state
    }
    
    /// Begin a new frame
    pub fn begin_frame(&mut self) -> UiResult<()> {
        if self.in_frame {
            return Err(UiError::ContextAlreadyInFrame);
        }
        
        self.commands.clear();
        self.command_stack.clear();
        self.cursor_stack.clear();
        self.transform_stack.clear();
        self.scissor_stack.clear();
        
        self.cursor = (0.0, 0.0);
        self.transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        self.scissor = None;
        self.in_frame = true;
        
        self.state.frame_number += 1;
        
        Ok(())
    }
    
    /// End the current frame and return commands
    pub fn end_frame(&mut self) -> UiResult<Vec<UiCommand>> {
        if !self.in_frame {
            return Err(UiError::ContextNotInFrame);
        }
        
        self.in_frame = false;
        
        // Flatten command stack
        let mut all_commands = std::mem::take(&mut self.commands);
        for mut group_commands in self.command_stack.drain(..) {
            all_commands.append(&mut group_commands);
        }
        
        Ok(all_commands)
    }
    
    /// Update state with new frame information
    pub fn update_state(&mut self, delta_time: f32) {
        self.state.delta_time = delta_time;
        self.state.time += delta_time as f64;
    }
    
    /// Update mouse position
    pub fn update_mouse(&mut self, x: f32, y: f32) {
        self.state.mouse_position = (x, y);
        self.state.mouse_normalized = (
            x / self.config.screen_width as f32,
            y / self.config.screen_height as f32,
        );
    }
    
    /// Update mouse button state
    pub fn update_mouse_button(&mut self, button: usize, pressed: bool) {
        if button < 3 {
            self.state.mouse_buttons[button] = pressed;
        }
    }
    
    /// Update keyboard modifiers
    pub fn update_modifiers(&mut self, modifiers: UiModifiers) {
        self.state.modifiers = modifiers;
    }
    
    /// Set clipboard content
    pub fn set_clipboard(&mut self, text: String) {
        self.clipboard = Some(text);
    }
    
    /// Get clipboard content
    pub fn get_clipboard(&self) -> Option<&str> {
        self.clipboard.as_deref()
    }
    
    /// Generate a new element ID
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_element_id;
        self.next_element_id += 1;
        id
    }
    
    /// Add a retained mode element
    pub fn add_element(&mut self, element: super::elements::UiElement) -> u64 {
        let id = self.next_id();
        self.elements.insert(id, element);
        id
    }
    
    /// Get a retained mode element by ID
    pub fn get_element(&self, id: u64) -> Option<&super::elements::UiElement> {
        self.elements.get(&id)
    }
    
    /// Get mutable retained mode element by ID
    pub fn get_element_mut(&mut self, id: u64) -> Option<&mut super::elements::UiElement> {
        self.elements.get_mut(&id)
    }
    
    /// Remove a retained mode element
    pub fn remove_element(&mut self, id: u64) -> Option<super::elements::UiElement> {
        self.elements.remove(&id)
    }
    
    /// Clear all retained mode elements
    pub fn clear_elements(&mut self) {
        self.elements.clear();
        self.next_element_id = 1;
    }
}

impl Default for UiContext {
    fn default() -> Self {
        Self::new()
    }
}
