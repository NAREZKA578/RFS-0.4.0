//! Magic Particle Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::core::settings::GraphicsSettings;
use crate::render::particles::{
    EmitterShape, ParticleEffect, ParticleEffectType, ParticleEmitterConfig,
};

/// Magic effect configuration
#[derive(Debug, Clone)]
pub struct MagicEffectConfig {
    pub base: ParticleEmitterConfig,
    pub color_start: [f32; 4],
    pub color_end: [f32; 4],
    pub size_start: f32,
    pub size_end: f32,
    pub lifetime: f32,
    pub spread: f32,
    pub speed: f32,
    pub swirl: f32,
    pub magic_type: MagicType,
}

/// Magic types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicType {
    Fireball,
    Ice,
    Lightning,
    Holy,
    Dark,
    Arcane,
}

impl Default for MagicEffectConfig {
    fn default() -> Self {
        Self {
            base: ParticleEmitterConfig::default(),
            color_start: [0.5, 0.5, 1.0, 1.0],
            color_end: [0.2, 0.2, 0.5, 0.0],
            size_start: 0.1,
            size_end: 0.0,
            lifetime: 1.5,
            spread: 2.0,
            speed: 3.0,
            swirl: 1.0,
            magic_type: MagicType::Arcane,
        }
    }
}

/// Magic particle effect
pub struct MagicEffect {
    config: MagicEffectConfig,
    emitter: crate::render::particles::ParticleEmitter,
    enabled: bool,
    quality: MagicQuality,
}

/// Magic quality levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl MagicEffect {
    /// Creates a new magic effect
    pub fn new(config: MagicEffectConfig) -> Self {
        let colors = Self::get_magic_colors(config.magic_type);

        let mut emitter_config = config.base.clone();
        emitter_config.emitter_shape = EmitterShape::Sphere;
        emitter_config.max_particles = 100;
        emitter_config.emission_rate = 50.0;
        emitter_config.particle_lifetime = config.lifetime;
        emitter_config.particle_size = glam::Vec2::new(config.size_start, config.size_end);
        emitter_config.particle_color = glam::Vec4::from_array(colors.0);
        emitter_config.particle_velocity_variation = glam::Vec3::splat(config.spread);
        emitter_config.particle_velocity = glam::Vec3::new(0.0, config.speed, 0.0);

        Self {
            config,
            emitter: crate::render::particles::ParticleEmitter::new(emitter_config),
            enabled: true,
            quality: MagicQuality::Medium,
        }
    }

    /// Gets colors based on magic type
    fn get_magic_colors(magic_type: MagicType) -> ([f32; 4], [f32; 4]) {
        match magic_type {
            MagicType::Fireball => ([1.0, 0.5, 0.0, 1.0], [0.5, 0.2, 0.0, 0.0]),
            MagicType::Ice => ([0.0, 0.8, 1.0, 1.0], [0.0, 0.4, 0.8, 0.0]),
            MagicType::Lightning => ([1.0, 1.0, 0.0, 1.0], [0.8, 0.8, 0.0, 0.0]),
            MagicType::Holy => ([1.0, 0.9, 0.0, 1.0], [0.8, 0.7, 0.0, 0.0]),
            MagicType::Dark => ([0.3, 0.1, 0.5, 1.0], [0.1, 0.0, 0.2, 0.0]),
            MagicType::Arcane => ([0.5, 0.5, 1.0, 1.0], [0.2, 0.2, 0.5, 0.0]),
        }
    }

    /// Casts magic from source to target
    pub fn cast(&mut self, source: [f32; 3], target: [f32; 3]) {
        if !self.enabled {
            return;
        }

        self.emitter.set_position(glam::Vec3::from_array(source));

        // Calculate direction
        let direction = [
            target[0] - source[0],
            target[1] - source[1],
            target[2] - source[2],
        ];

        let length = (direction[0].powi(2) + direction[1].powi(2) + direction[2].powi(2)).sqrt();
        if length > 0.0 {
            let normalized = [
                direction[0] / length,
                direction[1] / length,
                direction[2] / length,
            ];
            self.emitter
                .config_mut()
                .particle_velocity = glam::Vec3::from_array(normalized);
        }

        self.emitter.burst(50);
    }

    /// Updates the magic effect
    pub fn update(&mut self, dt: f32) {
        if !self.enabled {
            return;
        }
        self.emitter.update(std::time::Duration::from_secs_f32(dt));
    }

    /// Renders the magic effect
    pub fn render(&self, _renderer: &mut crate::render::core::Renderer) {
        if !self.enabled {
            return;
        }
    }

    /// Enables or disables the effect
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Sets the magic type
    pub fn set_magic_type(&mut self, magic_type: MagicType) {
        self.config.magic_type = magic_type;
        let colors = Self::get_magic_colors(magic_type);
        self.emitter.config_mut().particle_color = glam::Vec4::from_array(colors.0);
    }

    /// Sets the quality level
    pub fn set_quality(&mut self, quality: MagicQuality, settings: &GraphicsSettings) {
        self.quality = quality;
        let particle_count = match (quality, settings.particle_quality) {
            (MagicQuality::Low, _) => 40,
            (MagicQuality::Medium, 0) => 80,
            (MagicQuality::Medium, 1) => 120,
            (MagicQuality::High, 0) => 120,
            (MagicQuality::High, 1) => 180,
            (MagicQuality::Ultra, 0) => 180,
            (MagicQuality::Ultra, _) => 250,
            _ => 80,
        };
        self.emitter.config_mut().max_particles = particle_count;
    }

    /// Returns the effect type
    pub fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Magic
    }

    /// Returns mutable reference to the emitter
    pub fn emitter_mut(&mut self) -> &mut crate::render::particles::ParticleEmitter {
        &mut self.emitter
    }

    /// Returns reference to the config
    pub fn config(&self) -> &MagicEffectConfig {
        &self.config
    }

    /// Returns mutable reference to the config
    pub fn config_mut(&mut self) -> &mut MagicEffectConfig {
        &mut self.config
    }
}

impl ParticleEffect for MagicEffect {
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
        ParticleEffectType::Magic
    }
}
