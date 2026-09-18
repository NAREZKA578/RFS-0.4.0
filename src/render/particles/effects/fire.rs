//! Fire Particle Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::core::settings::GraphicsSettings;
use crate::render::particles::{
    EmitterShape, ParticleEffect, ParticleEffectType, ParticleEmitterConfig,
};

/// Fire effect configuration
#[derive(Debug, Clone)]
pub struct FireEffectConfig {
    pub base: ParticleEmitterConfig,
    pub color_start: [f32; 4],
    pub color_mid: [f32; 4],
    pub color_end: [f32; 4],
    pub size_start: f32,
    pub size_end: f32,
    pub lifetime: f32,
    pub rise_speed: f32,
    pub flicker_intensity: f32,
    pub heat_distortion: f32,
}

impl Default for FireEffectConfig {
    fn default() -> Self {
        Self {
            base: ParticleEmitterConfig::default(),
            color_start: [1.0, 0.8, 0.0, 1.0],
            color_mid: [1.0, 0.5, 0.0, 0.8],
            color_end: [0.5, 0.1, 0.0, 0.0],
            size_start: 0.2,
            size_end: 0.8,
            lifetime: 1.5,
            rise_speed: 2.0,
            flicker_intensity: 0.5,
            heat_distortion: 0.3,
        }
    }
}

/// Fire particle effect
pub struct FireEffect {
    config: FireEffectConfig,
    emitter: crate::render::particles::ParticleEmitter,
    enabled: bool,
    quality: FireQuality,
}

/// Fire quality levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl FireEffect {
    /// Creates a new fire effect
    pub fn new(config: FireEffectConfig) -> Self {
        let mut emitter_config = config.base.clone();
        emitter_config.emitter_shape = EmitterShape::Cone;
        emitter_config.max_particles = 100;
        emitter_config.emission_rate = 50.0;
        emitter_config.particle_lifetime = config.lifetime;
        emitter_config.particle_size = glam::Vec2::new(config.size_start, config.size_end);
        emitter_config.particle_color = glam::Vec4::from_array(config.color_start);
        emitter_config.particle_velocity = glam::Vec3::new(0.0, 1.0, 0.0);
        emitter_config.particle_velocity_variation = glam::Vec3::splat(0.5);

        Self {
            config,
            emitter: crate::render::particles::ParticleEmitter::new(emitter_config),
            enabled: true,
            quality: FireQuality::Medium,
        }
    }

    /// Updates the fire effect
    pub fn update(&mut self, dt: f32, position: [f32; 3], wind_direction: [f32; 3]) {
        if !self.enabled {
            return;
        }

        self.emitter.set_position(glam::Vec3::from_array(position));

        // Add wind influence
        let wind_influence = [
            wind_direction[0] * 0.5,
            wind_direction[1] * 0.2 + self.config.rise_speed,
            wind_direction[2] * 0.5,
        ];
        self.emitter.config_mut().particle_velocity = glam::Vec3::from_array(wind_influence);

        self.emitter.update(std::time::Duration::from_secs_f32(dt));
    }

    /// Renders the fire effect
    pub fn render(&self, _renderer: &mut crate::render::core::Renderer) {
        if !self.enabled {
            return;
        }
    }

    /// Enables or disables the effect
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Sets the quality level
    pub fn set_quality(&mut self, quality: FireQuality, settings: &GraphicsSettings) {
        self.quality = quality;
        let particle_count = match (quality, settings.particle_quality) {
            (FireQuality::Low, _) => 50,
            (FireQuality::Medium, 0) => 100,
            (FireQuality::Medium, 1) => 150,
            (FireQuality::High, 0) => 200,
            (FireQuality::High, 1) => 300,
            (FireQuality::Ultra, 0) => 300,
            (FireQuality::Ultra, _) => 500,
            _ => 100,
        };
        self.emitter.config_mut().max_particles = particle_count;
    }

    /// Returns the effect type
    pub fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Fire
    }

    /// Returns mutable reference to the emitter
    pub fn emitter_mut(&mut self) -> &mut crate::render::particles::ParticleEmitter {
        &mut self.emitter
    }

    /// Returns reference to the config
    pub fn config(&self) -> &FireEffectConfig {
        &self.config
    }

    /// Returns mutable reference to the config
    pub fn config_mut(&mut self) -> &mut FireEffectConfig {
        &mut self.config
    }
}

impl ParticleEffect for FireEffect {
    fn update(&mut self, dt: f32) {
        self.emitter.update(std::time::Duration::from_secs_f32(dt));
    }

    fn render(&self, _renderer: &mut crate::render::core::Renderer) {
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Fire
    }
}
