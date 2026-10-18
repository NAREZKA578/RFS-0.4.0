//! UI Elements
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Retained mode UI elements for complex user interfaces.

use std::collections::HashMap;
use super::UiAlignment;
use super::layout::{UiConstraints, UiLayoutMode};
use super::theme::NvidiaStyle;

/// Unique identifier for UI elements
pub type UiElementId = u64;

/// Type of UI element
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiElementType {
    /// Container for other elements
    Container,
    /// Text element
    Text,
    /// Image element
    Image,
    /// Button element
    Button,
    /// Input field element
    InputField,
    /// Checkbox element
    Checkbox,
    /// Radio button element
    RadioButton,
    /// Slider element
    Slider,
    /// Progress bar element
    ProgressBar,
    /// Panel/background element
    Panel,
    /// Scroll container
    ScrollContainer,
    /// Dropdown menu
    Dropdown,
    /// List view
    ListView,
    /// Grid view
    GridView,
    /// Custom element
    Custom,
}

/// Visual style for UI elements
#[derive(Debug, Clone, PartialEq)]
pub struct UiVisualStyle {
    /// Background color
    pub background_color: Option<[f32; 4]>,
    /// Foreground/text color
    pub foreground_color: Option<[f32; 4]>,
    /// Border color
    pub border_color: Option<[f32; 4]>,
    /// Border width
    pub border_width: f32,
    /// Corner radius
    pub corner_radius: f32,
    /// Padding (left, top, right, bottom)
    pub padding: [f32; 4],
    /// Margin (left, top, right, bottom)
    pub margin: [f32; 4],
    /// Font size
    pub font_size: f32,
    /// Font path
    pub font_path: Option<String>,
    /// Text alignment
    pub text_alignment: UiAlignment,
    /// Horizontal alignment
    pub horizontal_align: UiAlignment,
    /// Vertical alignment
    pub vertical_align: UiAlignment,
    /// Visibility
    pub visible: bool,
    /// Opacity (0.0 to 1.0)
    pub opacity: f32,
    /// Z-index (rendering order)
    pub z_index: i32,
}

impl Default for UiVisualStyle {
    fn default() -> Self {
        Self {
            background_color: None,
            foreground_color: None,
            border_color: None,
            border_width: 0.0,
            corner_radius: 0.0,
            padding: [0.0, 0.0, 0.0, 0.0],
            margin: [0.0, 0.0, 0.0, 0.0],
            font_size: 16.0,
            font_path: None,
            text_alignment: UiAlignment::TopLeft,
            horizontal_align: UiAlignment::TopLeft,
            vertical_align: UiAlignment::TopLeft,
            visible: true,
            opacity: 1.0,
            z_index: 0,
        }
    }
}

/// Slider orientation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SliderOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// Element-specific data
#[derive(Debug, Clone, Default)]
pub enum UiElementData {
    #[default]
    Empty,
    Text {
        content: String,
    },
    Image {
        texture_id: u64,
        uv_min: [f32; 2],
        uv_max: [f32; 2],
    },
    Button {
        text: String,
        normal_color: [f32; 4],
        hover_color: [f32; 4],
        pressed_color: [f32; 4],
        disabled_color: [f32; 4],
    },
    InputField {
        placeholder: String,
        value: String,
        max_length: Option<usize>,
        multiline: bool,
        password: bool,
    },
    Checkbox {
        checked: bool,
        label: String,
    },
    RadioButton {
        checked: bool,
        label: String,
        group: String,
    },
    Slider {
        min: f32,
        max: f32,
        value: f32,
        step: Option<f32>,
        orientation: SliderOrientation,
    },
    ProgressBar {
        progress: f32,
        show_text: bool,
        text_format: String,
    },
    ScrollContainer {
        scroll_x: f32,
        scroll_y: f32,
        scroll_width: f32,
        scroll_height: f32,
    },
    Dropdown {
        selected_index: Option<usize>,
        items: Vec<String>,
        is_open: bool,
    },
    ListView {
        items: Vec<String>,
        selected_indices: Vec<usize>,
    },
    GridView {
        columns: usize,
        items: Vec<String>,
        selected_indices: Vec<usize>,
    },
    Custom {
        render_function: Option<String>,
    },
}

