//! Dust Particle Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::core::settings::GraphicsSettings;
use crate::render::particles::{
    EmitterShape, ParticleEffect, ParticleEffectType, ParticleEmitterConfig,
};

/// Dust effect configuration
#[derive(Debug, Clone)]
pub struct DustEffectConfig {
    pub base: ParticleEmitterConfig,
    pub color_start: [f32; 4],
    pub color_end: [f32; 4],
    pub size_start: f32,
    pub size_end: f32,
    pub lifetime: f32,
    pub spread: f32,
    pub height: f32,
    pub density: f32,
}

impl Default for DustEffectConfig {
    fn default() -> Self {
        Self {
            base: ParticleEmitterConfig::default(),
            color_start: [0.6, 0.5, 0.4, 0.8],
            color_end: [0.4, 0.3, 0.2, 0.0],
            size_start: 0.1,
            size_end: 0.5,
            lifetime: 2.0,
            spread: 1.0,
            height: 0.5,
            density: 0.3,
        }
    }
}

/// Dust particle effect
pub struct DustEffect {
    config: DustEffectConfig,
    emitter: crate::render::particles::ParticleEmitter,
    enabled: bool,
    quality: DustQuality,
    active: bool,
}

/// Dust quality levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DustQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl DustEffect {
    /// Creates a new dust effect
    pub fn new(config: DustEffectConfig) -> Self {
        let mut emitter_config = config.base.clone();
        emitter_config.emitter_shape = EmitterShape::Box;
        emitter_config.max_particles = 50;
        emitter_config.emission_rate = 30.0;
        emitter_config.particle_lifetime = config.lifetime;
        emitter_config.particle_size = glam::Vec2::new(config.size_start, config.size_end);
        emitter_config.particle_color = glam::Vec4::from_array(config.color_start);
        emitter_config.particle_velocity_variation = glam::Vec3::splat(config.spread);
        emitter_config.particle_velocity = glam::Vec3::new(0.0, config.height, 0.0);
        emitter_config.loop_emission = true;

        Self {
            config,
            emitter: crate::render::particles::ParticleEmitter::new(emitter_config),
            enabled: true,
            quality: DustQuality::Medium,
            active: false,
        }
    }

    /// Activates dust at the given position
    pub fn activate(&mut self, position: [f32; 3]) {
        if !self.enabled {
            return;
        }

        self.emitter.set_position(glam::Vec3::from_array(position));
        self.active = true;
    }

    /// Deactivates dust
    pub fn deactivate(&mut self) {
        self.active = false;
        self.emitter.reset();
    }

    /// Updates the dust effect
    pub fn update(&mut self, dt: f32, position: [f32; 3], velocity: [f32; 3]) {
        if !self.enabled || !self.active {
            return;
        }

        // Update position based on velocity
        let new_position = [
            position[0] + velocity[0] * dt,
            position[1] + velocity[1] * dt,
            position[2] + velocity[2] * dt,
        ];
        self.emitter.set_position(glam::Vec3::from_array(new_position));

        // Adjust emission rate based on speed
        let speed = (velocity[0].powi(2) + velocity[1].powi(2) + velocity[2].powi(2)).sqrt();
        let emission_rate = 30.0 + speed * 20.0;
        self.emitter
            .config_mut()
            .emission_rate = emission_rate.clamp(10.0, 100.0);

        self.emitter.update(std::time::Duration::from_secs_f32(dt));
    }

    /// Renders the dust effect
    pub fn render(&self, _renderer: &mut crate::render::core::Renderer) {
        if !self.enabled || !self.active {
        }
    }

    /// Enables or disables the effect
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.deactivate();
        }
    }

    /// Sets the quality level
    pub fn set_quality(&mut self, quality: DustQuality, settings: &GraphicsSettings) {
        self.quality = quality;
        let particle_count = match (quality, settings.particle_quality) {
            (DustQuality::Low, _) => 20,
            (DustQuality::Medium, 0) => 40,
            (DustQuality::Medium, 1) => 60,
            (DustQuality::High, 0) => 80,
            (DustQuality::High, 1) => 120,
            (DustQuality::Ultra, 0) => 120,
            (DustQuality::Ultra, _) => 160,
            _ => 40,
        };
        self.emitter.config_mut().max_particles = particle_count;
    }

    /// Returns the effect type
    pub fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Dust
    }

    /// Returns mutable reference to the emitter
    pub fn emitter_mut(&mut self) -> &mut crate::render::particles::ParticleEmitter {
        &mut self.emitter
    }

    /// Returns reference to the config
    pub fn config(&self) -> &DustEffectConfig {
        &self.config
    }

    /// Returns mutable reference to the config
    pub fn config_mut(&mut self) -> &mut DustEffectConfig {
        &mut self.config
    }
}

impl ParticleEffect for DustEffect {
    fn update(&mut self, _dt: f32) {
        // Dust requires position and velocity - use specialized update
    }

    fn render(&self, _renderer: &mut crate::render::core::Renderer) {
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.set_enabled(enabled);
    }

    fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Dust
    }
}
