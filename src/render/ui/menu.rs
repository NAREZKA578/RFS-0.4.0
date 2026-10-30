//! Menu
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::rhi::ui::UiCommand;
use crate::render::textures::Texture;
use glam::{Vec2, Vec4};
use std::sync::Arc;

use super::model::{rgba, UiBatch};

/// Menu layout. Asserted exactly in `ui_pass_model.rs`, so it is named once
/// here and cited there.
const TITLE_SIZE: f32 = 24.0;
const ITEM_TEXT_SIZE: f32 = 16.0;
/// Vertical distance between stacked item tops.
const ITEM_PITCH: f32 = 48.0;
/// Height of the title zone, where stacked items start below.
const HEADER_HEIGHT: f32 = 64.0;
const MENU_PAD: f32 = 24.0;
/// Text inset inside an item's own rectangle.
const BOX_PAD: f32 = 8.0;
const CHECKBOX_BOX: f32 = 20.0;
/// Inside inset of a checkbox's filled square: 20 - 4 - 4 = 12 side.
const CHECKBOX_FILL_INSET: f32 = 4.0;
const OUTLINE_THICKNESS: f32 = 2.0;
const TITLE_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const ITEM_TEXT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const TRACK_COLOR: [f32; 4] = [0.15, 0.15, 0.15, 1.0];

/// Menu type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum MenuType {
    #[default]
    Main,
    Settings,
    ShipSelection,
    LoadoutSelection,
    Pause,
    InGame,
    Scoreboard,
}


/// Menu
pub struct Menu {
    pub menu_type: MenuType,
    pub title: String,
    pub position: Vec2,
    pub size: Vec2,
    pub background_color: Vec4,
    pub background_texture: Option<Arc<Texture>>,
    pub visible: bool,
    pub items: Vec<MenuItem>,
    pub selected_index: usize,
}

impl Menu {
    pub fn new(menu_type: MenuType) -> Self {
        let (title, items) = match menu_type {
            MenuType::Main => (
                "Main Menu".to_string(),
                vec![
                    MenuItem::new("Single Player", MenuItemType::Button),
                    MenuItem::new("Multiplayer", MenuItemType::Button),
                    MenuItem::new("Settings", MenuItemType::Button),
                    MenuItem::new("Quit", MenuItemType::Button),
                ],
            ),
            MenuType::Settings => (
                "Settings".to_string(),
                vec![
                    MenuItem::new("Graphics", MenuItemType::Slider),
                    MenuItem::new("Audio", MenuItemType::Slider),
                    MenuItem::new("Controls", MenuItemType::Button),
                    MenuItem::new("Back", MenuItemType::Button),
                ],
            ),
            MenuType::ShipSelection => (
                "Ship Selection".to_string(),
                vec![
                    MenuItem::new("Frigate", MenuItemType::Button),
                    MenuItem::new("Destroyer", MenuItemType::Button),
                    MenuItem::new("Battleship", MenuItemType::Button),
                    MenuItem::new("Back", MenuItemType::Button),
                ],
            ),
            MenuType::LoadoutSelection => (
                "Loadout Selection".to_string(),
                vec![
                    MenuItem::new("Primary Weapon", MenuItemType::Dropdown),
                    MenuItem::new("Secondary Weapon", MenuItemType::Dropdown),
                    MenuItem::new("Armor", MenuItemType::Slider),
                    MenuItem::new("Back", MenuItemType::Button),
                ],
            ),
            MenuType::Pause => (
                "Pause".to_string(),
                vec![
                    MenuItem::new("Resume", MenuItemType::Button),
                    MenuItem::new("Settings", MenuItemType::Button),
                    MenuItem::new("Quit to Menu", MenuItemType::Button),
                    MenuItem::new("Quit Game", MenuItemType::Button),
                ],
            ),
            MenuType::InGame => (
                "".to_string(),
                vec![
                    MenuItem::new("Objective", MenuItemType::Label),
                    MenuItem::new("Score: 0", MenuItemType::Label),
                    MenuItem::new("Time: 00:00", MenuItemType::Label),
                ],
            ),
            MenuType::Scoreboard => (
                "Scoreboard".to_string(),
                vec![
                    MenuItem::new("Player 1 - 100", MenuItemType::Label),
                    MenuItem::new("Player 2 - 80", MenuItemType::Label),
                    MenuItem::new("Player 3 - 60", MenuItemType::Label),
                    MenuItem::new("Back", MenuItemType::Button),
                ],
            ),
        };

        Self {
            menu_type,
            title,
            position: Vec2::new(0.0, 0.0),
            size: Vec2::new(400.0, 600.0),
            background_color: Vec4::new(0.1, 0.1, 0.1, 0.9),
            background_texture: None,
            visible: true,
            items,
            selected_index: 0,
        }
    }

