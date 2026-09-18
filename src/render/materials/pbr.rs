//! PBR Material
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Physically Based Rendering material with support for:
//! - Albedo (base color)
//! - Normal maps
//! - Roughness maps
//! - Metallic maps
//! - Ambient Occlusion maps
//! - Emissive maps

use super::material::{
    BlendMode, CullMode, Material, MaterialType,
};
use crate::render::textures::Texture;
use std::sync::Arc;

/// PBR material
#[derive(Debug, Clone)]
pub struct PbrMaterial {
    /// Base material
    pub base: Material,
    /// Albedo color (RGB) and opacity (A)
    pub albedo: [f32; 4],
    /// Roughness (0.0 = smooth, 1.0 = rough)
    pub roughness: f32,
    /// Metallic (0.0 = dielectric, 1.0 = metal)
    pub metallic: f32,
    /// Ambient Occlusion (0.0 = no occlusion, 1.0 = full occlusion)
    pub ao: f32,
    /// Emissive color and intensity
    pub emissive: [f32; 4],
    /// Albedo texture
    pub albedo_map: Option<Arc<Texture>>,
    /// Normal texture
    pub normal_map: Option<Arc<Texture>>,
    /// Roughness texture
    pub roughness_map: Option<Arc<Texture>>,
    /// Metallic texture
    pub metallic_map: Option<Arc<Texture>>,
    /// AO texture
    pub ao_map: Option<Arc<Texture>>,
    /// Emissive texture
    pub emissive_map: Option<Arc<Texture>>,
    /// Normal map intensity
    pub normal_intensity: f32,
    /// Use packed material texture (RGBA: Metallic, Roughness, AO, Emissive)
    pub use_packed_material: bool,
}

impl PbrMaterial {
    /// Create a new PBR material
    pub fn new(name: &str) -> Self {
        let mut base = Material::new(name, MaterialType::Pbr);
        base.blend_mode = BlendMode::Opaque;
        base.cull_mode = CullMode::Back;
        base.depth_test = true;
        base.depth_write = true;

        Self {
            base,
            albedo: [1.0, 1.0, 1.0, 1.0],
            roughness: 0.5,
            metallic: 0.0,
            ao: 1.0,
            emissive: [0.0, 0.0, 0.0, 1.0],
            albedo_map: None,
            normal_map: None,
            roughness_map: None,
            metallic_map: None,
            ao_map: None,
            emissive_map: None,
            normal_intensity: 1.0,
            use_packed_material: false,
        }
    }

    /// Set albedo color
    pub fn set_albedo(&mut self, albedo: [f32; 4]) {
        self.albedo = albedo;
        self.base.set_color("albedo", albedo);
    }

    /// Set roughness
    pub fn set_roughness(&mut self, roughness: f32) {
        self.roughness = roughness.clamp(0.0, 1.0);
        self.base.set_float("roughness", self.roughness);
    }

    /// Set metallic
    pub fn set_metallic(&mut self, metallic: f32) {
        self.metallic = metallic.clamp(0.0, 1.0);
        self.base.set_float("metallic", self.metallic);
    }

    /// Set AO
    pub fn set_ao(&mut self, ao: f32) {
        self.ao = ao.clamp(0.0, 1.0);
        self.base.set_float("ao", self.ao);
    }

    /// Set emissive
    pub fn set_emissive(&mut self, emissive: [f32; 4]) {
        self.emissive = emissive;
        self.base.set_color("emissive", emissive);
    }

    /// Set albedo texture
    pub fn set_albedo_map(&mut self, texture: Arc<Texture>) {
        self.albedo_map = Some(texture.clone());
        self.base.set_texture("albedo_map", texture);
    }

    /// Set normal texture
    pub fn set_normal_map(&mut self, texture: Arc<Texture>) {
        self.normal_map = Some(texture.clone());
        self.base.set_texture("normal_map", texture);
    }

    /// Set roughness texture
    pub fn set_roughness_map(&mut self, texture: Arc<Texture>) {
        self.roughness_map = Some(texture.clone());
        self.base.set_texture("roughness_map", texture);
    }

    /// Set metallic texture
    pub fn set_metallic_map(&mut self, texture: Arc<Texture>) {
        self.metallic_map = Some(texture.clone());
        self.base.set_texture("metallic_map", texture);
    }

    /// Set AO texture
    pub fn set_ao_map(&mut self, texture: Arc<Texture>) {
        self.ao_map = Some(texture.clone());
        self.base.set_texture("ao_map", texture);
    }

