//! HUD (Head-Up Display)
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::meshes::Mesh;
use crate::render::textures::Texture;
use glam::{Vec2, Vec4};
use std::sync::Arc;

/// HUD element alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum HUDAlignment {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    #[default]
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}


/// HUD element
#[derive(Debug, Clone)]
pub struct HUDElement {
    pub name: String,
    pub position: Vec2,
    pub size: Vec2,
    pub alignment: HUDAlignment,
    pub color: Vec4,
    pub texture: Option<Arc<Texture>>,
    pub mesh: Option<Arc<Mesh>>,
    pub visible: bool,
    pub z_index: i32,
}

impl HUDElement {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            position: Vec2::ZERO,
            size: Vec2::ONE,
            alignment: HUDAlignment::Center,
            color: Vec4::ONE,
            texture: None,
            mesh: None,
            visible: true,
            z_index: 0,
        }
    }

    pub fn with_position(mut self, position: Vec2) -> Self {
        self.position = position;
        self
    }

    pub fn with_size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    pub fn with_alignment(mut self, alignment: HUDAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn with_color(mut self, color: Vec4) -> Self {
        self.color = color;
        self
    }

    pub fn with_texture(mut self, texture: Arc<Texture>) -> Self {
        self.texture = Some(texture);
        self
    }

    pub fn with_mesh(mut self, mesh: Arc<Mesh>) -> Self {
        self.mesh = Some(mesh);
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_z_index(mut self, z_index: i32) -> Self {
        self.z_index = z_index;
        self
    }

    /// Calculate final position based on alignment
    pub fn calculate_position(&self, screen_size: Vec2) -> Vec2 {
        match self.alignment {
            HUDAlignment::TopLeft => self.position,
            HUDAlignment::TopCenter => {
                Vec2::new(screen_size.x / 2.0 + self.position.x, self.position.y)
            }
            HUDAlignment::TopRight => Vec2::new(screen_size.x - self.position.x, self.position.y),
            HUDAlignment::CenterLeft => {
                Vec2::new(self.position.x, screen_size.y / 2.0 + self.position.y)
            }
            HUDAlignment::Center => Vec2::new(
                screen_size.x / 2.0 + self.position.x,
                screen_size.y / 2.0 + self.position.y,
            ),
            HUDAlignment::CenterRight => Vec2::new(
                screen_size.x - self.position.x,
                screen_size.y / 2.0 + self.position.y,
            ),
            HUDAlignment::BottomLeft => Vec2::new(self.position.x, screen_size.y - self.position.y),
            HUDAlignment::BottomCenter => Vec2::new(
                screen_size.x / 2.0 + self.position.x,
                screen_size.y - self.position.y,
            ),
            HUDAlignment::BottomRight => Vec2::new(
                screen_size.x - self.position.x,
                screen_size.y - self.position.y,
            ),
        }
    }
}

impl Default for HUDElement {
    fn default() -> Self {
        Self::new("default")
    }
}

/// HUD (Head-Up Display)
pub struct HUD {
    /// All HUD elements
    elements: Vec<HUDElement>,
    /// Screen size
    screen_size: Vec2,
    /// Crosshair
    crosshair: Option<HUDElement>,
    /// Ship information
    ship_info: ShipHUDInfo,
    /// Weapon information
    weapon_info: WeaponHUDInfo,
    /// Player information
    player_info: PlayerHUDInfo,
    /// Objectives
    objectives: Vec<HUDObjective>,
    /// Compass
    compass: Option<HUDCompass>,
    /// Damage indicators
    damage_indicators: Vec<HUDDamageIndicator>,
}

impl HUD {
    pub fn new(screen_width: u32, screen_height: u32) -> Self {
        Self {
            elements: Vec::new(),
            screen_size: Vec2::new(screen_width as f32, screen_height as f32),
            crosshair: None,
            ship_info: ShipHUDInfo::default(),
            weapon_info: WeaponHUDInfo::default(),
            player_info: PlayerHUDInfo::default(),
            objectives: Vec::new(),
            compass: None,
            damage_indicators: Vec::new(),
        }
    }

    pub fn screen_size(&self) -> Vec2 {
        self.screen_size
    }

    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_size = Vec2::new(width as f32, height as f32);
    }

    pub fn add_element(&mut self, element: HUDElement) {
        self.elements.push(element);
    }

    pub fn remove_element(&mut self, name: &str) -> Option<HUDElement> {
        let index = self.elements.iter().position(|e| e.name == name);
        if let Some(index) = index {
            Some(self.elements.remove(index))
        } else {
            None
        }
    }

    pub fn get_element(&self, name: &str) -> Option<&HUDElement> {
        self.elements.iter().find(|e| e.name == name)
    }

    pub fn get_element_mut(&mut self, name: &str) -> Option<&mut HUDElement> {
        self.elements.iter_mut().find(|e| e.name == name)
    }

    pub fn set_crosshair(&mut self, element: HUDElement) {
        self.crosshair = Some(element);
    }

    pub fn set_ship_info(&mut self, info: ShipHUDInfo) {
        self.ship_info = info;
    }

    pub fn set_weapon_info(&mut self, info: WeaponHUDInfo) {
        self.weapon_info = info;
    }

    pub fn set_player_info(&mut self, info: PlayerHUDInfo) {
        self.player_info = info;
    }

    pub fn add_objective(&mut self, objective: HUDObjective) {
        self.objectives.push(objective);
    }

    pub fn set_compass(&mut self, compass: HUDCompass) {
        self.compass = Some(compass);
    }

    pub fn add_damage_indicator(&mut self, indicator: HUDDamageIndicator) {
        self.damage_indicators.push(indicator);
    }

    pub fn update(&mut self, delta_time: std::time::Duration) {
        // Update all elements
        for _element in &mut self.elements {
            // Update animations, etc.
        }

        // Update damage indicators
        for indicator in &mut self.damage_indicators {
            indicator.update(delta_time);
        }

        // Remove expired damage indicators
        self.damage_indicators.retain(|i| !i.expired());
    }

    pub fn render(&self) {
        // Render all HUD elements
        for element in &self.elements {
            if element.visible {
                self.render_element(element);
            }
        }

        // Render crosshair
        if let Some(crosshair) = &self.crosshair {
            if crosshair.visible {
                self.render_element(crosshair);
            }
        }

        // Render ship info
        self.render_ship_info();

        // Render weapon info
        self.render_weapon_info();

        // Render player info
        self.render_player_info();

        // Render objectives
        self.render_objectives();

        // Render compass
        if let Some(compass) = &self.compass {
            self.render_compass(compass);
        }

        // Render damage indicators
        self.render_damage_indicators();
    }

    fn render_element(&self, _element: &HUDElement) {
        // Render a single HUD element
        // This would use the UI render pass
    }

    fn render_ship_info(&self) {
        // Render ship information (HP, speed, fuel, etc.)
    }

    fn render_weapon_info(&self) {
        // Render weapon information (ammo, cooldown, etc.)
    }

    fn render_player_info(&self) {
        // Render player information (health, role, etc.)
    }

    fn render_objectives(&self) {
        // Render objectives
    }

    fn render_compass(&self, _compass: &HUDCompass) {
        // Render compass
    }

    fn render_damage_indicators(&self) {
        // Render damage indicators
    }
}

