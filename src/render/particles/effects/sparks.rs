//! Sparks Particle Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::core::settings::GraphicsSettings;
use crate::render::particles::{
    EmitterShape, ParticleEffect, ParticleEffectType, ParticleEmitterConfig,
};

/// Sparks effect configuration
#[derive(Debug, Clone)]
pub struct SparksEffectConfig {
    pub base: ParticleEmitterConfig,
    pub color_start: [f32; 4],
    pub color_end: [f32; 4],
    pub size_start: f32,
    pub size_end: f32,
    pub lifetime: f32,
    pub spread: f32,
    pub speed: f32,
    pub gravity: f32,
    pub count: u32,
}

impl Default for SparksEffectConfig {
    fn default() -> Self {
        Self {
            base: ParticleEmitterConfig::default(),
            color_start: [1.0, 0.9, 0.5, 1.0],
            color_end: [1.0, 0.3, 0.0, 0.0],
            size_start: 0.05,
            size_end: 0.0,
            lifetime: 0.5,
            spread: 3.0,
            speed: 10.0,
            gravity: -2.0,
            count: 30,
        }
    }
}

/// Sparks particle effect
pub struct SparksEffect {
    config: SparksEffectConfig,
    emitter: crate::render::particles::ParticleEmitter,
    enabled: bool,
    quality: SparksQuality,
}

/// Sparks quality levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SparksQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl SparksEffect {
    /// Creates a new sparks effect
    pub fn new(config: SparksEffectConfig) -> Self {
        let mut emitter_config = config.base.clone();
        emitter_config.emitter_shape = EmitterShape::Sphere;
        emitter_config.max_particles = config.count;
        emitter_config.emission_rate = config.count as f32 * 5.0;
        emitter_config.particle_lifetime = config.lifetime;
        emitter_config.particle_size = glam::Vec2::new(config.size_start, config.size_end);
        emitter_config.particle_color = glam::Vec4::from_array(config.color_start);
        emitter_config.particle_velocity_variation = glam::Vec3::splat(config.spread);
        emitter_config.particle_velocity = glam::Vec3::new(0.0, config.speed, 0.0);
        emitter_config.gravity = glam::Vec3::new(0.0, config.gravity, 0.0);

        Self {
            config,
            emitter: crate::render::particles::ParticleEmitter::new(emitter_config),
            enabled: true,
            quality: SparksQuality::Medium,
        }
    }

    /// Triggers sparks at the given position and direction
    pub fn trigger(&mut self, position: [f32; 3], direction: [f32; 3]) {
        if !self.enabled {
            return;
        }

        self.emitter.set_position(glam::Vec3::from_array(position));
        self.emitter.config_mut().particle_velocity = glam::Vec3::from_array(direction);
        self.emitter.burst(self.config.count);
    }

    /// Updates the sparks effect
    pub fn update(&mut self, dt: f32) {
        if !self.enabled {
            return;
        }
        self.emitter.update(std::time::Duration::from_secs_f32(dt));
    }

    /// Renders the sparks effect
    pub fn render(&self, _renderer: &mut crate::render::core::Renderer) {
        if !self.enabled {
        }
    }

    /// Enables or disables the effect
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Sets the quality level
    pub fn set_quality(&mut self, quality: SparksQuality, settings: &GraphicsSettings) {
        self.quality = quality;
        let particle_count = match (quality, settings.particle_quality) {
            (SparksQuality::Low, _) => 15,
            (SparksQuality::Medium, 0) => 30,
            (SparksQuality::Medium, 1) => 45,
            (SparksQuality::High, 0) => 60,
            (SparksQuality::High, 1) => 90,
            (SparksQuality::Ultra, 0) => 90,
            (SparksQuality::Ultra, _) => 120,
            _ => 30,
        };
        self.emitter.config_mut().max_particles = particle_count;
        self.config.count = particle_count;
    }

    /// Returns the effect type
    pub fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Sparks
    }

    /// Returns mutable reference to the emitter
    pub fn emitter_mut(&mut self) -> &mut crate::render::particles::ParticleEmitter {
        &mut self.emitter
    }

    /// Returns reference to the config
    pub fn config(&self) -> &SparksEffectConfig {
        &self.config
    }

    /// Returns mutable reference to the config
    pub fn config_mut(&mut self) -> &mut SparksEffectConfig {
        &mut self.config
    }
}

impl ParticleEffect for SparksEffect {
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
        ParticleEffectType::Sparks
    }
}
