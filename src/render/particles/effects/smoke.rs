//! Smoke Particle Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::core::settings::GraphicsSettings;
use crate::render::particles::{
    EmitterShape, ParticleEffect, ParticleEffectType, ParticleEmitterConfig,
};

/// Smoke effect configuration
#[derive(Debug, Clone)]
pub struct SmokeEffectConfig {
    pub base: ParticleEmitterConfig,
    pub color_start: [f32; 4],
    pub color_end: [f32; 4],
    pub size_start: f32,
    pub size_end: f32,
    pub lifetime: f32,
    pub rise_speed: f32,
    pub spread: f32,
    pub density: f32,
}

impl Default for SmokeEffectConfig {
    fn default() -> Self {
        Self {
            base: ParticleEmitterConfig::default(),
            color_start: [0.3, 0.3, 0.3, 0.8],
            color_end: [0.1, 0.1, 0.1, 0.0],
            size_start: 0.5,
            size_end: 1.5,
            lifetime: 3.0,
            rise_speed: 0.5,
            spread: 0.3,
            density: 0.5,
        }
    }
}

/// Smoke particle effect
pub struct SmokeEffect {
    config: SmokeEffectConfig,
    emitter: crate::render::particles::ParticleEmitter,
    enabled: bool,
    quality: SmokeQuality,
}

/// Smoke quality levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmokeQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl SmokeEffect {
    /// Creates a new smoke effect
    pub fn new(config: SmokeEffectConfig) -> Self {
        let mut emitter_config = config.base.clone();
        emitter_config.emitter_shape = EmitterShape::Sphere;
        emitter_config.max_particles = match config.density {
            d if d < 0.3 => 20,
            d if d < 0.6 => 50,
            d if d < 0.8 => 100,
            _ => 200,
        };
        emitter_config.emission_rate = 20.0;
        emitter_config.particle_lifetime = config.lifetime;
        emitter_config.particle_size = glam::Vec2::new(config.size_start, config.size_end);
        emitter_config.particle_color = glam::Vec4::from_array(config.color_start);
        emitter_config.particle_velocity = glam::Vec3::new(0.0, config.rise_speed, 0.0);
        emitter_config.particle_velocity_variation = glam::Vec3::splat(config.spread);

        Self {
            config,
            emitter: crate::render::particles::ParticleEmitter::new(emitter_config),
            enabled: true,
            quality: SmokeQuality::Medium,
        }
    }

    /// Updates the smoke effect
    pub fn update(&mut self, dt: f32, position: [f32; 3], wind_direction: [f32; 3]) {
        if !self.enabled {
            return;
        }

        self.emitter.set_position(glam::Vec3::from_array(position));
        self.emitter.config_mut().particle_velocity = glam::Vec3::new(
            wind_direction[0] * 0.3,
            wind_direction[1] * 0.3 + self.config.rise_speed,
            wind_direction[2] * 0.3,
        );
        self.emitter.update(std::time::Duration::from_secs_f32(dt));
    }

    /// Renders the smoke effect
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
    pub fn set_quality(&mut self, quality: SmokeQuality, settings: &GraphicsSettings) {
        self.quality = quality;
        let particle_count = match (quality, settings.particle_quality) {
            (SmokeQuality::Low, _) => 20,
            (SmokeQuality::Medium, 0) => 50,
            (SmokeQuality::Medium, 1) => 75,
            (SmokeQuality::High, 0) => 100,
            (SmokeQuality::High, 1) => 150,
            (SmokeQuality::Ultra, 0) => 200,
            (SmokeQuality::Ultra, _) => 300,
            _ => 100,
        };
        self.emitter.config_mut().max_particles = particle_count;
    }

    /// Returns the effect type
    pub fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Smoke
    }

    /// Returns mutable reference to the emitter
    pub fn emitter_mut(&mut self) -> &mut crate::render::particles::ParticleEmitter {
        &mut self.emitter
    }

    /// Returns reference to the config
    pub fn config(&self) -> &SmokeEffectConfig {
        &self.config
    }

    /// Returns mutable reference to the config
    pub fn config_mut(&mut self) -> &mut SmokeEffectConfig {
        &mut self.config
    }
}

impl ParticleEffect for SmokeEffect {
    fn update(&mut self, dt: f32) {
        // Default update - position should be set externally
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
        ParticleEffectType::Smoke
    }
}