impl Default for HUD {
    fn default() -> Self {
        Self::new(1920, 1080)
    }
}

/// Ship HUD information
#[derive(Debug, Clone, Default)]
pub struct ShipHUDInfo {
    pub name: String,
    pub hp: f32,
    pub max_hp: f32,
    pub speed: f32,
    pub max_speed: f32,
    pub fuel: f32,
    pub max_fuel: f32,
    pub crew_count: usize,
    pub max_crew: usize,
    pub fire_level: f32,
    pub water_level: f32,
}

/// Weapon HUD information
#[derive(Debug, Clone, Default)]
pub struct WeaponHUDInfo {
    pub name: String,
    pub ammo: usize,
    pub max_ammo: usize,
    pub cooldown: f32,
    pub max_cooldown: f32,
    pub ready: bool,
    pub reloading: bool,
}

/// Player HUD information
#[derive(Debug, Clone, Default)]
pub struct PlayerHUDInfo {
    pub name: String,
    pub health: f32,
    pub max_health: f32,
    pub role: String,
    pub team: String,
    pub alive: bool,
}

/// HUD objective
#[derive(Debug, Clone)]
pub struct HUDObjective {
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub position: Vec2,
    pub visible: bool,
}

impl Default for HUDObjective {
    fn default() -> Self {
        Self {
            title: "Objective".to_string(),
            description: "Complete the objective".to_string(),
            completed: false,
            position: Vec2::ZERO,
            visible: true,
        }
    }
}

/// HUD compass
#[derive(Debug, Clone)]
pub struct HUDCompass {
    pub position: Vec2,
    pub size: Vec2,
    pub north_direction: f32,
    pub visible: bool,
    pub markers: Vec<CompassMarker>,
}

impl Default for HUDCompass {
    fn default() -> Self {
        Self {
            position: Vec2::new(0.0, -100.0),
            size: Vec2::new(200.0, 40.0),
            north_direction: 0.0,
            visible: true,
            markers: Vec::new(),
        }
    }
}

/// Compass marker
#[derive(Debug, Clone)]
pub struct CompassMarker {
    pub name: String,
    pub direction: f32,
    pub distance: f32,
    pub color: Vec4,
    pub icon: Option<Arc<Texture>>,
}

impl Default for CompassMarker {
    fn default() -> Self {
        Self {
            name: "Marker".to_string(),
            direction: 0.0,
            distance: 0.0,
            color: Vec4::ONE,
            icon: None,
        }
    }
}

/// HUD damage indicator
#[derive(Debug, Clone)]
pub struct HUDDamageIndicator {
    pub position: Vec2,
    pub direction: Vec2,
    pub intensity: f32,
    pub color: Vec4,
    pub size: Vec2,
    pub lifetime: f32,
    pub max_lifetime: f32,
}

impl HUDDamageIndicator {
    pub fn new(position: Vec2, direction: Vec2, intensity: f32) -> Self {
        Self {
            position,
            direction,
            intensity,
            color: Vec4::new(1.0, 0.0, 0.0, 0.8),
            size: Vec2::new(20.0, 2.0),
            lifetime: 0.0,
            max_lifetime: 1.0,
        }
    }

    pub fn update(&mut self, delta_time: std::time::Duration) {
        self.lifetime += delta_time.as_secs_f32();
    }

    pub fn expired(&self) -> bool {
        self.lifetime >= self.max_lifetime
    }
}

impl Default for HUDDamageIndicator {
    fn default() -> Self {
        Self::new(Vec2::ZERO, Vec2::ZERO, 1.0)
    }
}
