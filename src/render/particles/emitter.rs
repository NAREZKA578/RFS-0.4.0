//! Particle Emitter
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::particle::Particle;
use glam::{Vec2, Vec3, Vec4};
use std::time::Duration;

/// Emitter shape
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum EmitterShape {
    #[default]
    Point,
    Sphere,
    Box,
    Cylinder,
    Cone,
    Line,
    Circle,
}


/// Particle emitter configuration
#[derive(Debug, Clone)]
pub struct ParticleEmitterConfig {
    pub name: String,
    pub max_particles: u32,
    pub emission_rate: f32,
    pub particle_lifetime: f32,
    pub particle_lifetime_variation: f32,
    pub particle_size: Vec2,
    pub particle_size_variation: Vec2,
    pub particle_color: Vec4,
    pub particle_color_variation: Vec4,
    pub particle_velocity: Vec3,
    pub particle_velocity_variation: Vec3,
    pub emitter_shape: EmitterShape,
    pub emitter_size: Vec3,
    pub gravity: Vec3,
    pub drag: f32,
    pub rotation_speed: f32,
    pub rotation_speed_variation: f32,
    pub loop_emission: bool,
    pub duration: f32,
    pub burst_count: u32,
    pub burst_interval: f32,
    pub texture_index: u32,
    pub blend_mode: crate::render::materials::material::BlendMode,
}

impl Default for ParticleEmitterConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            max_particles: 1000,
            emission_rate: 10.0,
            particle_lifetime: 1.0,
            particle_lifetime_variation: 0.2,
            particle_size: Vec2::new(0.1, 0.1),
            particle_size_variation: Vec2::ZERO,
            particle_color: Vec4::ONE,
            particle_color_variation: Vec4::ZERO,
            particle_velocity: Vec3::ZERO,
            particle_velocity_variation: Vec3::ZERO,
            emitter_shape: EmitterShape::Point,
            emitter_size: Vec3::ONE,
            gravity: Vec3::ZERO,
            drag: 0.0,
            rotation_speed: 0.0,
            rotation_speed_variation: 0.0,
            loop_emission: true,
            duration: f32::INFINITY,
            burst_count: 0,
            burst_interval: 0.0,
            texture_index: 0,
            blend_mode: crate::render::materials::material::BlendMode::Alpha,
        }
    }
}

/// Particle emitter
pub struct ParticleEmitter {
    config: ParticleEmitterConfig,
    particles: Vec<Particle>,
    active_particles: Vec<usize>,
    free_particles: Vec<usize>,
    position: Vec3,
    rotation: Vec3,
    scale: Vec3,
    time: f32,
    active: bool,
    emitting: bool,
    particles_emitted: u32,
    emission_accumulator: f32,
}

impl ParticleEmitter {
    pub fn new(config: ParticleEmitterConfig) -> Self {
        let mut particles = Vec::with_capacity(config.max_particles as usize);
        for i in 0..config.max_particles {
            particles.push(Particle {
                emitter_id: i,
                ..Default::default()
            });
        }

        let free_particles: Vec<usize> = (0..config.max_particles as usize).collect();

        Self {
            config,
            particles,
            active_particles: Vec::new(),
            free_particles,
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
            time: 0.0,
            active: true,
            emitting: true,
            particles_emitted: 0,
            emission_accumulator: 0.0,
        }
    }

    pub fn config(&self) -> &ParticleEmitterConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut ParticleEmitterConfig {
        &mut self.config
    }

    pub fn set_position(&mut self, position: Vec3) {
        self.position = position;
    }

    pub fn set_rotation(&mut self, rotation: Vec3) {
        self.rotation = rotation;
    }

    pub fn set_scale(&mut self, scale: Vec3) {
        self.scale = scale;
    }

    pub fn position(&self) -> Vec3 {
        self.position
    }

    pub fn rotation(&self) -> Vec3 {
        self.rotation
    }