/// UI Element base structure
#[derive(Debug, Clone)]
pub struct UiElement {
    /// Element ID
    pub id: UiElementId,
    /// Element type
    pub element_type: UiElementType,
    /// Parent element ID
    pub parent_id: Option<UiElementId>,
    /// Children element IDs
    pub children: Vec<UiElementId>,
    /// Element name (for debugging)
    pub name: Option<String>,
    /// Visual style
    pub style: UiVisualStyle,
    /// Position (x, y)
    pub position: (f32, f32),
    /// Size (width, height)
    pub size: (f32, f32),
    /// Minimum size
    pub min_size: Option<(f32, f32)>,
    /// Maximum size
    pub max_size: Option<(f32, f32)>,
    /// Size constraints: preferred size, fill fraction, aspect ratio.
    ///
    /// Kept apart from `min_size`/`max_size` because they answer different
    /// questions: those clamp what the element asked for, these describe what
    /// it wants in the first place.
    pub constraints: UiConstraints,
    /// Anchor point (0.0 to 1.0)
    pub anchor: (f32, f32),
    /// Pivot point (0.0 to 1.0)
    pub pivot: (f32, f32),
    /// How this container arranges its children.
    ///
    /// `Absolute` is the default; `Row`/`Column` stack the children with
    /// [`Self::layout_spacing`] between them, inside this element's padded box.
    pub layout: UiLayoutMode,
    /// Gap between children under `Row`/`Column` layout, in pixels.
    pub layout_spacing: f32,
    /// Rotation in radians
    pub rotation: f32,
    /// Scale (x, y)
    pub scale: (f32, f32),
    /// Whether element is interactive
    pub interactive: bool,
    /// Whether element is enabled
    pub enabled: bool,
    /// Whether element is focused
    pub focused: bool,
    /// Whether element is hovered
    pub hovered: bool,
    /// Whether element is pressed
    pub pressed: bool,
    /// Whether element is selected
    pub selected: bool,
    /// Custom data
    pub custom_data: HashMap<String, String>,
    /// Element-specific data
    pub data: UiElementData,
}

impl UiElement {
    /// Create a new UI element
    pub fn new(element_type: UiElementType, id: UiElementId) -> Self {
        Self {
            id,
            element_type,
            parent_id: None,
            children: Vec::new(),
            name: None,
            style: UiVisualStyle::default(),
            position: (0.0, 0.0),
            size: (0.0, 0.0),
            min_size: None,
            max_size: None,
            constraints: UiConstraints::default(),
            anchor: (0.0, 0.0),
            pivot: (0.0, 0.0),
            layout: UiLayoutMode::Absolute,
            layout_spacing: 0.0,
            rotation: 0.0,
            scale: (1.0, 1.0),
            interactive: false,
            enabled: true,
            focused: false,
            hovered: false,
            pressed: false,
            selected: false,
            custom_data: HashMap::new(),
            data: UiElementData::default(),
        }
    }
    
    /// Create a new container element
    pub fn container(id: UiElementId) -> Self {
        let mut element = Self::new(UiElementType::Container, id);
        element.interactive = false;
        element
    }
    
    /// Create a new text element
    pub fn text(id: UiElementId, content: String) -> Self {
        let mut element = Self::new(UiElementType::Text, id);
        element.data = UiElementData::Text { content };
        element
    }
    
    /// Create a new image element
    pub fn image(id: UiElementId, texture_id: u64) -> Self {
        let mut element = Self::new(UiElementType::Image, id);
        element.data = UiElementData::Image { 
            texture_id,
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
        };
        element
    }
    
    /// Create a new button element
    pub fn button(id: UiElementId, text: String) -> Self {
        let mut element = Self::new(UiElementType::Button, id);
        element.interactive = true;
        element.data = UiElementData::Button { 
            text,
            normal_color: [0.3, 0.3, 0.3, 1.0],
            hover_color: [0.4, 0.4, 0.4, 1.0],
            pressed_color: [0.2, 0.2, 0.2, 1.0],
            disabled_color: [0.1, 0.1, 0.1, 1.0],
        };
        element
    }
    
    /// Create a new input field element
    pub fn input_field(id: UiElementId, placeholder: String, value: String) -> Self {
        let mut element = Self::new(UiElementType::InputField, id);
        element.interactive = true;
        element.data = UiElementData::InputField { 
            placeholder,
            value,
            max_length: None,
            multiline: false,
            password: false,
        };
        element
    }
    
