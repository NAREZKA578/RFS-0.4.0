//! Water Material
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Specialized material for rendering water with:
//! - Wave animation
//! - Reflections
//! - Refractions
//! - Foam
//! - Depth-based color changes

use super::material::{BlendMode, CullMode, Material, MaterialType};
use crate::render::textures::Texture;
use std::sync::Arc;

/// Water material
#[derive(Debug, Clone)]
pub struct WaterMaterial {
    /// Base material
    pub base: Material,
    /// Water color (RGB) and opacity (A)
    pub color: [f32; 4],
    /// Deep water color
    pub deep_color: [f32; 4],
    /// Shallow water color
    pub shallow_color: [f32; 4],
    /// Water depth (for color blending)
    pub depth: f32,
    /// Water clarity (0.0 = opaque, 1.0 = clear)
    pub clarity: f32,
    /// Wave speed
    pub wave_speed: f32,
    /// Wave scale
    pub wave_scale: f32,
    /// Wave height
    pub wave_height: f32,
    /// Normal map for waves
    pub normal_map: Option<Arc<Texture>>,
    /// Foam texture
    pub foam_texture: Option<Arc<Texture>>,
    /// Reflection texture
    pub reflection_texture: Option<Arc<Texture>>,
    /// Refraction texture
    pub refraction_texture: Option<Arc<Texture>>,
    /// Foam threshold (depth at which foam appears)
    pub foam_threshold: f32,
    /// Foam intensity
    pub foam_intensity: f32,
    /// Fresnel factor (controls edge reflection intensity)
    pub fresnel_factor: f32,
    /// Fresnel bias
    pub fresnel_bias: f32,
    /// Fresnel power
    pub fresnel_power: f32,
}

impl WaterMaterial {
    /// Create a new water material
    pub fn new(name: &str) -> Self {
        let mut base = Material::new(name, MaterialType::Water);
        base.blend_mode = BlendMode::Alpha;
        base.cull_mode = CullMode::Back;
        base.depth_test = true;
        base.depth_write = false; // Water doesn't write to depth buffer

        Self {
            base,
            color: [0.0, 0.1, 0.3, 0.7],
            deep_color: [0.0, 0.05, 0.2, 0.7],
            shallow_color: [0.0, 0.2, 0.4, 0.7],
            depth: 100.0,
            clarity: 0.7,
            wave_speed: 0.5,
            wave_scale: 0.1,
            wave_height: 0.2,
            normal_map: None,
            foam_texture: None,
            reflection_texture: None,
            refraction_texture: None,
            foam_threshold: 1.0,
            foam_intensity: 0.5,
            fresnel_factor: 0.02,
            fresnel_bias: 0.1,
            fresnel_power: 5.0,
        }
    }

    /// Set water color
    pub fn set_color(&mut self, color: [f32; 4]) {
        self.color = color;
        self.base.set_color("water_color", color);
    }

    /// Set deep water color
    pub fn set_deep_color(&mut self, color: [f32; 4]) {
        self.deep_color = color;
        self.base.set_color("deep_color", color);
    }

    /// Set shallow water color
    pub fn set_shallow_color(&mut self, color: [f32; 4]) {
        self.shallow_color = color;
        self.base.set_color("shallow_color", color);
    }

    /// Set depth
    pub fn set_depth(&mut self, depth: f32) {
        self.depth = depth;
        self.base.set_float("water_depth", depth);
    }

    /// Set clarity
    pub fn set_clarity(&mut self, clarity: f32) {
        self.clarity = clarity.clamp(0.0, 1.0);
        self.base.set_float("water_clarity", clarity);
    }

    /// Set wave speed
    pub fn set_wave_speed(&mut self, speed: f32) {
        self.wave_speed = speed;
        self.base.set_float("wave_speed", speed);
    }

    /// Set wave scale
    pub fn set_wave_scale(&mut self, scale: f32) {
        self.wave_scale = scale;
        self.base.set_float("wave_scale", scale);
    }

    /// Set wave height
    pub fn set_wave_height(&mut self, height: f32) {
        self.wave_height = height;
        self.base.set_float("wave_height", height);
    }

    /// Set normal map
    pub fn set_normal_map(&mut self, texture: Arc<Texture>) {
        self.normal_map = Some(texture.clone());
        self.base.set_texture("normal_map", texture);
    }

    /// Set foam texture
    pub fn set_foam_texture(&mut self, texture: Arc<Texture>) {
        self.foam_texture = Some(texture.clone());
        self.base.set_texture("foam_texture", texture);
    }

    /// Set reflection texture
    pub fn set_reflection_texture(&mut self, texture: Arc<Texture>) {
        self.reflection_texture = Some(texture.clone());
        self.base.set_texture("reflection_texture", texture);
    }

    /// Set refraction texture
    pub fn set_refraction_texture(&mut self, texture: Arc<Texture>) {
        self.refraction_texture = Some(texture.clone());
        self.base.set_texture("refraction_texture", texture);
    }

    /// Set foam threshold
    pub fn set_foam_threshold(&mut self, threshold: f32) {
        self.foam_threshold = threshold;
        self.base.set_float("foam_threshold", threshold);
    }

