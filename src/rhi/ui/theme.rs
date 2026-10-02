//! UI Theme System - Nvidia GeForce Experience Style
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This module provides a theme system styled after Nvidia GeForce Experience.
//! Features:
//! - Dark theme with green accents
//! - Modern flat design
//! - Semi-transparent panels
//! - Smooth animations
//! - Consistent styling across all UI elements

use super::UiAlignment;

/// Nvidia GeForce Experience color palette
pub mod nvidia_colors {
    /// Primary background - Deep dark gray (almost black)
    pub const BACKGROUND_PRIMARY: [f32; 4] = [0.08, 0.08, 0.08, 1.0];
    
    /// Secondary background - Slightly lighter dark gray
    pub const BACKGROUND_SECONDARY: [f32; 4] = [0.12, 0.12, 0.12, 1.0];
    
    /// Tertiary background - For elevated surfaces
    pub const BACKGROUND_TERTIARY: [f32; 4] = [0.18, 0.18, 0.18, 1.0];
    
    /// Panel background - Semi-transparent dark
    pub const PANEL_BACKGROUND: [f32; 4] = [0.15, 0.15, 0.15, 0.95];
    
    /// Nvidia Green - Primary accent color
    pub const NVIDIA_GREEN: [f32; 4] = [0.0, 0.85, 0.35, 1.0];
    
    /// Nvidia Green Light - Lighter accent for hover states
    pub const NVIDIA_GREEN_LIGHT: [f32; 4] = [0.2, 1.0, 0.4, 1.0];
    
    /// Nvidia Green Dark - Darker accent for pressed states
    pub const NVIDIA_GREEN_DARK: [f32; 4] = [0.0, 0.65, 0.25, 1.0];
    
    /// Text primary - Light gray for readability on dark backgrounds
    pub const TEXT_PRIMARY: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
    
    /// Text secondary - Medium gray for secondary information
    pub const TEXT_SECONDARY: [f32; 4] = [0.65, 0.65, 0.65, 1.0];
    
    /// Text disabled - Dark gray for disabled elements
    pub const TEXT_DISABLED: [f32; 4] = [0.35, 0.35, 0.35, 1.0];
    
    /// Border color - Subtle gray for borders
    pub const BORDER: [f32; 4] = [0.25, 0.25, 0.25, 1.0];
    
    /// Border highlight - Nvidia green for active borders
    pub const BORDER_HIGHLIGHT: [f32; 4] = [0.0, 0.85, 0.35, 1.0];
    
    /// Success color - Green for positive actions
    pub const SUCCESS: [f32; 4] = [0.0, 0.85, 0.35, 1.0];
    
    /// Warning color - Orange for alerts
    pub const WARNING: [f32; 4] = [1.0, 0.65, 0.0, 1.0];
    
    /// Error color - Red for errors
    pub const ERROR: [f32; 4] = [0.95, 0.2, 0.2, 1.0];
    
    /// Info color - Blue for information
    pub const INFO: [f32; 4] = [0.2, 0.6, 1.0, 1.0];
    
    /// Overlay background - Semi-transparent black for overlays
    pub const OVERLAY: [f32; 4] = [0.0, 0.0, 0.0, 0.7];
    
    /// Tooltip background - Dark with high opacity
    pub const TOOLTIP_BACKGROUND: [f32; 4] = [0.2, 0.2, 0.2, 0.98];
    
    /// Progress bar fill - Nvidia green
    pub const PROGRESS_FILL: [f32; 4] = [0.0, 0.85, 0.35, 1.0];
    
    /// Progress bar background - Dark gray
    pub const PROGRESS_BACKGROUND: [f32; 4] = [0.15, 0.15, 0.15, 1.0];
}

/// UI Theme
#[derive(Debug, Clone)]
pub struct UiTheme {
    /// Theme name
    pub name: String,
    
    /// Background colors
    pub background_primary: [f32; 4],
    pub background_secondary: [f32; 4],
    pub background_tertiary: [f32; 4],
    pub panel_background: [f32; 4],
    
    /// Text colors
    pub text_primary: [f32; 4],
    pub text_secondary: [f32; 4],
    pub text_disabled: [f32; 4],
    
    /// Accent colors
    pub accent_primary: [f32; 4],
    pub accent_secondary: [f32; 4],
    pub accent_tertiary: [f32; 4],
    
    /// Border colors
    pub border: [f32; 4],
    pub border_highlight: [f32; 4],
    