    /// Create a new checkbox element
    pub fn checkbox(id: UiElementId, checked: bool, label: String) -> Self {
        let mut element = Self::new(UiElementType::Checkbox, id);
        element.interactive = true;
        element.data = UiElementData::Checkbox { 
            checked,
            label,
        };
        element
    }
    
    /// Create a new slider element
    pub fn slider(id: UiElementId, min: f32, max: f32, value: f32) -> Self {
        let mut element = Self::new(UiElementType::Slider, id);
        element.interactive = true;
        element.data = UiElementData::Slider { 
            min,
            max,
            value,
            step: None,
            orientation: SliderOrientation::Horizontal,
        };
        element
    }
    
    /// Create a new progress bar element
    pub fn progress_bar(id: UiElementId, progress: f32) -> Self {
        let mut element = Self::new(UiElementType::ProgressBar, id);
        element.data = UiElementData::ProgressBar { 
            progress,
            show_text: true,
            text_format: "{:.0}%".to_string(),
        };
        element
    }
    
    /// Create a new panel element
    pub fn panel(id: UiElementId) -> Self {
        let mut element = Self::new(UiElementType::Panel, id);
        element.style.background_color = Some([0.15, 0.15, 0.15, 0.9]);
        element.style.corner_radius = 4.0;
        element.style.padding = [8.0, 8.0, 8.0, 8.0];
        element
    }

    /// A button in the Nvidia GeForce Experience palette.
    pub fn nvidia_button(id: UiElementId, text: String) -> Self {
        let mut element = Self::button(id, text);
        element.apply_nvidia_button_style();
        element
    }

    /// A panel in the Nvidia GeForce Experience palette.
    pub fn nvidia_panel(id: UiElementId) -> Self {
        let mut element = Self::panel(id);
        element.apply_nvidia_panel_style();
        element
    }

    /// An input field in the Nvidia GeForce Experience palette.
    pub fn nvidia_input(id: UiElementId, placeholder: String, value: String) -> Self {
        let mut element = Self::input_field(id, placeholder, value);
        element.apply_nvidia_input_style();
        element
    }

    /// A progress bar in the Nvidia GeForce Experience palette.
    pub fn nvidia_progress(id: UiElementId, progress: f32) -> Self {
        let mut element = Self::progress_bar(id, progress);
        element.apply_nvidia_progress_style();
        element
    }
    
    /// Set element name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }
    
