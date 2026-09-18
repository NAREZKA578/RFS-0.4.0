//! Splash Particle Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::core::settings::GraphicsSettings;
use crate::render::particles::{
    EmitterShape, ParticleEffect, ParticleEffectType, ParticleEmitterConfig,
};

/// Splash effect configuration
#[derive(Debug, Clone)]
pub struct SplashEffectConfig {
    pub base: ParticleEmitterConfig,
    pub color: [f32; 4],
    pub size_start: f32,
    pub size_end: f32,
    pub lifetime: f32,
    pub gravity: f32,
    pub spread: f32,
    pub count: u32,
}

impl Default for SplashEffectConfig {
    fn default() -> Self {
        Self {
            base: ParticleEmitterConfig::default(),
            color: [0.5, 0.7, 1.0, 0.8],
            size_start: 0.1,
            size_end: 0.5,
            lifetime: 1.0,
            gravity: -9.8,
            spread: 2.0,
            count: 50,
        }
    }
}

/// Splash particle effect
pub struct SplashEffect {
    config: SplashEffectConfig,
    emitter: crate::render::particles::ParticleEmitter,
    enabled: bool,
    quality: SplashQuality,
}

/// Splash quality levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplashQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl SplashEffect {
    /// Creates a new splash effect
    pub fn new(config: SplashEffectConfig) -> Self {
        let mut emitter_config = config.base.clone();
        emitter_config.emitter_shape = EmitterShape::Sphere;
        emitter_config.max_particles = config.count;
        emitter_config.emission_rate = config.count as f32 * 2.0;
        emitter_config.particle_lifetime = config.lifetime;
        emitter_config.particle_size = glam::Vec2::new(config.size_start, config.size_end);
        emitter_config.particle_color = glam::Vec4::from_array(config.color);
        emitter_config.particle_velocity_variation = glam::Vec3::splat(config.spread);
        emitter_config.particle_velocity = glam::Vec3::new(0.0, 1.0, 0.0);
        emitter_config.gravity = glam::Vec3::new(0.0, config.gravity, 0.0);

        Self {
            config,
            emitter: crate::render::particles::ParticleEmitter::new(emitter_config),
            enabled: true,
            quality: SplashQuality::Medium,
        }
    }

    /// Triggers a splash at the given position
    pub fn trigger(&mut self, position: [f32; 3], normal: [f32; 3]) {
        if !self.enabled {
            return;
        }

        self.emitter.set_position(glam::Vec3::from_array(position));
        self.emitter.config_mut().particle_velocity = glam::Vec3::from_array(normal);
        self.emitter.burst(self.config.count);
    }

    /// Updates the splash effect
    pub fn update(&mut self, dt: f32) {
        if !self.enabled {
            return;
        }
        self.emitter.update(std::time::Duration::from_secs_f32(dt));
    }

    /// Renders the splash effect
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
    pub fn set_quality(&mut self, quality: SplashQuality, settings: &GraphicsSettings) {
        self.quality = quality;
        let particle_count = match (quality, settings.particle_quality) {
            (SplashQuality::Low, _) => 20,
            (SplashQuality::Medium, 0) => 50,
            (SplashQuality::Medium, 1) => 75,
            (SplashQuality::High, 0) => 100,
            (SplashQuality::High, 1) => 150,
            (SplashQuality::Ultra, 0) => 150,
            (SplashQuality::Ultra, _) => 200,
            _ => 50,
        };
        self.emitter.config_mut().max_particles = particle_count;
        self.config.count = particle_count;
    }

    /// Returns the effect type
    pub fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Splash
    }

    /// Returns mutable reference to the emitter
    pub fn emitter_mut(&mut self) -> &mut crate::render::particles::ParticleEmitter {
        &mut self.emitter
    }

    /// Returns reference to the config
    pub fn config(&self) -> &SplashEffectConfig {
        &self.config
    }

    /// Returns mutable reference to the config
    pub fn config_mut(&mut self) -> &mut SplashEffectConfig {
        &mut self.config
    }
}

impl ParticleEffect for SplashEffect {
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
        ParticleEffectType::Splash
    }
}
