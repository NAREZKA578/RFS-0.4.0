//! Water Surface
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::meshes::Mesh;
use crate::render::textures::Texture;
use glam::{Mat4, Vec2, Vec3};
use std::sync::Arc;

/// Water surface
pub struct WaterSurface {
    /// Mesh for rendering
    pub mesh: Option<Arc<Mesh>>,
    /// Normal map
    pub normal_map: Option<Arc<Texture>>,
    /// Foam texture
    pub foam_texture: Option<Arc<Texture>>,
    /// Reflection texture
    pub reflection_texture: Option<Arc<Texture>>,
    /// Refraction texture
    pub refraction_texture: Option<Arc<Texture>>,
    /// Tessellation factor
    pub tessellation_factor: f32,
    /// Mesh size (in world units)
    pub size: Vec3,
    /// Mesh resolution (number of vertices)
    pub resolution: Vec2,
}

impl WaterSurface {
    pub fn new() -> Self {
        Self {
            mesh: None,
            normal_map: None,
            foam_texture: None,
            reflection_texture: None,
            refraction_texture: None,
            tessellation_factor: 8.0,
            size: Vec3::new(1000.0, 1.0, 1000.0),
            resolution: Vec2::new(128.0, 128.0),
        }
    }

    pub fn create_mesh(&mut self) {
        // Create a tessellated mesh for water
        // This would create a mesh that can be tessellated based on the tessellation factor

        // For now, create a simple plane
        self.mesh = Some(Arc::new(Mesh::plane(
            "water_surface",
            self.size.x,
            self.size.z,
        )));
    }

    pub fn set_normal_map(&mut self, texture: Arc<Texture>) {
        self.normal_map = Some(texture);
    }

    pub fn set_foam_texture(&mut self, texture: Arc<Texture>) {
        self.foam_texture = Some(texture);
    }

    pub fn set_reflection_texture(&mut self, texture: Arc<Texture>) {
        self.reflection_texture = Some(texture);
    }

    pub fn set_refraction_texture(&mut self, texture: Arc<Texture>) {
        self.refraction_texture = Some(texture);
    }

    pub fn set_size(&mut self, size: Vec3) {
        self.size = size;
        self.create_mesh();
    }

    pub fn set_resolution(&mut self, resolution: Vec2) {
        self.resolution = resolution;
        self.create_mesh();
    }

    pub fn set_tessellation_factor(&mut self, factor: f32) {
        self.tessellation_factor = factor;
    }

    /// Get the model matrix for the water surface
    pub fn model_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::new(self.size.x, 1.0, self.size.z),
            glam::Quat::IDENTITY,
            Vec3::new(0.0, self.size.y / 2.0, 0.0),
        )
    }

    /// Render the water surface
    pub fn render(&self) {
        // This would render the water mesh with the appropriate shaders
        // and textures
    }
}

impl Default for WaterSurface {
    fn default() -> Self {
        Self::new()
    }
}
