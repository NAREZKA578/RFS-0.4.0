//! Scene
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::components::{
    CameraComponent, LightComponent, Renderable,
};
use super::entity::Entity;
use crate::render::core::RenderContext;
use crate::render::scene::culling::{Aabb, FrustumCuller};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::time::Duration;

/// Scene contains all entities and manages rendering
pub struct Scene {
    /// All entities in the scene
    entities: Vec<Entity>,
    /// Entity map for quick lookup
    entity_map: HashMap<usize, usize>, // entity_id -> index in entities
    /// Next entity ID
    next_entity_id: usize,
    /// Active camera
    active_camera: Option<usize>, // entity_id of active camera
    /// Frustum culler
    _frustum_culler: FrustumCuller,
    /// Scene bounding box
    bounding_box: Option<Aabb>,
    /// Scene name
    name: String,
}

impl Scene {
    /// Create a new scene
    pub fn new(name: &str) -> Self {
        Self {
            entities: Vec::new(),
            entity_map: HashMap::new(),
            next_entity_id: 1,
            active_camera: None,
            _frustum_culler: FrustumCuller::new(),
            bounding_box: None,
            name: name.to_string(),
        }
    }

    /// Get scene name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Add an entity to the scene
    pub fn add_entity(&mut self, mut entity: Entity) -> usize {
        let id = self.next_entity_id;
        entity.set_id(id);
        self.entities.push(entity);
        self.entity_map.insert(id, self.entities.len() - 1);
        self.next_entity_id += 1;
        id
    }

    /// Remove an entity from the scene
    pub fn remove_entity(&mut self, entity_id: usize) -> Option<Entity> {
        if let Some(index) = self.entity_map.remove(&entity_id) {
            let entity = self.entities.remove(index);

            // Update entity_map for entities after the removed one
            for (_, idx) in self.entity_map.iter_mut() {
                if *idx > index {
                    *idx -= 1;
                }
            }

            // If this was the active camera, clear it
            if self.active_camera == Some(entity_id) {
                self.active_camera = None;
            }

            Some(entity)
        } else {
            None
        }
    }

    /// Get an entity by ID
    pub fn get_entity(&self, entity_id: usize) -> Option<&Entity> {
        self.entity_map
            .get(&entity_id)
            .map(|&index| &self.entities[index])
    }

    /// Get a mutable entity by ID
    pub fn get_entity_mut(&mut self, entity_id: usize) -> Option<&mut Entity> {
        self.entity_map
            .get(&entity_id)
            .map(|&index| &mut self.entities[index])
    }

    /// Get all entities
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// Get all entities mutably
    pub fn entities_mut(&mut self) -> &mut [Entity] {
        &mut self.entities
    }

    /// Get entity count
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Set active camera
    pub fn set_active_camera(&mut self, entity_id: usize) {
        if self
            .get_entity(entity_id)
            .map_or(false, |e| e.has_component::<CameraComponent>())
        {
            self.active_camera = Some(entity_id);
        }
    }

    /// Get active camera entity ID
    pub fn active_camera(&self) -> Option<usize> {
        self.active_camera
    }

    /// Get active camera
    pub fn get_active_camera(&self) -> Option<&CameraComponent> {
        self.active_camera
            .and_then(|id| self.get_entity(id))
            .and_then(|e| e.camera())
    }

    /// Get camera position
    pub fn camera_position(&self) -> Vec3 {
        if let Some(camera_entity_id) = self.active_camera {
            if let Some(camera_entity) = self.get_entity(camera_entity_id) {
                if let Some(transform) = camera_entity.transform() {
                    return transform.position;
                }
            }
        }
        Vec3::ZERO
    }

    /// Get camera view matrix
    pub fn camera_view_matrix(&self) -> Mat4 {
        if let Some(camera_entity_id) = self.active_camera {
            if let Some(camera_entity) = self.get_entity(camera_entity_id) {
                if let Some(camera) = camera_entity.camera() {
                    return camera.camera.view_matrix();
                }
            }
        }
        Mat4::IDENTITY
    }