    /// Set element position
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.position = (x, y);
        self
    }
    
    /// Set element size
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = (width, height);
        self
    }
    
    /// Set element anchor
    pub fn with_anchor(mut self, anchor_x: f32, anchor_y: f32) -> Self {
        self.anchor = (anchor_x, anchor_y);
        self
    }
    
    /// Set element pivot
    pub fn with_pivot(mut self, pivot_x: f32, pivot_y: f32) -> Self {
        self.pivot = (pivot_x, pivot_y);
        self
    }
    
    /// Set element rotation
    pub fn with_rotation(mut self, rotation: f32) -> Self {
        self.rotation = rotation;
        self
    }
    
    /// Set element scale
    pub fn with_scale(mut self, scale_x: f32, scale_y: f32) -> Self {
        self.scale = (scale_x, scale_y);
        self
    }
    
    /// Set element style
    pub fn with_style(mut self, style: UiVisualStyle) -> Self {
        self.style = style;
        self
    }
    
    /// Update element style
    pub fn update_style(&mut self, style: UiVisualStyle) {
        self.style = style;
    }
    
    /// Set element visibility
    pub fn set_visible(&mut self, visible: bool) {
        self.style.visible = visible;
    }
    
    /// Set element enabled state
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    /// Set element focused state
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
    
    /// Set element hovered state
    pub fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }
    
    /// Set element pressed state
    pub fn set_pressed(&mut self, pressed: bool) {
        self.pressed = pressed;
    }
    
    /// Set element selected state
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }
    
    /// Add a child element
    pub fn add_child(&mut self, child_id: UiElementId) {
        self.children.push(child_id);
    }
    
    /// Remove a child element
    pub fn remove_child(&mut self, child_id: UiElementId) {
        self.children.retain(|&id| id != child_id);
    }
    
    /// Get element type name
    pub fn type_name(&self) -> &'static str {
        match self.element_type {
            UiElementType::Container => "Container",
            UiElementType::Text => "Text",
            UiElementType::Image => "Image",
            UiElementType::Button => "Button",
            UiElementType::InputField => "InputField",
            UiElementType::Checkbox => "Checkbox",
            UiElementType::RadioButton => "RadioButton",
            UiElementType::Slider => "Slider",
            UiElementType::ProgressBar => "ProgressBar",
            UiElementType::Panel => "Panel",
            UiElementType::ScrollContainer => "ScrollContainer",
            UiElementType::Dropdown => "Dropdown",
            UiElementType::ListView => "ListView",
            UiElementType::GridView => "GridView",
            UiElementType::Custom => "Custom",
        }
    }
    
    /// Check if element is a container
    pub fn is_container(&self) -> bool {
        matches!(
            self.element_type,
            UiElementType::Container | UiElementType::Panel | UiElementType::ScrollContainer
        )
    }
    
    /// Check if element is interactive
    pub fn is_interactive(&self) -> bool {
        self.interactive && self.enabled && self.style.visible
    }
    
    /// Check if point is inside element
    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        if !self.style.visible || !self.enabled {
            return false;
        }
        
        let (pos_x, pos_y) = self.position;
        let (size_w, size_h) = self.size;
        
        // Apply anchor
        let anchor_x = self.anchor.0;
        let anchor_y = self.anchor.1;
        
        let min_x = pos_x - anchor_x * size_w;
        let max_x = pos_x + (1.0 - anchor_x) * size_w;
        let min_y = pos_y - anchor_y * size_h;
        let max_y = pos_y + (1.0 - anchor_y) * size_h;
        
        x >= min_x && x <= max_x && y >= min_y && y <= max_y
    }
}

/// Builder for UI elements
pub struct UiElementBuilder {
    element: UiElement,
}

impl UiElementBuilder {
    pub fn new(element_type: UiElementType, id: UiElementId) -> Self {
        Self {
            element: UiElement::new(element_type, id),
        }
    }
    
    pub fn with_name(mut self, name: String) -> Self {
        self.element.name = Some(name);
        self
    }
    
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.element.position = (x, y);
        self
    }
    
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.element.size = (width, height);
        self
    }
    
    pub fn with_anchor(mut self, anchor_x: f32, anchor_y: f32) -> Self {
        self.element.anchor = (anchor_x, anchor_y);
        self
    }

    pub fn with_min_size(mut self, width: f32, height: f32) -> Self {
        self.element.min_size = Some((width, height));
        self
    }

    pub fn with_max_size(mut self, width: f32, height: f32) -> Self {
        self.element.max_size = Some((width, height));
        self
    }

    pub fn with_constraints(mut self, constraints: UiConstraints) -> Self {
        self.element.constraints = constraints;
        self
    }

    /// Arrange the children in a row or column, `spacing` pixels apart.
    pub fn with_layout(mut self, layout: UiLayoutMode, spacing: f32) -> Self {
        self.element.layout = layout;
        self.element.layout_spacing = spacing;
        self
    }
    
    pub fn with_style(mut self, style: UiVisualStyle) -> Self {
        self.element.style = style;
        self
    }
    
    pub fn build(self) -> UiElement {
        self.element
    }
}

impl From<UiElement> for UiElementBuilder {
    fn from(element: UiElement) -> Self {
        Self { element }
    }
}

/// The Nvidia GeForce Experience look, applied to a single element.
///
/// The palette already existed in [`super::theme::nvidia_colors`] and the trait
/// already existed here, but nothing implemented it, so "apply the Nvidia
/// style" was a promise no element could keep. A button also carries its own
/// four state colours in [`UiElementData::Button`]; those are repainted too,
/// because the renderer reads them and would otherwise keep drawing the grey
/// defaults over the green fill.
impl NvidiaStyle for UiElement {
    fn apply_nvidia_button_style(&mut self) {
        use super::theme::nvidia_colors as c;
        if let UiElementData::Button {
            normal_color,
            hover_color,
            pressed_color,
            disabled_color,
            ..
        } = &mut self.data
        {
            *normal_color = c::NVIDIA_GREEN;
            *hover_color = c::NVIDIA_GREEN_LIGHT;
            *pressed_color = c::NVIDIA_GREEN_DARK;
            *disabled_color = c::BACKGROUND_TERTIARY;
        }
        self.style.background_color = Some(c::NVIDIA_GREEN);
        // Dark text on the green fill; the default light foreground would wash
        // out against it.
        self.style.foreground_color = Some(c::BACKGROUND_PRIMARY);
        self.style.border_color = Some(c::NVIDIA_GREEN_LIGHT);
        self.style.border_width = 1.0;
        self.style.corner_radius = 4.0;
    }