    /// Status colors
    pub success: [f32; 4],
    pub warning: [f32; 4],
    pub error: [f32; 4],
    pub info: [f32; 4],
    
    /// Button styles
    pub button_normal: [f32; 4],
    pub button_hover: [f32; 4],
    pub button_pressed: [f32; 4],
    pub button_disabled: [f32; 4],
    
    /// Input field styles
    pub input_background: [f32; 4],
    pub input_border: [f32; 4],
    pub input_text: [f32; 4],
    pub input_placeholder: [f32; 4],
    
    /// Scrollbar styles
    pub scrollbar_background: [f32; 4],
    pub scrollbar_thumb: [f32; 4],
    pub scrollbar_thumb_hover: [f32; 4],
    
    /// Progress bar styles
    pub progress_background: [f32; 4],
    pub progress_fill: [f32; 4],
    
    /// Checkbox and radio button styles
    pub checkbox_unchecked: [f32; 4],
    pub checkbox_checked: [f32; 4],
    pub checkbox_disabled: [f32; 4],
    
    /// Tooltip styles
    pub tooltip_background: [f32; 4],
    pub tooltip_text: [f32; 4],
    pub tooltip_border: [f32; 4],
    
    /// Panel styles
    pub panel_border: [f32; 4],
    pub panel_title: [f32; 4],
    
    /// Corner radius for various elements
    pub corner_radius_small: f32,
    pub corner_radius_medium: f32,
    pub corner_radius_large: f32,
    
    /// Border widths
    pub border_width_thin: f32,
    pub border_width_normal: f32,
    pub border_width_thick: f32,
    
    /// Shadows
    pub shadow_color: [f32; 4],
    pub shadow_blur: f32,
    pub shadow_offset: (f32, f32),
    
    /// Padding and spacing
    pub padding_small: f32,
    pub padding_medium: f32,
    pub padding_large: f32,
    pub spacing_small: f32,
    pub spacing_medium: f32,
    pub spacing_large: f32,
    
    /// Font sizes
    pub font_size_small: f32,
    pub font_size_medium: f32,
    pub font_size_large: f32,
    pub font_size_title: f32,
    
    /// Default font path
    pub default_font: Option<String>,
    
    /// Default text alignment
    pub default_alignment: UiAlignment,
    
    /// Animation settings
    pub animation_enabled: bool,
    pub animation_duration: f32,
    pub animation_easing: AnimationEasing,
}

/// Animation easing functions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationEasing {
    Linear,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,
}

impl AnimationEasing {
    /// Apply easing function to a value (0.0 to 1.0)
    pub fn apply(&self, t: f32) -> f32 {
        match self {
            AnimationEasing::Linear => t,
            AnimationEasing::EaseInQuad => t * t,
            AnimationEasing::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            AnimationEasing::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            AnimationEasing::EaseInCubic => t * t * t,
            AnimationEasing::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            AnimationEasing::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    (2.0 * t - 2.0).powi(3) + 1.0
                }
            }
            AnimationEasing::EaseInSine => 1.0 - (std::f32::consts::PI / 2.0 - t * std::f32::consts::PI / 2.0).cos(),
            AnimationEasing::EaseOutSine => (t * std::f32::consts::PI / 2.0).sin(),
            AnimationEasing::EaseInOutSine => (std::f32::consts::PI * t / 2.0).sin(),
        }
    }
}

impl Default for AnimationEasing {
    fn default() -> Self {
        Self::EaseOutQuad
    }
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            name: "Nvidia GeForce Experience".to_string(),
            
            // Background colors
            background_primary: nvidia_colors::BACKGROUND_PRIMARY,
            background_secondary: nvidia_colors::BACKGROUND_SECONDARY,
            background_tertiary: nvidia_colors::BACKGROUND_TERTIARY,
            panel_background: nvidia_colors::PANEL_BACKGROUND,
            
            // Text colors
            text_primary: nvidia_colors::TEXT_PRIMARY,
            text_secondary: nvidia_colors::TEXT_SECONDARY,
            text_disabled: nvidia_colors::TEXT_DISABLED,
            
            // Accent colors (Nvidia Green)
            accent_primary: nvidia_colors::NVIDIA_GREEN,
            accent_secondary: nvidia_colors::NVIDIA_GREEN_LIGHT,
            accent_tertiary: nvidia_colors::NVIDIA_GREEN_DARK,
            
            // Border colors
            border: nvidia_colors::BORDER,
            border_highlight: nvidia_colors::BORDER_HIGHLIGHT,
            