    /// Set foam intensity
    pub fn set_foam_intensity(&mut self, intensity: f32) {
        self.foam_intensity = intensity.clamp(0.0, 1.0);
        self.base.set_float("foam_intensity", intensity);
    }

    /// Set fresnel parameters
    pub fn set_fresnel(&mut self, factor: f32, bias: f32, power: f32) {
        self.fresnel_factor = factor;
        self.fresnel_bias = bias;
        self.fresnel_power = power;
        self.base
            .set_vector("fresnel_params", [factor, bias, power, 0.0]);
    }

    /// Create pipeline
    pub fn create_pipeline(&mut self, device: &crate::rhi::Device) {
        self.base.create_pipeline(device);
    }

    /// Bind the material
    pub fn bind(&self, encoder: &mut crate::rhi::CommandEncoder) {
        self.base.bind(encoder);
    }

    /// Update water parameters (called every frame for animation)
    pub fn update(&mut self, delta_time: std::time::Duration) {
        // Update wave parameters based on time
        let _time = delta_time.as_secs_f32();

        // In the actual implementation, we would update wave offsets
        // based on time and wave speed
    }
}

impl Default for WaterMaterial {
    fn default() -> Self {
        Self::new("water_default")
    }
}

/// Water Material builder
pub struct WaterMaterialBuilder {
    material: WaterMaterial,
}

impl WaterMaterialBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            material: WaterMaterial::new(name),
        }
    }

    pub fn color(mut self, color: [f32; 4]) -> Self {
        self.material.set_color(color);
        self
    }

    pub fn deep_color(mut self, color: [f32; 4]) -> Self {
        self.material.set_deep_color(color);
        self
    }

    pub fn shallow_color(mut self, color: [f32; 4]) -> Self {
        self.material.set_shallow_color(color);
        self
    }

    pub fn depth(mut self, depth: f32) -> Self {
        self.material.set_depth(depth);
        self
    }

    pub fn clarity(mut self, clarity: f32) -> Self {
        self.material.set_clarity(clarity);
        self
    }

    pub fn wave_speed(mut self, speed: f32) -> Self {
        self.material.set_wave_speed(speed);
        self
    }

    pub fn wave_scale(mut self, scale: f32) -> Self {
        self.material.set_wave_scale(scale);
        self
    }

    pub fn wave_height(mut self, height: f32) -> Self {
        self.material.set_wave_height(height);
        self
    }

    pub fn normal_map(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_normal_map(texture);
        self
    }

    pub fn foam_texture(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_foam_texture(texture);
        self
    }

    pub fn reflection_texture(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_reflection_texture(texture);
        self
    }

    pub fn refraction_texture(mut self, texture: Arc<Texture>) -> Self {
        self.material.set_refraction_texture(texture);
        self
    }

    pub fn foam_threshold(mut self, threshold: f32) -> Self {
        self.material.set_foam_threshold(threshold);
        self
    }

    pub fn foam_intensity(mut self, intensity: f32) -> Self {
        self.material.set_foam_intensity(intensity);
        self
    }

    pub fn fresnel(mut self, factor: f32, bias: f32, power: f32) -> Self {
        self.material.set_fresnel(factor, bias, power);
        self
    }

    pub fn build(self) -> WaterMaterial {
        self.material
    }
}

/// Common water material presets
impl WaterMaterial {
    /// Create ocean water material
    pub fn ocean(name: &str) -> Self {
        let mut material = Self::new(name);
        material.set_color([0.0, 0.05, 0.2, 0.8]);
        material.set_deep_color([0.0, 0.02, 0.15, 0.8]);
        material.set_shallow_color([0.0, 0.15, 0.35, 0.7]);
        material.set_depth(1000.0);
        material.set_clarity(0.8);
        material.set_wave_speed(0.3);
        material.set_wave_scale(0.05);
        material.set_wave_height(0.5);
        material.set_fresnel(0.02, 0.1, 5.0);
        material
    }

    /// Create lake water material
    pub fn lake(name: &str) -> Self {
        let mut material = Self::new(name);
        material.set_color([0.0, 0.2, 0.3, 0.7]);
        material.set_deep_color([0.0, 0.1, 0.2, 0.7]);
        material.set_shallow_color([0.1, 0.3, 0.4, 0.6]);
        material.set_depth(50.0);
        material.set_clarity(0.9);
        material.set_wave_speed(0.2);
        material.set_wave_scale(0.1);
        material.set_wave_height(0.1);
        material
    }

    /// Create river water material
    pub fn river(name: &str) -> Self {
        let mut material = Self::new(name);
        material.set_color([0.1, 0.3, 0.4, 0.6]);
        material.set_deep_color([0.1, 0.25, 0.35, 0.6]);
        material.set_shallow_color([0.2, 0.4, 0.5, 0.5]);
        material.set_depth(10.0);
        material.set_clarity(0.95);
        material.set_wave_speed(0.5);
        material.set_wave_scale(0.2);
        material.set_wave_height(0.05);
        material
    }
}
