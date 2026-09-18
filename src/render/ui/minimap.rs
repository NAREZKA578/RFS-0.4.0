//! Minimap
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::textures::Texture;
use glam::{Vec2, Vec3, Vec4};
use std::sync::Arc;

/// Minimap
pub struct Minimap {
    /// Position on screen
    pub position: Vec2,
    /// Size on screen
    pub size: Vec2,
    /// World size (in world units)
    pub world_size: Vec2,
    /// World center position
    pub world_center: Vec3,
    /// Rotation (in radians)
    pub rotation: f32,
    /// Zoom level
    pub zoom: f32,
    /// Background texture
    pub background_texture: Option<Arc<Texture>>,
    /// Fog of war texture
    pub fog_of_war_texture: Option<Arc<Texture>>,
    /// Visible
    pub visible: bool,
    /// Elements to display on minimap
    pub elements: Vec<MinimapElement>,
}

impl Minimap {
    pub fn new() -> Self {
        Self {
            position: Vec2::new(10.0, 10.0),
            size: Vec2::new(200.0, 200.0),
            world_size: Vec2::new(1000.0, 1000.0),
            world_center: Vec3::ZERO,
            rotation: 0.0,
            zoom: 1.0,
            background_texture: None,
            fog_of_war_texture: None,
            visible: true,
            elements: Vec::new(),
        }
    }

    pub fn set_position(&mut self, position: Vec2) {
        self.position = position;
    }

    pub fn set_size(&mut self, size: Vec2) {
        self.size = size;
    }

    pub fn set_world_size(&mut self, size: Vec2) {
        self.world_size = size;
    }

    pub fn set_world_center(&mut self, center: Vec3) {
        self.world_center = center;
    }

    pub fn set_rotation(&mut self, rotation: f32) {
        self.rotation = rotation;
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(0.1, 10.0);
    }

    pub fn set_background_texture(&mut self, texture: Arc<Texture>) {
        self.background_texture = Some(texture);
    }

    pub fn set_fog_of_war_texture(&mut self, texture: Arc<Texture>) {
        self.fog_of_war_texture = Some(texture);
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn add_element(&mut self, element: MinimapElement) {
        self.elements.push(element);
    }

    pub fn remove_element(&mut self, name: &str) -> Option<MinimapElement> {
        let index = self.elements.iter().position(|e| e.name == name);
        if let Some(index) = index {
            Some(self.elements.remove(index))
        } else {
            None
        }
    }

    pub fn get_element(&self, name: &str) -> Option<&MinimapElement> {
        self.elements.iter().find(|e| e.name == name)
    }

    pub fn get_element_mut(&mut self, name: &str) -> Option<&mut MinimapElement> {
        self.elements.iter_mut().find(|e| e.name == name)
    }

    pub fn update(&mut self) {
        // Update minimap elements
        for _element in &mut self.elements {
            // Update element positions, etc.
        }
    }

    /// Convert world position to minimap position
    pub fn world_to_minimap(&self, world_position: Vec3) -> Vec2 {
        let relative_pos = world_position - self.world_center;
        let normalized_pos = Vec2::new(
            relative_pos.x / self.world_size.x,
            relative_pos.z / self.world_size.y,
        );

        // Apply rotation
        let rotated_pos = Vec2::new(
            normalized_pos.x * self.rotation.cos() - normalized_pos.y * self.rotation.sin(),
            normalized_pos.x * self.rotation.sin() + normalized_pos.y * self.rotation.cos(),
        );

        // Apply zoom and position
        Vec2::new(
            self.position.x + rotated_pos.x * self.size.x * self.zoom,
            self.position.y + rotated_pos.y * self.size.y * self.zoom,
        )
    }

    /// Convert minimap position to world position
    pub fn minimap_to_world(&self, minimap_position: Vec2) -> Vec3 {
        let relative_pos = Vec2::new(
            (minimap_position.x - self.position.x) / (self.size.x * self.zoom),
            (minimap_position.y - self.position.y) / (self.size.y * self.zoom),
        );

        // Apply inverse rotation
        let rotated_pos = Vec2::new(
            relative_pos.x * self.rotation.cos() + relative_pos.y * self.rotation.sin(),
            -relative_pos.x * self.rotation.sin() + relative_pos.y * self.rotation.cos(),
        );

        self.world_center
            + Vec3::new(
                rotated_pos.x * self.world_size.x,
                0.0,
                rotated_pos.y * self.world_size.y,
            )
    }

    pub fn render(&self) {
        // Render minimap background
        if let Some(texture) = &self.background_texture {
            self.render_texture(texture, self.position, self.size, Vec4::ONE);
        }

        // Render fog of war
        if let Some(texture) = &self.fog_of_war_texture {
            self.render_texture(
                texture,
                self.position,
                self.size,
                Vec4::new(0.5, 0.5, 0.5, 0.5),
            );
        }

        // Render elements
        for element in &self.elements {
            if element.visible {
                self.render_element(element);
            }
        }
    }

    fn render_texture(&self, _texture: &Arc<Texture>, _position: Vec2, _size: Vec2, _color: Vec4) {
        // Render a texture at the specified position and size
    }

    fn render_element(&self, element: &MinimapElement) {
        // Render a minimap element
        match &element.element_type {
            MinimapElementType::Ship => self.render_ship_element(element),
            MinimapElementType::Objective => self.render_objective_element(element),
            MinimapElementType::Marker => self.render_marker_element(element),
            MinimapElementType::Area => self.render_area_element(element),
        }
    }

    fn render_ship_element(&self, _element: &MinimapElement) {
        // Render ship element
    }

    fn render_objective_element(&self, _element: &MinimapElement) {
        // Render objective element
    }

    fn render_marker_element(&self, _element: &MinimapElement) {
        // Render marker element
    }

    fn render_area_element(&self, _element: &MinimapElement) {
        // Render area element
    }
}

impl Default for Minimap {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimap element type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MinimapElementType {
    Ship,
    Objective,
    Marker,
    Area,
}

impl Default for MinimapElementType {
    fn default() -> Self {
        Self::Ship
    }
}

/// Minimap element
#[derive(Debug, Clone)]
pub struct MinimapElement {
    pub name: String,
    pub element_type: MinimapElementType,
    pub world_position: Vec3,
    pub world_size: Vec2,
    pub color: Vec4,
    pub icon: Option<Arc<Texture>>,
    pub size: Vec2,
    pub visible: bool,
    pub z_index: i32,
}

impl MinimapElement {
    pub fn new(name: &str, element_type: MinimapElementType) -> Self {
        Self {
            name: name.to_string(),
            element_type,
            world_position: Vec3::ZERO,
            world_size: Vec2::ONE,
            color: Vec4::ONE,
            icon: None,
            size: Vec2::new(10.0, 10.0),
            visible: true,
            z_index: 0,
        }
    }

    pub fn with_world_position(mut self, position: Vec3) -> Self {
        self.world_position = position;
        self
    }

    pub fn with_world_size(mut self, size: Vec2) -> Self {
        self.world_size = size;
        self
    }

    pub fn with_color(mut self, color: Vec4) -> Self {
        self.color = color;
        self
    }

    pub fn with_icon(mut self, icon: Arc<Texture>) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn with_size(mut self, size: Vec2) -> Self {
        self.size = size;
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
}

impl Default for MinimapElement {
    fn default() -> Self {
        Self::new("default", MinimapElementType::Ship)
    }
}