    pub fn scale(&self) -> Vec3 {
        self.scale
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn is_emitting(&self) -> bool {
        self.emitting
    }

    pub fn set_emitting(&mut self, emitting: bool) {
        self.emitting = emitting;
    }

    pub fn particle_count(&self) -> u32 {
        self.active_particles.len() as u32
    }

    pub fn particles(&self) -> &[Particle] {
        &self.particles
    }

    /// Only live particles, in draw order (fixes GPU upload of dead pool entries).
    pub fn live_particles(&self) -> Vec<Particle> {
        self.active_particles
            .iter()
            .map(|&i| self.particles[i])
            .collect()
    }

    pub fn update(&mut self, delta_time: Duration) {
        if !self.active {
            return;
        }

        let delta_seconds = delta_time.as_secs_f32();
        self.time += delta_seconds;

        // Update existing particles
        for &index in &self.active_particles {
            let particle = &mut self.particles[index];
            particle.update(delta_time);

            // Apply gravity and drag
            particle.velocity += self.config.gravity * delta_seconds;
            // Bug №185: this was `velocity *= 1.0 - drag * dt`. Drag is
            // applied per *second*, so a hitch (or a low frame rate) easily
            // pushed `drag * dt` past 1, the factor went negative, and the
            // velocity flipped sign and grew without bound — particles
            // accelerated away instead of settling. Clamping the factor to
            // [0, 1] makes drag monotone: a frame can at most remove all
            // velocity, never reverse it.
            let drag_factor = (1.0 - self.config.drag * delta_seconds).clamp(0.0, 1.0);
            particle.velocity *= drag_factor;

            // If particle is dead, move it to free list
            if !particle.is_alive() {
                // Note: We can't modify active_particles while iterating
                // This is handled after the loop
            }
        }

        // Remove dead particles and return them to the free list (the pool
        // must recycle, otherwise the emitter dies after max_particles).
        let mut died = Vec::new();
        self.active_particles
            .retain(|&index| {
                if self.particles[index].is_alive() {
                    true
                } else {
                    died.push(index);
                    false
                }
            });
        self.free_particles.extend(died);

        // Emit new particles with fractional accumulator (small rates like
        // 10/s at 60 FPS emit 0.16/frame — truncation without accumulator
        // would never spawn).
        if self.emitting && (self.config.loop_emission || self.time < self.config.duration) {
            self.emission_accumulator += self.config.emission_rate * delta_seconds;
            let mut particles_to_emit = self.emission_accumulator as u32;
            self.emission_accumulator -= particles_to_emit as f32;
            particles_to_emit = particles_to_emit.min(self.free_particles.len() as u32);

            for _ in 0..particles_to_emit {
                self.emit_particle();
            }
        }
    }

    fn emit_particle(&mut self) {
        if self.free_particles.is_empty() {
            return;
        }

        let index = self.free_particles.pop().unwrap();

        // Compute values before taking a mutable borrow of `particles`
        let position = self.position + self.get_random_position_in_shape();
        let velocity = self.config.particle_velocity + self.get_random_velocity();
        let size = self.config.particle_size + self.get_random_size();
        let color = self.config.particle_color + self.get_random_color();
        let max_lifetime = self.config.particle_lifetime + self.get_random_lifetime();
        let rotation = self.rotation.x;
        let rotation_speed = self.config.rotation_speed + self.get_random_rotation_speed();
        let texture_index = self.config.texture_index;

        let particle = &mut self.particles[index];

        // Initialize particle
        particle.position = position;
        particle.velocity = velocity;
        particle.size = size;
        particle.color = color;
        particle.lifetime = 0.0;
        particle.max_lifetime = max_lifetime;
        particle.rotation = rotation;
        particle.rotation_speed = rotation_speed;
        particle.texture_index = texture_index;
        particle.emitter_id = index as u32;

        self.active_particles.push(index);
        self.particles_emitted += 1;
    }

    fn get_random_position_in_shape(&self) -> Vec3 {
        match self.config.emitter_shape {
            EmitterShape::Point => Vec3::ZERO,
            EmitterShape::Sphere => {
                
                use rand::Rng;
                let mut rng = rand::thread_rng();

                let radius = self.config.emitter_size.x;
                let theta = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                let phi = rng.gen_range(0.0..std::f32::consts::PI);

                Vec3::new(
                    radius * theta.sin() * phi.cos(),
                    radius * phi.sin(),
                    radius * theta.cos() * phi.cos(),
                )
            }
            EmitterShape::Box => {
                use rand::Rng;
                let mut rng = rand::thread_rng();

                Vec3::new(
                    rng.gen_range(-self.config.emitter_size.x..self.config.emitter_size.x),
                    rng.gen_range(-self.config.emitter_size.y..self.config.emitter_size.y),
                    rng.gen_range(-self.config.emitter_size.z..self.config.emitter_size.z),
                )
            }
            EmitterShape::Cone => {
                // Cone along +Y with base radius emitter_size.x, height emitter_size.y.
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let h: f32 = rng.gen_range(0.0..self.config.emitter_size.y.max(0.001));
                let r = self.config.emitter_size.x * (h / self.config.emitter_size.y.max(0.001));
                let theta: f32 = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                let rr: f32 = rng.gen_range(0.0..r.max(0.001));
                Vec3::new(rr * theta.cos(), h, rr * theta.sin())
            }
            EmitterShape::Cylinder => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let theta: f32 = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                let rr: f32 = rng.gen_range(0.0..self.config.emitter_size.x.max(0.001));
                let h: f32 = rng.gen_range(-self.config.emitter_size.y..self.config.emitter_size.y);
                Vec3::new(rr * theta.cos(), h, rr * theta.sin())
            }
            EmitterShape::Line => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let t: f32 = rng.gen_range(-1.0..1.0);
                self.config.emitter_size * t
            }
            EmitterShape::Circle => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let theta: f32 = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                let rr: f32 = rng.gen_range(0.0..self.config.emitter_size.x.max(0.001));
                Vec3::new(rr * theta.cos(), 0.0, rr * theta.sin())
            }
        }
    }

    fn get_random_velocity(&self) -> Vec3 {
        let mut rng = rand::thread_rng();

        let vx = Self::safe_range(&mut rng, self.config.particle_velocity_variation.x);
        let vy = Self::safe_range(&mut rng, self.config.particle_velocity_variation.y);
        let vz = Self::safe_range(&mut rng, self.config.particle_velocity_variation.z);
        Vec3::new(vx, vy, vz)
    }

    fn get_random_size(&self) -> Vec2 {
        let mut rng = rand::thread_rng();

        let sx = Self::safe_range(&mut rng, self.config.particle_size_variation.x);
        let sy = Self::safe_range(&mut rng, self.config.particle_size_variation.y);
        Vec2::new(sx, sy)
    }

    fn get_random_color(&self) -> Vec4 {
        let mut rng = rand::thread_rng();

        let cx = Self::safe_range(&mut rng, self.config.particle_color_variation.x);
        let cy = Self::safe_range(&mut rng, self.config.particle_color_variation.y);
        let cz = Self::safe_range(&mut rng, self.config.particle_color_variation.z);
        let cw = Self::safe_range(&mut rng, self.config.particle_color_variation.w);
        Vec4::new(cx, cy, cz, cw)
    }

    fn get_random_lifetime(&self) -> f32 {
        let mut rng = rand::thread_rng();

        Self::safe_range(&mut rng, self.config.particle_lifetime_variation)
    }

    fn get_random_rotation_speed(&self) -> f32 {
        let mut rng = rand::thread_rng();

        Self::safe_range(&mut rng, self.config.rotation_speed_variation)
    }

    fn safe_range(rng: &mut impl rand::Rng, variation: f32) -> f32 {
        if variation <= 0.0 {
            0.0
        } else {
            rng.gen_range(-variation..variation)
        }
    }

    /// Reset the emitter
    pub fn reset(&mut self) {
        self.time = 0.0;
        self.active_particles.clear();
        self.free_particles.clear();

        for i in 0..self.config.max_particles as usize {
            self.free_particles.push(i);
        }

        self.particles_emitted = 0;
    }

    /// Burst emit particles
    pub fn burst(&mut self, count: u32) {
        for _ in 0..count.min(self.free_particles.len() as u32) {
            self.emit_particle();
        }
    }
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self::new(ParticleEmitterConfig::default())
    }
}