    pub fn menu_type(&self) -> MenuType {
        self.menu_type
    }

    pub fn set_menu_type(&mut self, menu_type: MenuType) {
        self.menu_type = menu_type;
        // Rebuild menu items for the new type
        let (title, items) = match menu_type {
            MenuType::Main => (
                "Main Menu".to_string(),
                vec![
                    MenuItem::new("Single Player", MenuItemType::Button),
                    MenuItem::new("Multiplayer", MenuItemType::Button),
                    MenuItem::new("Settings", MenuItemType::Button),
                    MenuItem::new("Quit", MenuItemType::Button),
                ],
            ),
            MenuType::Settings => (
                "Settings".to_string(),
                vec![
                    MenuItem::new("Graphics", MenuItemType::Slider),
                    MenuItem::new("Audio", MenuItemType::Slider),
                    MenuItem::new("Controls", MenuItemType::Button),
                    MenuItem::new("Back", MenuItemType::Button),
                ],
            ),
            MenuType::ShipSelection => (
                "Ship Selection".to_string(),
                vec![
                    MenuItem::new("Frigate", MenuItemType::Button),
                    MenuItem::new("Destroyer", MenuItemType::Button),
                    MenuItem::new("Battleship", MenuItemType::Button),
                    MenuItem::new("Back", MenuItemType::Button),
                ],
            ),
            MenuType::LoadoutSelection => (
                "Loadout Selection".to_string(),
                vec![
                    MenuItem::new("Primary Weapon", MenuItemType::Dropdown),
                    MenuItem::new("Secondary Weapon", MenuItemType::Dropdown),
                    MenuItem::new("Armor", MenuItemType::Slider),
                    MenuItem::new("Back", MenuItemType::Button),
                ],
            ),
            MenuType::Pause => (
                "Pause".to_string(),
                vec![
                    MenuItem::new("Resume", MenuItemType::Button),
                    MenuItem::new("Settings", MenuItemType::Button),
                    MenuItem::new("Quit to Menu", MenuItemType::Button),
                    MenuItem::new("Quit Game", MenuItemType::Button),
                ],
            ),
            MenuType::InGame => (
                "".to_string(),
                vec![
                    MenuItem::new("Objective", MenuItemType::Label),
                    MenuItem::new("Score: 0", MenuItemType::Label),
                    MenuItem::new("Time: 00:00", MenuItemType::Label),
                ],
            ),
            MenuType::Scoreboard => (
                "Scoreboard".to_string(),
                vec![
                    MenuItem::new("Player 1 - 100", MenuItemType::Label),
                    MenuItem::new("Player 2 - 80", MenuItemType::Label),
                    MenuItem::new("Player 3 - 60", MenuItemType::Label),
                    MenuItem::new("Back", MenuItemType::Button),
                ],
            ),
        };

        self.title = title;
        self.items = items;
        self.selected_index = 0;
    }

    pub fn set_position(&mut self, position: Vec2) {
        self.position = position;
    }

    pub fn set_size(&mut self, size: Vec2) {
        self.size = size;
    }

    pub fn set_background_color(&mut self, color: Vec4) {
        self.background_color = color;
    }