    /// Set emissive texture
    pub fn set_emissive_map(&mut self, texture: Arc<Texture>) {
        self.emissive_map = Some(texture.clone());
        self.base.set_texture("emissive_map", texture);
    }

    /// Set normal intensity
    pub fn set_normal_intensity(&mut self, intensity: f32) {
        self.normal_intensity = intensity;
        self.base.set_float("normal_intensity", intensity);
    }

    /// Enable packed material texture
    pub fn set_packed_material(&mut self, packed: bool) {
        self.use_packed_material = packed;
        self.base
            .set_float("use_packed_material", if packed { 1.0 } else { 0.0 });
    }

    /// Set blend mode
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) {
        self.base.blend_mode = blend_mode;
    }

    /// Set cull mode
    pub fn set_cull_mode(&mut self, cull_mode: CullMode) {
        self.base.cull_mode = cull_mode;
    }

    /// Create pipeline
    pub fn create_pipeline(&mut self, device: &crate::rhi::Device) {
        self.base.create_pipeline(device);
    }

    /// Bind the material
    pub fn bind(&self, encoder: &mut crate::rhi::CommandEncoder) {
        self.base.bind(encoder);
    }
}

impl Default for PbrMaterial {
    fn default() -> Self {
        Self::new("pbr_default")
    }
}

/// PBR Material builder
pub struct PbrMaterialBuilder {
    material: PbrMaterial,
}

impl PbrMaterialBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            material: PbrMaterial::new(name),
        }
    }

    pub fn albedo(mut self, albedo: [f32; 4]) -> Self {
        self.material.set_albedo(albedo);
        self
    }

    pub fn roughness(mut self, roughness: f32) -> Self {
        self.material.set_roughness(roughness);
        self
    }

    pub fn metallic(mut self, metallic: f32) -> Self {
        self.material.set_metallic(metallic);
        self
    }

    pub fn ao(mut self, ao: f32) -> Self {
        self.material.set_ao(ao);
        self
    }

    pub fn emissive(mut self, emissive: [f32; 4]) -> Self {
        self.material.set_emissive(emissive);
        self
    }

    pub fn albedo_map(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_albedo_map(texture);
        self
    }

    pub fn normal_map(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_normal_map(texture);
        self
    }

    pub fn roughness_map(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_roughness_map(texture);
        self
    }

    pub fn metallic_map(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_metallic_map(texture);
        self
    }

    pub fn ao_map(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_ao_map(texture);
        self
    }

    pub fn emissive_map(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_emissive_map(texture);
        self
    }

    pub fn normal_intensity(mut self, intensity: f32) -> Self {
        self.material.set_normal_intensity(intensity);
        self
    }

    pub fn packed_material(mut self, packed: bool) -> Self {
        self.material.set_packed_material(packed);
        self
    }

    pub fn blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.material.set_blend_mode(blend_mode);
        self
    }

    pub fn cull_mode(mut self, cull_mode: CullMode) -> Self {
        self.material.set_cull_mode(cull_mode);
        self
    }

    pub fn build(self) -> PbrMaterial {
        self.material
    }
}

/// Common PBR material presets
impl PbrMaterial {
    /// Create a metal material
    pub fn metal(name: &str, albedo: [f32; 4]) -> Self {
        let mut material = Self::new(name);
        material.set_albedo(albedo);
        material.set_metallic(1.0);
        material.set_roughness(0.1);
        material
    }

    /// Create a plastic material
    pub fn plastic(name: &str, albedo: [f32; 4]) -> Self {
        let mut material = Self::new(name);
        material.set_albedo(albedo);
        material.set_metallic(0.0);
        material.set_roughness(0.3);
        material
    }

    /// Create a wood material
    pub fn wood(name: &str, albedo: [f32; 4]) -> Self {
        let mut material = Self::new(name);
        material.set_albedo(albedo);
        material.set_metallic(0.0);
        material.set_roughness(0.7);
        material
    }

    /// Create a glass material
    pub fn glass(name: &str) -> Self {
        let mut material = Self::new(name);
        material.set_albedo([0.9, 0.9, 0.95, 0.5]);
        material.set_metallic(0.0);
        material.set_roughness(0.05);
        material.set_blend_mode(BlendMode::Alpha);
        material.set_cull_mode(CullMode::None);
        material
    }

    /// Create a water material
    pub fn water(name: &str) -> Self {
        let mut material = Self::new(name);
        material.set_albedo([0.0, 0.1, 0.3, 0.7]);
        material.set_metallic(0.0);
        material.set_roughness(0.1);
        material.set_blend_mode(BlendMode::Alpha);
        material
    }
}