    fn apply_nvidia_panel_style(&mut self) {
        use super::theme::nvidia_colors as c;
        self.style.background_color = Some(c::PANEL_BACKGROUND);
        self.style.border_color = Some(c::BORDER);
        self.style.border_width = 1.0;
        self.style.corner_radius = 6.0;
        self.style.padding = [8.0, 8.0, 8.0, 8.0];
    }

    fn apply_nvidia_input_style(&mut self) {
        use super::theme::nvidia_colors as c;
        self.style.background_color = Some(c::BACKGROUND_SECONDARY);
        self.style.foreground_color = Some(c::TEXT_PRIMARY);
        self.style.border_color = Some(c::BORDER);
        self.style.border_width = 1.0;
        self.style.corner_radius = 3.0;
        self.style.padding = [10.0, 6.0, 10.0, 6.0];
    }

    fn apply_nvidia_progress_style(&mut self) {
        use super::theme::nvidia_colors as c;
        self.style.background_color = Some(c::PROGRESS_BACKGROUND);
        // A progress bar has no colour of its own, so the fill is the
        // foreground: the trough is the background and the bar is what is
        // painted over it.
        self.style.foreground_color = Some(c::PROGRESS_FILL);
        self.style.border_width = 0.0;
        self.style.corner_radius = 3.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::nvidia_colors as c;

    #[test]
    fn a_plain_button_is_not_green() {
        // The negative control for the tests below: if the default button were
        // already green, none of them would prove the trait did anything.
        let button = UiElement::button(1, "Play".to_string());
        match &button.data {
            UiElementData::Button { normal_color, .. } => {
                assert_ne!(*normal_color, c::NVIDIA_GREEN);
            }
            other => panic!("expected a button, got {other:?}"),
        }
    }

    #[test]
    fn an_nvidia_button_paints_the_fill_and_its_states() {
        let button = UiElement::nvidia_button(1, "Play".to_string());
        assert_eq!(button.style.background_color, Some(c::NVIDIA_GREEN));
        assert_eq!(button.style.corner_radius, 4.0);
        match &button.data {
            UiElementData::Button {
                normal_color,
                hover_color,
                pressed_color,
                ..
            } => {
                assert_eq!(*normal_color, c::NVIDIA_GREEN);
                assert_eq!(*hover_color, c::NVIDIA_GREEN_LIGHT);
                assert_eq!(*pressed_color, c::NVIDIA_GREEN_DARK);
            }
            other => panic!("expected a button, got {other:?}"),
        }
    }

    #[test]
    fn an_nvidia_panel_uses_the_panel_palette() {
        let panel = UiElement::nvidia_panel(1);
        assert_eq!(panel.style.background_color, Some(c::PANEL_BACKGROUND));
        assert_eq!(panel.style.border_color, Some(c::BORDER));
        assert_eq!(panel.style.corner_radius, 6.0);
    }

    #[test]
    fn an_nvidia_input_uses_the_secondary_background() {
        let input = UiElement::nvidia_input(1, "host".to_string(), String::new());
        assert_eq!(input.style.background_color, Some(c::BACKGROUND_SECONDARY));
        assert_eq!(input.style.foreground_color, Some(c::TEXT_PRIMARY));
        assert_eq!(input.style.padding, [10.0, 6.0, 10.0, 6.0]);
    }

    #[test]
    fn an_nvidia_progress_paints_the_fill_as_its_foreground() {
        let bar = UiElement::nvidia_progress(1, 0.5);
        assert_eq!(bar.style.background_color, Some(c::PROGRESS_BACKGROUND));
        assert_eq!(bar.style.foreground_color, Some(c::PROGRESS_FILL));
    }
}