    pub fn set_background_texture(&mut self, texture: Arc<Texture>) {
        self.background_texture = Some(texture);
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn add_item(&mut self, item: MenuItem) {
        self.items.push(item);
    }

    pub fn remove_item(&mut self, index: usize) -> Option<MenuItem> {
        if index < self.items.len() {
            Some(self.items.remove(index))
        } else {
            None
        }
    }

    pub fn get_item(&self, index: usize) -> Option<&MenuItem> {
        self.items.get(index)
    }

    pub fn get_item_mut(&mut self, index: usize) -> Option<&mut MenuItem> {
        self.items.get_mut(index)
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn set_selected_index(&mut self, index: usize) {
        if index < self.items.len() {
            self.selected_index = index;
        }
    }

    pub fn next_item(&mut self) {
        if self.selected_index + 1 < self.items.len() {
            self.selected_index += 1;
        }
    }

    pub fn previous_item(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn select_item(&mut self) -> Option<&MenuItem> {
        self.items.get(self.selected_index)
    }

    pub fn update(&mut self, delta_time: std::time::Duration) {
        // Update menu items
        for item in &mut self.items {
            item.update(delta_time);
        }
    }

    /// Compiles the menu into `UiCommand`s, replacing the stub `render` and
    /// its nine `render_*` helpers this once replaced (Bug №724).
    ///
    /// Layout: the background fills `position..position+size`; the title text
    /// sits at the top-left padding; items are either at their explicit
    /// `position` or stacked one `ITEM_PITCH` apart below the header. Selected
    /// highlighting uses the menu's `selected_index` when the item is enabled;
    /// otherwise the item's own colour rule applies.
    ///
    /// v1 scope, on the record: a dropdown renders like a button (no expanded
    /// option list) and carries only its name — per-option lists are v2.
    pub fn to_commands(&self, batch: &mut UiBatch) {
        if !self.visible {
            return;
        }
        let (x, y, w, h) = (
            self.position.x,
            self.position.y,
            self.size.x,
            self.size.y,
        );

        if let Some(texture) = &self.background_texture {
            let id = batch.push_texture(texture);
            batch.commands.push(UiCommand::Image {
                texture_id: id,
                x,
                y,
                width: w,
                height: h,
                uv_min: [0.0, 0.0],
                uv_max: [1.0, 1.0],
                color: rgba(self.background_color),
            });
        } else {
            batch.commands.push(UiCommand::Rectangle {
                x,
                y,
                width: w,
                height: h,
                color: rgba(self.background_color),
                corner_radius: 0.0,
            });
        }

        if !self.title.is_empty() {
            batch.commands.push(UiCommand::Text {
                content: self.title.clone(),
                x: x + MENU_PAD,
                y: y + MENU_PAD,
                size: TITLE_SIZE,
                color: TITLE_COLOR,
                font: None,
            });
        }

        for (index, item) in self.items.iter().enumerate() {
            let at = if item.position == Vec2::ZERO {
                Vec2::new(
                    x + MENU_PAD,
                    y + HEADER_HEIGHT + index as f32 * ITEM_PITCH,
                )
            } else {
                item.position
            };
            let color = if index == self.selected_index && item.enabled {
                rgba(item.selected_color)
            } else {
                rgba(item.color())
            };
            match item.item_type {
                MenuItemType::Button | MenuItemType::Dropdown => {
                    batch.commands.push(rect(at, item.size, color));
                    push_item_text(batch, item, at);
                }
                MenuItemType::Label => push_item_text(batch, item, at),
                MenuItemType::Slider => {
                    batch.commands.push(UiCommand::ProgressBar {
                        x: at.x,
                        y: at.y,
                        width: item.size.x,
                        height: item.size.y,
                        progress: range_ratio(item.value, item.min_value, item.max_value),
                        background_color: TRACK_COLOR,
                        fill_color: color,
                        corner_radius: 0.0,
                    });
                    push_item_text(batch, item, at);
                }
                MenuItemType::Checkbox => {
                    // The square is vertically centred in the item row, the
                    // label sits right of it.
                    let box_y = at.y + (item.size.y - CHECKBOX_BOX) * 0.5;
                    batch.commands.push(UiCommand::RectangleOutline {
                        x: at.x,
                        y: box_y,
                        width: CHECKBOX_BOX,
                        height: CHECKBOX_BOX,
                        color,
                        thickness: OUTLINE_THICKNESS,
                        corner_radius: 0.0,
                    });
                    if item.checked {
                        batch.commands.push(UiCommand::Rectangle {
                            x: at.x + CHECKBOX_FILL_INSET,
                            y: box_y + CHECKBOX_FILL_INSET,
                            width: CHECKBOX_BOX - CHECKBOX_FILL_INSET * 2.0,
                            height: CHECKBOX_BOX - CHECKBOX_FILL_INSET * 2.0,
                            color,
                            corner_radius: 0.0,
                        });
                    }
                    let text_at = Vec2::new(at.x + CHECKBOX_BOX, at.y);
                    push_item_text(batch, item, text_at);
                }
                MenuItemType::TextInput => {
                    batch.commands.push(UiCommand::RectangleOutline {
                        x: at.x,
                        y: at.y,
                        width: item.size.x,
                        height: item.size.y,
                        color,
                        thickness: OUTLINE_THICKNESS,
                        corner_radius: 0.0,
                    });
                    push_item_text(batch, item, at);
                }
            }
        }
    }

    /// The text an item displays. A text input shows what has been typed; when
    /// that is empty it falls back to the item's own text, which for inputs
    /// doubles as the prompt.
    fn item_content(item: &MenuItem) -> String {
        if item.input_text.is_empty() {
            item.text.clone()
        } else {
            item.input_text.clone()
        }
    }
}

/// `(value - min) / (max - min)` clamped into 0..1, degenerate range handled:
/// without the guard a zero-height range divides by zero and hands the GPU NaN.
fn range_ratio(value: f32, min: f32, max: f32) -> f32 {
    if max > min {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// A flat quad at an item's position and size. The renderer skips any
/// rectangle with a positive `corner_radius`, so v1 compiles square ones.
fn rect(at: Vec2, size: Vec2, color: [f32; 4]) -> UiCommand {
    UiCommand::Rectangle {
        x: at.x,
        y: at.y,
        width: size.x,
        height: size.y,
        color,
        corner_radius: 0.0,
    }
}

/// An item's label, centred vertically inside its own row height.
fn push_item_text(batch: &mut UiBatch, item: &MenuItem, at: Vec2) {
    let content = Menu::item_content(item);
    if content.is_empty() {
        return;
    }
    batch.commands.push(UiCommand::Text {
        content,
        x: at.x + BOX_PAD,
        y: at.y + (item.size.y - ITEM_TEXT_SIZE) * 0.5,
        size: ITEM_TEXT_SIZE,
        color: ITEM_TEXT_COLOR,
        font: None,
    });
}

impl Default for Menu {
    fn default() -> Self {
        Self::new(MenuType::Main)
    }
}

/// Menu item type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum MenuItemType {
    #[default]
    Button,
    Label,
    Slider,
    Dropdown,
    Checkbox,
    TextInput,
}


/// Menu item
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub name: String,
    pub item_type: MenuItemType,
    pub text: String,
    pub position: Vec2,
    pub size: Vec2,
    pub normal_color: Vec4,
    pub selected_color: Vec4,
    pub disabled_color: Vec4,
    pub enabled: bool,
    pub selected: bool,
    /// For sliders
    pub value: f32,
    pub min_value: f32,
    pub max_value: f32,
    /// For dropdowns
    pub options: Vec<String>,
    pub selected_option: usize,
    /// For checkboxes
    pub checked: bool,
    /// For text inputs
    pub input_text: String,
    pub cursor_position: usize,
}

impl MenuItem {
    pub fn new(name: &str, item_type: MenuItemType) -> Self {
        Self {
            name: name.to_string(),
            item_type,
            text: name.to_string(),
            position: Vec2::ZERO,
            size: Vec2::new(200.0, 40.0),
            normal_color: Vec4::new(0.8, 0.8, 0.8, 1.0),
            selected_color: Vec4::new(1.0, 1.0, 0.5, 1.0),
            disabled_color: Vec4::new(0.5, 0.5, 0.5, 0.5),
            enabled: true,
            selected: false,
            value: 0.5,
            min_value: 0.0,
            max_value: 1.0,
            options: Vec::new(),
            selected_option: 0,
            checked: false,
            input_text: String::new(),
            cursor_position: 0,
        }
    }

    pub fn with_text(mut self, text: &str) -> Self {
        self.text = text.to_string();
        self
    }

    pub fn with_position(mut self, position: Vec2) -> Self {
        self.position = position;
        self
    }

    pub fn with_size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    pub fn with_colors(mut self, normal: Vec4, selected: Vec4, disabled: Vec4) -> Self {
        self.normal_color = normal;
        self.selected_color = selected;
        self.disabled_color = disabled;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_value_range(mut self, value: f32, min: f32, max: f32) -> Self {
        self.value = value.clamp(min, max);
        self.min_value = min;
        self.max_value = max;
        self
    }

    pub fn with_options(mut self, options: Vec<String>) -> Self {
        self.options = options;
        self
    }

    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn update(&mut self, _delta_time: std::time::Duration) {
        // Update menu item (animations, etc.)
    }

    pub fn color(&self) -> Vec4 {
        if !self.enabled {
            self.disabled_color
        } else if self.selected {
            self.selected_color
        } else {
            self.normal_color
        }
    }

    pub fn set_value(&mut self, value: f32) {
        self.value = value.clamp(self.min_value, self.max_value);
    }

    pub fn set_selected_option(&mut self, index: usize) {
        if index < self.options.len() {
            self.selected_option = index;
        }
    }

    pub fn set_checked(&mut self, checked: bool) {
        self.checked = checked;
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
    }

    pub fn set_input_text(&mut self, text: &str) {
        self.input_text = text.to_string();
    }

    pub fn set_cursor_position(&mut self, position: usize) {
        self.cursor_position = position.min(self.input_text.len());
    }
}

impl Default for MenuItem {
    fn default() -> Self {
        Self::new("default", MenuItemType::Button)
    }
}