            // Status colors
            success: nvidia_colors::SUCCESS,
            warning: nvidia_colors::WARNING,
            error: nvidia_colors::ERROR,
            info: nvidia_colors::INFO,
            
            // Button styles
            button_normal: [0.15, 0.15, 0.15, 1.0],
            button_hover: [0.2, 0.2, 0.2, 1.0],
            button_pressed: [0.1, 0.1, 0.1, 1.0],
            button_disabled: [0.08, 0.08, 0.08, 1.0],
            
            // Input field styles
            input_background: [0.12, 0.12, 0.12, 1.0],
            input_border: [0.25, 0.25, 0.25, 1.0],
            input_text: [0.95, 0.95, 0.95, 1.0],
            input_placeholder: [0.5, 0.5, 0.5, 1.0],
            
            // Scrollbar styles
            scrollbar_background: [0.1, 0.1, 0.1, 1.0],
            scrollbar_thumb: [0.25, 0.25, 0.25, 1.0],
            scrollbar_thumb_hover: [0.35, 0.35, 0.35, 1.0],
            
            // Progress bar styles
            progress_background: nvidia_colors::PROGRESS_BACKGROUND,
            progress_fill: nvidia_colors::PROGRESS_FILL,
            
            // Checkbox styles
            checkbox_unchecked: [0.25, 0.25, 0.25, 1.0],
            checkbox_checked: nvidia_colors::NVIDIA_GREEN,
            checkbox_disabled: [0.15, 0.15, 0.15, 1.0],
            
            // Tooltip styles
            tooltip_background: nvidia_colors::TOOLTIP_BACKGROUND,
            tooltip_text: [0.95, 0.95, 0.95, 1.0],
            tooltip_border: [0.4, 0.4, 0.4, 1.0],
            
            // Panel styles
            panel_border: [0.2, 0.2, 0.2, 1.0],
            panel_title: [0.95, 0.95, 0.95, 1.0],
            
            // Corner radii
            corner_radius_small: 2.0,
            corner_radius_medium: 4.0,
            corner_radius_large: 8.0,
            
            // Border widths
            border_width_thin: 1.0,
            border_width_normal: 2.0,
            border_width_thick: 3.0,
            
            // Shadows
            shadow_color: [0.0, 0.0, 0.0, 0.3],
            shadow_blur: 8.0,
            shadow_offset: (0.0, 2.0),
            
            // Padding and spacing
            padding_small: 4.0,
            padding_medium: 8.0,
            padding_large: 16.0,
            spacing_small: 4.0,
            spacing_medium: 8.0,
            spacing_large: 16.0,
            
            // Font sizes
            font_size_small: 12.0,
            font_size_medium: 14.0,
            font_size_large: 16.0,
            font_size_title: 20.0,
            
            // Default font
            default_font: None,
            
            // Default alignment
            default_alignment: UiAlignment::TopLeft,
            
            // Animation settings
            animation_enabled: true,
            animation_duration: 0.2,
            animation_easing: AnimationEasing::EaseOutQuad,
        }
    }
}

impl UiTheme {
    /// Create a new Nvidia GeForce Experience theme
    pub fn nvidia_geforce() -> Self {
        Self::default()
    }
    
    /// Create a dark theme
    pub fn dark() -> Self {
        Self {
            name: "Dark".to_string(),
            ..Self::default()
        }
    }
    
    /// Create a light theme (for contrast)
    pub fn light() -> Self {
        Self {
            name: "Light".to_string(),
            background_primary: [0.98, 0.98, 0.98, 1.0],
            background_secondary: [0.95, 0.95, 0.95, 1.0],
            background_tertiary: [0.90, 0.90, 0.90, 1.0],
            panel_background: [1.0, 1.0, 1.0, 0.95],
            text_primary: [0.1, 0.1, 0.1, 1.0],
            text_secondary: [0.4, 0.4, 0.4, 1.0],
            text_disabled: [0.65, 0.65, 0.65, 1.0],
            border: [0.8, 0.8, 0.8, 1.0],
            button_normal: [0.9, 0.9, 0.9, 1.0],
            button_hover: [0.85, 0.85, 0.85, 1.0],
            button_pressed: [0.8, 0.8, 0.8, 1.0],
            input_background: [1.0, 1.0, 1.0, 1.0],
            input_border: [0.7, 0.7, 0.7, 1.0],
            input_text: [0.1, 0.1, 0.1, 1.0],
            input_placeholder: [0.5, 0.5, 0.5, 1.0],
            scrollbar_background: [0.9, 0.9, 0.9, 1.0],
            scrollbar_thumb: [0.75, 0.75, 0.75, 1.0],
            tooltip_background: [1.0, 1.0, 1.0, 0.98],
            tooltip_text: [0.1, 0.1, 0.1, 1.0],
            panel_border: [0.7, 0.7, 0.7, 1.0],
            panel_title: [0.1, 0.1, 0.1, 1.0],
            shadow_color: [0.0, 0.0, 0.0, 0.1],
            ..Self::default()
        }
    }
    
