//! UI Layout System
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Layout management for UI elements with constraints and anchoring.

use super::UiAlignment;

/// Layout result for an element
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiLayoutResult {
    /// Final x position
    pub x: f32,
    /// Final y position
    pub y: f32,
    /// Final width
    pub width: f32,
    /// Final height
    pub height: f32,
}

/// Layout constraints for UI elements
#[derive(Debug, Clone, Copy, PartialEq)]
#[derive(Default)]
pub struct UiConstraints {
    /// Minimum width
    pub min_width: Option<f32>,
    /// Maximum width
    pub max_width: Option<f32>,
    /// Preferred width
    pub preferred_width: Option<f32>,
    /// Minimum height
    pub min_height: Option<f32>,
    /// Maximum height
    pub max_height: Option<f32>,
    /// Preferred height
    pub preferred_height: Option<f32>,
    /// Width fill (1.0 = fill parent)
    pub width_fill: Option<f32>,
    /// Height fill (1.0 = fill parent)
    pub height_fill: Option<f32>,
    /// Aspect ratio (width / height)
    pub aspect_ratio: Option<f32>,
}


impl UiConstraints {
    /// Create constraints with minimum size
    pub fn min_size(width: f32, height: f32) -> Self {
        Self {
            min_width: Some(width),
            min_height: Some(height),
            ..Default::default()
        }
    }
    
    /// Create constraints with exact size
    pub fn exact_size(width: f32, height: f32) -> Self {
        Self {
            min_width: Some(width),
            max_width: Some(width),
            min_height: Some(height),
            max_height: Some(height),
            ..Default::default()
        }
    }
    
    /// Create constraints with maximum size
    pub fn max_size(width: f32, height: f32) -> Self {
        Self {
            max_width: Some(width),
            max_height: Some(height),
            ..Default::default()
        }
    }
    
    /// Create fill constraints
    pub fn fill() -> Self {
        Self {
            width_fill: Some(1.0),
            height_fill: Some(1.0),
            ..Default::default()
        }
    }
    
    /// Create horizontal fill constraints
    pub fn fill_horizontal() -> Self {
        Self {
            width_fill: Some(1.0),
            ..Default::default()
        }
    }
    
    /// Create vertical fill constraints
    pub fn fill_vertical() -> Self {
        Self {
            height_fill: Some(1.0),
            ..Default::default()
        }
    }
}

/// Anchor point for layout
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UiAnchor {
    /// Horizontal anchor (0.0 = left, 0.5 = center, 1.0 = right)
    pub x: f32,
    /// Vertical anchor (0.0 = top, 0.5 = center, 1.0 = bottom)
    pub y: f32,
}

impl UiAnchor {
    /// Create anchor from alignment
    pub fn from_alignment(alignment: UiAlignment) -> Self {
        let (x, y) = alignment.to_anchor();
        Self { x, y }
    }
    
    /// Top-left anchor
    pub fn top_left() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
    
    /// Top-center anchor
    pub fn top_center() -> Self {
        Self { x: 0.5, y: 0.0 }
    }
    
    /// Top-right anchor
    pub fn top_right() -> Self {
        Self { x: 1.0, y: 0.0 }
    }
    
    /// Center-left anchor
    pub fn center_left() -> Self {
        Self { x: 0.0, y: 0.5 }
    }
    
    /// Center anchor
    pub fn center() -> Self {
        Self { x: 0.5, y: 0.5 }
    }
    
    /// Center-right anchor
    pub fn center_right() -> Self {
        Self { x: 1.0, y: 0.5 }
    }
    
    /// Bottom-left anchor
    pub fn bottom_left() -> Self {
        Self { x: 0.0, y: 1.0 }
    }
    
    /// Bottom-center anchor
    pub fn bottom_center() -> Self {
        Self { x: 0.5, y: 1.0 }
    }
    
    /// Bottom-right anchor
    pub fn bottom_right() -> Self {
        Self { x: 1.0, y: 1.0 }
    }
}

/// Layout manager for UI
pub struct UiLayout {
    /// Parent dimensions
    parent_width: f32,
    parent_height: f32,
    /// Padding
    padding: [f32; 4],
    /// Margin
    margin: [f32; 4],
    /// Spacing between elements
    spacing: f32,
}

impl Default for UiLayout {
    fn default() -> Self {
        Self {
            parent_width: 0.0,
            parent_height: 0.0,
            padding: [0.0, 0.0, 0.0, 0.0],
            margin: [0.0, 0.0, 0.0, 0.0],
            spacing: 0.0,
        }
    }
}

impl UiLayout {
    /// Create a new layout with parent dimensions
    pub fn new(parent_width: f32, parent_height: f32) -> Self {
        Self {
            parent_width,
            parent_height,
            ..Default::default()
        }
    }
    
    /// Set padding
    pub fn with_padding(mut self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        self.padding = [left, top, right, bottom];
        self
    }
    
    /// Set uniform padding
    pub fn with_uniform_padding(mut self, padding: f32) -> Self {
        self.padding = [padding, padding, padding, padding];
        self
    }
    
    /// Set margin
    pub fn with_margin(mut self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        self.margin = [left, top, right, bottom];
        self
    }
    
    /// Set spacing
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }
}
