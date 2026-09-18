//! Entity
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::components::{
    CameraComponent, LightComponent, MaterialComponent, MeshComponent, Renderable, Transform,
};
use std::any::Any;
use std::collections::HashMap;

/// Entity ID type
pub type EntityId = usize;

/// Entity is a game object that can be rendered
pub struct Entity {
    /// Unique identifier
    id: EntityId,
    /// Entity name
    name: String,
    /// Active flag
    active: bool,
    /// Visible flag
    visible: bool,
    /// Components
    components: HashMap<String, Box<dyn Any + Send + Sync>>,
    /// Tags for categorization
    tags: Vec<String>,
    /// Static flag (doesn't move)
    is_static: bool,
}

impl Entity {
    /// Create a new entity
    pub fn new(name: &str) -> Self {
        Self {
            id: 0, // Will be set when added to scene
            name: name.to_string(),
            active: true,
            visible: true,
            components: HashMap::new(),
            tags: Vec::new(),
            is_static: false,
        }
    }

    /// Set entity ID (called by scene when adding)
    pub fn set_id(&mut self, id: EntityId) {
        self.id = id;
    }

    /// Get entity ID
    pub fn id(&self) -> EntityId {
        self.id
    }

    /// Get entity name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if entity is active
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Set active state
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Check if entity is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Set visible state
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Check if entity is static
    pub fn is_static(&self) -> bool {
        self.is_static
    }

    /// Set static flag
    pub fn set_static(&mut self, is_static: bool) {
        self.is_static = is_static;
    }

    /// Add a tag
    pub fn add_tag(&mut self, tag: &str) {
        if !self.tags.contains(&tag.to_string()) {
            self.tags.push(tag.to_string());
        }
    }

    /// Remove a tag
    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.retain(|t| t != tag);
    }

    /// Check if entity has a tag
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&tag.to_string())
    }

    /// Add a component
    pub fn add_component<T: Any + Send + Sync>(&mut self, component: T) {
        let type_name = std::any::type_name::<T>().to_string();
        self.components.insert(type_name, Box::new(component));
    }

    /// Get a component by type
    pub fn get_component<T: Any + Send + Sync>(&self) -> Option<&T> {
        let type_name = std::any::type_name::<T>();
        self.components
            .get(type_name)
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    /// Get a mutable component by type
    pub fn get_component_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut T> {
        let type_name = std::any::type_name::<T>();
        self.components
            .get_mut(type_name)
            .and_then(|boxed| boxed.downcast_mut::<T>())
    }

    /// Remove a component by type
    pub fn remove_component<T: Any + Send + Sync>(&mut self) -> Option<T> {
        let type_name = std::any::type_name::<T>();
        self.components
            .remove(type_name)
            .and_then(|boxed| boxed.downcast::<T>().ok().map(|boxed| *boxed))
    }

    /// Check if entity has a component of type T
    pub fn has_component<T: Any + Send + Sync>(&self) -> bool {
        let type_name = std::any::type_name::<T>();
        self.components.contains_key(type_name)
    }

    /// Get transform component (convenience method)
    pub fn transform(&self) -> Option<&Transform> {
        self.get_component::<Transform>()
    }

    /// Get mutable transform component (convenience method)
    pub fn transform_mut(&mut self) -> Option<&mut Transform> {
        self.get_component_mut::<Transform>()
    }

    /// Get renderable component (convenience method)
    pub fn renderable(&self) -> Option<&Renderable> {
        self.get_component::<Renderable>()
    }

    /// Get mesh component (convenience method)
    pub fn mesh(&self) -> Option<&MeshComponent> {
        self.get_component::<MeshComponent>()
    }

    /// Get material component (convenience method)
    pub fn material(&self) -> Option<&MaterialComponent> {
        self.get_component::<MaterialComponent>()
    }

    /// Get light component (convenience method)
    pub fn light(&self) -> Option<&LightComponent> {
        self.get_component::<LightComponent>()
    }

    /// Get camera component (convenience method)
    pub fn camera(&self) -> Option<&CameraComponent> {
        self.get_component::<CameraComponent>()
    }

    /// Check if entity is opaque
    pub fn is_opaque(&self) -> bool {
        if let Some(renderable) = self.renderable() {
            renderable.is_opaque
        } else {
            true // Default to opaque
        }
    }

    /// Check if entity is transparent
    pub fn is_transparent(&self) -> bool {
        !self.is_opaque()
    }

    /// Check if entity casts shadows
    pub fn casts_shadows(&self) -> bool {
        if let Some(renderable) = self.renderable() {
            renderable.casts_shadows
        } else {
            true // Default to casting shadows
        }
    }

    /// Get distance from camera
    pub fn distance_from_camera(&self, context: &crate::render::core::RenderContext) -> f32 {
        if let Some(transform) = self.transform() {
            let camera_pos = context.camera_position();
            let entity_pos = transform.position();
            camera_pos.distance(entity_pos)
        } else {
            f32::MAX
        }
    }

    /// Get bounding box in world space
    pub fn bounding_box(&self) -> Option<crate::render::scene::culling::Aabb> {
        if let Some(mesh) = self.mesh() {
            if let Some(transform) = self.transform() {
                let local_aabb = mesh.mesh.bounding_box();
                Some(local_aabb.transform(transform.matrix()))
            } else {
                Some(mesh.mesh.bounding_box())
            }
        } else {
            None
        }
    }

    /// Get blend mode
    pub fn blend_mode(&self) -> super::components::BlendMode {
        if let Some(material) = self.material() {
            material.material.blend_mode
        } else {
            super::components::BlendMode::Opaque
        }
    }
}

impl Default for Entity {
    fn default() -> Self {
        Self::new("default")
    }
}

/// Entity builder for easier entity creation
pub struct EntityBuilder {
    entity: Entity,
    _next_id: EntityId,
}

impl EntityBuilder {
    pub fn new() -> Self {
        Self {
            entity: Entity::new("default"),
            _next_id: 0,
        }
    }

    pub fn with_id(mut self, id: EntityId) -> Self {
        self.entity.set_id(id);
        self
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.entity.name = name.to_string();
        self
    }

    pub fn with_tag(mut self, tag: &str) -> Self {
        self.entity.add_tag(tag);
        self
    }

    pub fn with_component<T: Any + Send + Sync>(mut self, component: T) -> Self {
        self.entity.add_component(component);
        self
    }

    pub fn with_transform(mut self, transform: Transform) -> Self {
        self.entity.add_component(transform);
        self
    }

    pub fn with_mesh(mut self, mesh: MeshComponent) -> Self {
        self.entity.add_component(mesh);
        self
    }

    pub fn with_material(mut self, material: MaterialComponent) -> Self {
        self.entity.add_component(material);
        self
    }

    pub fn with_renderable(mut self, renderable: Renderable) -> Self {
        self.entity.add_component(renderable);
        self
    }

    pub fn with_light(mut self, light: LightComponent) -> Self {
        self.entity.add_component(light);
        self
    }

    pub fn with_camera(mut self, camera: CameraComponent) -> Self {
        self.entity.add_component(camera);
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.entity.set_active(active);
        self
    }

    pub fn visible(mut self, visible: bool) -> Self {
        self.entity.set_visible(visible);
        self
    }

    pub fn is_static(mut self, is_static: bool) -> Self {
        self.entity.set_static(is_static);
        self
    }

    pub fn build(self) -> Entity {
        self.entity
    }
}
