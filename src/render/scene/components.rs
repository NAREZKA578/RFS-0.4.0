//! Components
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::lighting::Light;
use crate::render::materials::{
    Material, PbrMaterial,
    WaterMaterial,
};
use crate::render::meshes::Mesh;
use glam::{Mat4, Quat, Vec3};
use std::sync::Arc;

/// Transform component
#[derive(Debug, Clone)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub fn new() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    pub fn with_position(position: Vec3) -> Self {
        Self {
            position,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    pub fn with_rotation(rotation: Quat) -> Self {
        Self {
            position: Vec3::ZERO,
            rotation,
            scale: Vec3::ONE,
        }
    }

    pub fn with_scale(scale: Vec3) -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale,
        }
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    pub fn position(&self) -> Vec3 {
        self.position
    }

    pub fn rotation(&self) -> Quat {
        self.rotation
    }

    pub fn scale(&self) -> Vec3 {
        self.scale
    }

    pub fn set_position(&mut self, position: Vec3) {
        self.position = position;
    }

    pub fn set_rotation(&mut self, rotation: Quat) {
        self.rotation = rotation;
    }

    pub fn set_scale(&mut self, scale: Vec3) {
        self.scale = scale;
    }

    pub fn translate(&mut self, translation: Vec3) {
        self.position += translation;
    }

    pub fn rotate(&mut self, rotation: Quat) {
        self.rotation = rotation * self.rotation;
    }

    pub fn scale_by(&mut self, factor: Vec3) {
        self.scale *= factor;
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::new()
    }
}

/// Renderable component
#[derive(Debug, Clone)]
pub struct Renderable {
    pub is_opaque: bool,
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    pub lod_level: usize,
    pub lod_distance: f32,
    pub visible: bool,
}

impl Renderable {
    pub fn new() -> Self {
        Self {
            is_opaque: true,
            casts_shadows: true,
            receives_shadows: true,
            lod_level: 0,
            lod_distance: 0.0,
            visible: true,
        }
    }

    pub fn opaque(mut self, is_opaque: bool) -> Self {
        self.is_opaque = is_opaque;
        self
    }

    pub fn casts_shadows(mut self, casts_shadows: bool) -> Self {
        self.casts_shadows = casts_shadows;
        self
    }

    pub fn receives_shadows(mut self, receives_shadows: bool) -> Self {
        self.receives_shadows = receives_shadows;
        self
    }

    pub fn lod_level(mut self, level: usize) -> Self {
        self.lod_level = level;
        self
    }
}

impl Default for Renderable {
    fn default() -> Self {
        Self::new()
    }
}

/// Mesh component
#[derive(Debug, Clone)]
pub struct MeshComponent {
    pub mesh: Arc<Mesh>,
    pub lods: Vec<Arc<Mesh>>,
}

impl MeshComponent {
    pub fn new(mesh: Arc<Mesh>) -> Self {
        Self {
            mesh,
            lods: Vec::new(),
        }
    }

    pub fn with_lods(mut self, lods: Vec<Arc<Mesh>>) -> Self {
        self.lods = lods;
        self
    }

    pub fn get_lod_mesh(&self, level: usize) -> Arc<Mesh> {
        if level < self.lods.len() {
            self.lods[level].clone()
        } else {
            self.mesh.clone()
        }
    }
}

/// Material component
#[derive(Debug, Clone)]
pub struct MaterialComponent {
    pub material: Material,
    pub pbr_material: Option<PbrMaterial>,
    pub water_material: Option<WaterMaterial>,
}

impl MaterialComponent {
    pub fn new(material: Material) -> Self {
        Self {
            material,
            pbr_material: None,
            water_material: None,
        }
    }

    pub fn with_pbr(mut self, pbr_material: PbrMaterial) -> Self {
        self.pbr_material = Some(pbr_material);
        self
    }

    pub fn with_water(mut self, water_material: WaterMaterial) -> Self {
        self.water_material = Some(water_material);
        self
    }

    pub fn get_material(&self) -> &Material {
        &self.material
    }

    pub fn get_pbr(&self) -> Option<&PbrMaterial> {
        self.pbr_material.as_ref()
    }

    pub fn get_water(&self) -> Option<&WaterMaterial> {
        self.water_material.as_ref()
    }
}

/// Light component
#[derive(Debug, Clone)]
pub struct LightComponent {
    pub light: Light,
}

impl LightComponent {
    pub fn new(light: Light) -> Self {
        Self { light }
    }

    pub fn get_light(&self) -> &Light {
        &self.light
    }

    pub fn get_light_mut(&mut self) -> &mut Light {
        &mut self.light
    }
}

/// Camera component
#[derive(Debug, Clone)]
pub struct CameraComponent {
    pub camera: Arc<dyn crate::render::camera::Camera>,
    pub is_active: bool,
}

impl CameraComponent {
    pub fn new(camera: Arc<dyn crate::render::camera::Camera>) -> Self {
        Self {
            camera,
            is_active: false,
        }
    }

    pub fn get_camera(&self) -> &Arc<dyn crate::render::camera::Camera> {
        &self.camera
    }

    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }
}

/// Blend mode (re-export from materials)
pub use super::super::materials::material::BlendMode;

/// Cull mode (re-export from materials)
pub use super::super::materials::material::CullMode;
