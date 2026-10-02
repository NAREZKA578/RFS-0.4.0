//! Explosion Particle Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::core::settings::GraphicsSettings;
use crate::render::particles::{
    EmitterShape, ParticleEffect, ParticleEffectType, ParticleEmitterConfig,
};

/// Explosion effect configuration
#[derive(Debug, Clone)]
pub struct ExplosionEffectConfig {
    pub base: ParticleEmitterConfig,
    pub color_start: [f32; 4],
    pub color_end: [f32; 4],
    pub size_start: f32,
    pub size_end: f32,
    pub lifetime: f32,
    pub radius: f32,
    pub power: f32,
    pub debris_count: u32,
    pub smoke_enabled: bool,
}

impl Default for ExplosionEffectConfig {
    fn default() -> Self {
        Self {
            base: ParticleEmitterConfig::default(),
            color_start: [1.0, 0.5, 0.0, 1.0],
            color_end: [0.2, 0.1, 0.0, 0.0],
            size_start: 0.1,
            size_end: 2.0,
            lifetime: 2.0,
            radius: 5.0,
            power: 10.0,
            debris_count: 20,
            smoke_enabled: true,
        }
    }
}

/// Explosion particle effect
pub struct ExplosionEffect {
    config: ExplosionEffectConfig,
    fire_emitter: crate::render::particles::ParticleEmitter,
    debris_emitter: crate::render::particles::ParticleEmitter,
    smoke_emitter: Option<crate::render::particles::ParticleEmitter>,
    enabled: bool,
    quality: ExplosionQuality,
    timer: f32,
}

/// Explosion quality levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplosionQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl ExplosionEffect {
    /// Creates a new explosion effect
    pub fn new(config: ExplosionEffectConfig) -> Self {
        // Fire emitter
        let mut fire_config = config.base.clone();
        fire_config.emitter_shape = EmitterShape::Sphere;
        fire_config.max_particles = 100;
        fire_config.emission_rate = 200.0;
        fire_config.particle_lifetime = config.lifetime * 0.6;
        fire_config.particle_size = glam::Vec2::new(config.size_start, config.size_end * 0.8);
        fire_config.particle_color = glam::Vec4::from_array(config.color_start);
        fire_config.particle_velocity = glam::Vec3::new(0.0, 1.0, 0.0);
        fire_config.particle_velocity_variation = glam::Vec3::splat(config.power * 0.5);

        let fire_emitter = crate::render::particles::ParticleEmitter::new(fire_config);

        // Debris emitter
        let mut debris_config = config.base.clone();
        debris_config.emitter_shape = EmitterShape::Sphere;
        debris_config.max_particles = config.debris_count;
        debris_config.emission_rate = config.debris_count as f32 * 3.0;
        debris_config.particle_lifetime = config.lifetime * 0.8;
        debris_config.particle_size = glam::Vec2::new(config.size_start * 0.5, config.size_end * 0.3);
        debris_config.particle_color = glam::Vec4::from_array([0.3, 0.3, 0.3, 1.0]);
        debris_config.particle_velocity_variation = glam::Vec3::splat(config.power);
        debris_config.gravity = glam::Vec3::new(0.0, -5.0, 0.0);

        let debris_emitter = crate::render::particles::ParticleEmitter::new(debris_config);

        // Smoke emitter (optional)
        let smoke_emitter = if config.smoke_enabled {
            let mut smoke_config = config.base.clone();
            smoke_config.emitter_shape = EmitterShape::Sphere;
            smoke_config.max_particles = 50;
            smoke_config.emission_rate = 50.0;
            smoke_config.particle_lifetime = config.lifetime * 1.5;
            smoke_config.particle_size = glam::Vec2::new(config.size_start * 2.0, config.size_end * 3.0);
            smoke_config.particle_color = glam::Vec4::from_array([0.3, 0.3, 0.3, 0.6]);
            smoke_config.particle_velocity_variation = glam::Vec3::splat(config.power * 0.2);
            smoke_config.particle_velocity = glam::Vec3::new(0.0, 0.3, 0.0);

            Some(crate::render::particles::ParticleEmitter::new(smoke_config))
        } else {
            None
        };

        Self {
            config,
            fire_emitter,
            debris_emitter,
            smoke_emitter,
            enabled: true,
            quality: ExplosionQuality::Medium,
            timer: 0.0,
        }
    }

    /// Triggers an explosion at the given position
    pub fn trigger(&mut self, position: [f32; 3]) {
        if !self.enabled {
            return;
        }

        self.timer = 0.0;

        // Set positions for all emitters
        self.fire_emitter.set_position(glam::Vec3::from_array(position));
        self.debris_emitter.set_position(glam::Vec3::from_array(position));

        if let Some(smoke) = &mut self.smoke_emitter {
            smoke.set_position(glam::Vec3::from_array(position));
        }

        // Burst emitters
        self.fire_emitter.burst(self.fire_emitter.config().max_particles);
        self.debris_emitter
            .burst(self.debris_emitter.config().max_particles);

        if let Some(smoke) = &mut self.smoke_emitter {
            smoke.burst(smoke.config().max_particles);
        }
    }

    /// Updates the explosion effect
    pub fn update(&mut self, dt: f32) {
        if !self.enabled {
            return;
        }

        self.timer += dt;

        // Stop updating after lifetime
        if self.timer >= self.config.lifetime {
            return;
        }

        self.fire_emitter.update(std::time::Duration::from_secs_f32(dt));
        self.debris_emitter
            .update(std::time::Duration::from_secs_f32(dt));

        if let Some(smoke) = &mut self.smoke_emitter {
            smoke.update(std::time::Duration::from_secs_f32(dt));
        }
    }

    /// Renders the explosion effect
    pub fn render(&self, _renderer: &mut crate::render::core::Renderer) {
        if !self.enabled {
        }
    }

    /// Enables or disables the effect
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Sets the quality level
    pub fn set_quality(&mut self, quality: ExplosionQuality, settings: &GraphicsSettings) {
        self.quality = quality;

        let (fire_count, debris_count, smoke_count) = match (quality, settings.particle_quality) {
            (ExplosionQuality::Low, _) => (50, 10, 20),
            (ExplosionQuality::Medium, 0) => (100, 20, 30),
            (ExplosionQuality::Medium, 1) => (150, 30, 50),
            (ExplosionQuality::High, 0) => (200, 40, 70),
            (ExplosionQuality::High, 1) => (300, 60, 100),
            (ExplosionQuality::Ultra, 0) => (300, 80, 100),
            (ExplosionQuality::Ultra, _) => (500, 100, 150),
            _ => (100, 20, 30),
        };

        self.fire_emitter.config_mut().max_particles = fire_count;
        self.debris_emitter.config_mut().max_particles = debris_count;

        if let Some(smoke) = &mut self.smoke_emitter {
            smoke.config_mut().max_particles = smoke_count;
        }
    }

    /// Returns the effect type
    pub fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Explosion
    }

    /// Returns reference to the config
    pub fn config(&self) -> &ExplosionEffectConfig {
        &self.config
    }

    /// Returns mutable reference to the config
    pub fn config_mut(&mut self) -> &mut ExplosionEffectConfig {
        &mut self.config
    }
}

impl ParticleEffect for ExplosionEffect {
    fn update(&mut self, dt: f32) {
        self.update(dt);
    }

    fn render(&self, renderer: &mut crate::render::core::Renderer) {
        if self.enabled {
            self.render(renderer);
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn effect_type(&self) -> ParticleEffectType {
        ParticleEffectType::Explosion
    }
}