    /// Get camera projection matrix
    pub fn camera_projection_matrix(&self) -> Mat4 {
        if let Some(camera_entity_id) = self.active_camera {
            if let Some(camera_entity) = self.get_entity(camera_entity_id) {
                if let Some(camera) = camera_entity.camera() {
                    return camera.camera.projection_matrix();
                }
            }
        }
        Mat4::IDENTITY
    }

    /// Get camera view-projection matrix
    pub fn camera_view_projection_matrix(&self) -> Mat4 {
        let view = self.camera_view_matrix();
        let proj = self.camera_projection_matrix();
        proj * view
    }

    /// Update the scene
    pub fn update(&mut self, _delta_time: Duration, _context: &RenderContext) {
        // Update all entities
        for entity in &mut self.entities {
            if !entity.is_active() {
                continue;
            }

            // Update entity components
            // For now, this is a placeholder
            // In the actual implementation, this would update:
            // - Animations
            // - Particle systems
            // - Physics
            // - etc.
        }

        // Update scene bounding box
        self.update_bounding_box();
    }

    /// Update scene bounding box
    fn update_bounding_box(&mut self) {
        let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

        for entity in &self.entities {
            if let Some(aabb) = entity.bounding_box() {
                min = min.min(aabb.min);
                max = max.max(aabb.max);
            }
        }

        if min.x < f32::MAX {
            self.bounding_box = Some(Aabb::new(min, max));
        }
    }

    /// Perform frustum culling
    pub fn cull(&self, _context: &RenderContext) -> Vec<usize> {
        let view_proj = self.camera_view_projection_matrix();
        let mut visible_entities = Vec::new();

        for (entity_id, &index) in &self.entity_map {
            let entity = &self.entities[index];

            if !entity.is_active() || !entity.is_visible() {
                continue;
            }

            // Check if entity is in frustum
            if let Some(aabb) = entity.bounding_box() {
                let frustum = crate::render::scene::culling::Frustum::from_matrix(view_proj);
                if frustum.is_visible(&aabb) {
                    visible_entities.push(*entity_id);
                }
            } else {
                // If no bounding box, assume visible
                visible_entities.push(*entity_id);
            }
        }

        visible_entities
    }

    /// Get all lights in the scene
    pub fn lights(&self) -> Vec<&LightComponent> {
        self.entities.iter().filter_map(|e| e.light()).collect()
    }

    /// Get all renderable entities
    pub fn renderable_entities(&self) -> Vec<usize> {
        self.entities
            .iter()
            .filter(|e| e.is_active() && e.is_visible() && e.has_component::<Renderable>())
            .map(|e| e.id())
            .collect()
    }

    /// Get all opaque entities
    pub fn opaque_entities(&self) -> Vec<usize> {
        self.entities
            .iter()
            .filter(|e| e.is_active() && e.is_visible() && e.is_opaque())
            .map(|e| e.id())
            .collect()
    }

    /// Get all transparent entities
    pub fn transparent_entities(&self) -> Vec<usize> {
        self.entities
            .iter()
            .filter(|e| e.is_active() && e.is_visible() && e.is_transparent())
            .map(|e| e.id())
            .collect()
    }

    /// Get all shadow-casting entities
    pub fn shadow_casting_entities(&self) -> Vec<usize> {
        self.entities
            .iter()
            .filter(|e| e.is_active() && e.is_visible() && e.casts_shadows())
            .map(|e| e.id())
            .collect()
    }

    /// Clear the scene
    pub fn clear(&mut self) {
        self.entities.clear();
        self.entity_map.clear();
        self.next_entity_id = 1;
        self.active_camera = None;
        self.bounding_box = None;
    }

    /// Get scene bounding box
    pub fn bounding_box(&self) -> Option<&Aabb> {
        self.bounding_box.as_ref()
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new("default")
    }
}

/// Scene builder
pub struct SceneBuilder {
    scene: Scene,
}

impl SceneBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            scene: Scene::new(name),
        }
    }

    pub fn with_entity(mut self, entity: Entity) -> Self {
        self.scene.add_entity(entity);
        self
    }

    pub fn with_camera(mut self, camera_entity: Entity) -> Self {
        let entity_id = self.scene.add_entity(camera_entity);
        self.scene.set_active_camera(entity_id);
        self
    }

    pub fn build(self) -> Scene {
        self.scene
    }
}