/// Particle emitter builder
pub struct ParticleEmitterBuilder {
    config: ParticleEmitterConfig,
}

impl ParticleEmitterBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            config: ParticleEmitterConfig {
                name: name.to_string(),
                ..Default::default()
            },
        }
    }

    pub fn max_particles(mut self, count: u32) -> Self {
        self.config.max_particles = count;
        self
    }

    pub fn emission_rate(mut self, rate: f32) -> Self {
        self.config.emission_rate = rate;
        self
    }

    pub fn particle_lifetime(mut self, lifetime: f32) -> Self {
        self.config.particle_lifetime = lifetime;
        self
    }

    pub fn particle_size(mut self, size: Vec2) -> Self {
        self.config.particle_size = size;
        self
    }

    pub fn particle_color(mut self, color: Vec4) -> Self {
        self.config.particle_color = color;
        self
    }

    pub fn particle_velocity(mut self, velocity: Vec3) -> Self {
        self.config.particle_velocity = velocity;
        self
    }

    pub fn gravity(mut self, gravity: Vec3) -> Self {
        self.config.gravity = gravity;
        self
    }

    pub fn emitter_shape(mut self, shape: EmitterShape) -> Self {
        self.config.emitter_shape = shape;
        self
    }

    pub fn emitter_size(mut self, size: Vec3) -> Self {
        self.config.emitter_size = size;
        self
    }

    pub fn loop_emission(mut self, loop_emission: bool) -> Self {
        self.config.loop_emission = loop_emission;
        self
    }

    pub fn duration(mut self, duration: f32) -> Self {
        self.config.duration = duration;
        self
    }

    pub fn texture_index(mut self, index: u32) -> Self {
        self.config.texture_index = index;
        self
    }

    pub fn build(self) -> ParticleEmitter {
        ParticleEmitter::new(self.config)
    }
}
