//! Menu
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::textures::Texture;
use glam::{Vec2, Vec4};
use std::sync::Arc;

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

    pub fn render(&self) {
        // Render menu background
        self.render_background();

        // Render menu title
        self.render_title();

        // Render menu items
        for (index, item) in self.items.iter().enumerate() {
            let is_selected = index == self.selected_index;
            self.render_item(item, is_selected);
        }
    }

    fn render_background(&self) {
        // Render menu background
    }

    fn render_title(&self) {
        // Render menu title
    }

    fn render_item(&self, item: &MenuItem, is_selected: bool) {
        // Render menu item
        match item.item_type {
            MenuItemType::Button => self.render_button(item, is_selected),
            MenuItemType::Label => self.render_label(item),
            MenuItemType::Slider => self.render_slider(item, is_selected),
            MenuItemType::Dropdown => self.render_dropdown(item, is_selected),
            MenuItemType::Checkbox => self.render_checkbox(item, is_selected),
            MenuItemType::TextInput => self.render_text_input(item, is_selected),
        }
    }

    fn render_button(&self, _item: &MenuItem, _is_selected: bool) {
        // Render button
    }

    fn render_label(&self, _item: &MenuItem) {
        // Render label
    }

    fn render_slider(&self, _item: &MenuItem, _is_selected: bool) {
        // Render slider
    }

    fn render_dropdown(&self, _item: &MenuItem, _is_selected: bool) {
        // Render dropdown
    }

    fn render_checkbox(&self, _item: &MenuItem, _is_selected: bool) {
        // Render checkbox
    }

    fn render_text_input(&self, _item: &MenuItem, _is_selected: bool) {
        // Render text input
    }
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
