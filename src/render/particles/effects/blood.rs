//! Blood Particle Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::core::settings::GraphicsSettings;
use crate::render::particles::{
    EmitterShape, ParticleEffect, ParticleEffectType, ParticleEmitterConfig,
};

/// Blood effect configuration
#[derive(Debug, Clone)]
pub struct BloodEffectConfig {
    pub base: ParticleEmitterConfig,
    pub color_start: [f32; 4],
    pub color_end: [f32; 4],
    pub size_start: f32,
    pub size_end: f32,
    pub lifetime: f32,
    pub spread: f32,
    pub gravity: f32,
    pub count: u32,
}

impl Default for BloodEffectConfig {
    fn default() -> Self {
        Self {
            base: ParticleEmitterConfig::default(),
            color_start: [0.5, 0.0, 0.0, 1.0],
            color_end: [0.3, 0.0, 0.0, 0.0],
            size_start: 0.05,
            size_end: 0.0,
            lifetime: 0.8,
            spread: 1.5,
            gravity: -5.0,
            count: 20,
        }
    }
}

/// Blood particle effect
pub struct BloodEffect {
    config: BloodEffectConfig,
    emitter: crate::render::particles::ParticleEmitter,
    enabled: bool,
    quality: BloodQuality,
}

/// Blood quality levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BloodQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl BloodEffect {
    /// Creates a new blood effect
    pub fn new(config: BloodEffectConfig) -> Self {
        let mut emitter_config = config.base.clone();
        emitter_config.emitter_shape = EmitterShape::Cone;
        emitter_config.max_particles = config.count;
        emitter_config.emission_rate = config.count as f32 * 3.0;
        emitter_config.particle_lifetime = config.lifetime;
        emitter_config.particle_size = glam::Vec2::new(config.size_start, config.size_end);
        emitter_config.particle_color = glam::Vec4::from_array(config.color_start);
        emitter_config.particle_velocity_variation = glam::Vec3::splat(config.spread);
        emitter_config.gravity = glam::Vec3::new(0.0, config.gravity, 0.0);
        emitter_config.particle_velocity = glam::Vec3::new(0.0, -1.0, 0.0);

        Self {
            config,
            emitter: crate::render::particles::ParticleEmitter::new(emitter_config),
            enabled: true,
            quality: BloodQuality::Medium,
        }
    }

    /// Triggers blood at the given position and direction
    pub fn trigger(&mut self, position: [f32; 3], direction: [f32; 3]) {
        if !self.enabled {
            return;
        }

        self.emitter.set_position(glam::Vec3::from_array(position));
        self.emitter.config_mut().particle_velocity = glam::Vec3::from_array(direction);
        self.emitter.burst(self.config.count);
    }

    /// Updates the blood effect
    pub fn update(&mut self, dt: f32) {
        if !self.enabled {
            return;
        }
        self.emitter.update(std::time::Duration::from_secs_f32(dt));
    }

    /// Renders the blood effect
    pub fn render(&self, _renderer: &mut crate::render::core::Renderer) {
        if !self.enabled {
        }
    }

    /// Enables or disables the effect
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Sets the quality level
    pub fn set_quality(&mut self, quality: BloodQuality, settings: &GraphicsSettings) {
        self.quality = quality;
        let particle_count = match (quality, settings.particle_quality) {
            (BloodQuality::Low, _) => 10,
            (BloodQuality::Medium, 0) => 20,
            (BloodQuality::Medium, 1) => 30,
            (BloodQuality::High, 0) => 40,
            (BloodQuality::High, 1) => 60,
            (BloodQuality::Ultra, 0) => 60,
            (BloodQuality::Ultra, _) => 80,
            _ => 20,
        };
        self.emitter.config_mut().max_particles = particle_count;
        self.config.count = particle_count;
    }

    /// Returns the effect type
    pub fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Blood
    }

    /// Returns mutable reference to the emitter
    pub fn emitter_mut(&mut self) -> &mut crate::render::particles::ParticleEmitter {
        &mut self.emitter
    }

    /// Returns reference to the config
    pub fn config(&self) -> &BloodEffectConfig {
        &self.config
    }

    /// Returns mutable reference to the config
    pub fn config_mut(&mut self) -> &mut BloodEffectConfig {
        &mut self.config
    }
}

impl ParticleEffect for BloodEffect {
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
        ParticleEffectType::Blood
    }
}