    /// Get button style for a given state
    pub fn get_button_color(&self, state: ButtonState) -> [f32; 4] {
        match state {
            ButtonState::Normal => self.button_normal,
            ButtonState::Hover => self.button_hover,
            ButtonState::Pressed => self.button_pressed,
            ButtonState::Disabled => self.button_disabled,
            ButtonState::Focused => self.accent_primary,
        }
    }
    
    /// Get input field style for a given state
    pub fn get_input_style(&self, state: InputState) -> InputStyle {
        match state {
            InputState::Normal => InputStyle {
                background: self.input_background,
                border: self.input_border,
                text: self.input_text,
            },
            InputState::Focused => InputStyle {
                background: self.input_background,
                border: self.accent_primary,
                text: self.input_text,
            },
            InputState::Disabled => InputStyle {
                background: [0.1, 0.1, 0.1, 1.0],
                border: [0.2, 0.2, 0.2, 1.0],
                text: self.text_disabled,
            },
            InputState::Error => InputStyle {
                background: self.input_background,
                border: self.error,
                text: self.input_text,
            },
        }
    }
    
    /// Get progress bar color based on value
    pub fn get_progress_color(&self, progress: f32) -> [f32; 4] {
        if progress >= 0.75 {
            self.success
        } else if progress >= 0.5 {
            self.info
        } else if progress >= 0.25 {
            self.warning
        } else {
            self.error
        }
    }
}

/// Button state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    Normal,
    Hover,
    Pressed,
    Disabled,
    Focused,
}

/// Input field state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputState {
    Normal,
    Focused,
    Disabled,
    Error,
}

/// Input field style
#[derive(Debug, Clone)]
pub struct InputStyle {
    pub background: [f32; 4],
    pub border: [f32; 4],
    pub text: [f32; 4],
}

/// Theme manager for handling multiple themes
pub struct ThemeManager {
    /// Current theme
    current_theme: UiTheme,
    /// Available themes
    themes: std::collections::HashMap<String, UiTheme>,
}

impl ThemeManager {
    /// Create a new theme manager with default themes
    pub fn new() -> Self {
        let mut themes = std::collections::HashMap::new();
        
        // Add default themes
        themes.insert("nvidia".to_string(), UiTheme::nvidia_geforce());
        themes.insert("dark".to_string(), UiTheme::dark());
        themes.insert("light".to_string(), UiTheme::light());
        
        Self {
            current_theme: UiTheme::nvidia_geforce(),
            themes,
        }
    }
    
    /// Get the current theme
    pub fn current(&self) -> &UiTheme {
        &self.current_theme
    }
    
    /// Get mutable current theme
    pub fn current_mut(&mut self) -> &mut UiTheme {
        &mut self.current_theme
    }
    
    /// Set the current theme by name
    pub fn set_theme(&mut self, name: &str) -> Option<()> {
        if let Some(theme) = self.themes.get(name) {
            self.current_theme = theme.clone();
            Some(())
        } else {
            None
        }
    }
    
    /// Add a new theme
    pub fn add_theme(&mut self, name: String, theme: UiTheme) {
        self.themes.insert(name, theme);
    }
    
    /// Remove a theme
    pub fn remove_theme(&mut self, name: &str) -> Option<UiTheme> {
        self.themes.remove(name)
    }
    
    /// Get a theme by name
    pub fn get_theme(&self, name: &str) -> Option<&UiTheme> {
        self.themes.get(name)
    }
    
    /// List all available theme names
    pub fn theme_names(&self) -> Vec<&String> {
        self.themes.keys().collect()
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply Nvidia GeForce Experience style to a UI element
pub trait NvidiaStyle {
    /// Apply Nvidia button style
    fn apply_nvidia_button_style(&mut self);
    
    /// Apply Nvidia panel style
    fn apply_nvidia_panel_style(&mut self);
    
    /// Apply Nvidia input style
    fn apply_nvidia_input_style(&mut self);
    
    /// Apply Nvidia progress bar style
    fn apply_nvidia_progress_style(&mut self);
}
