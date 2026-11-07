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
    /// Time since the last burst fired. Bug №236: `burst_count`/`burst_interval`
    /// were declared in the config and never read.
    burst_timer: f32,
    /// Bursts fired since `new`/`reset` (diagnostics for the schedule above).
    bursts_fired: u32,
    /// Stable identity of this emitter. Bug №235: `Particle::emitter_id` used
    /// to carry the pool slot, so a reshuffled pool re-pointed particles at
    /// another emitter.
    emitter_id: u32,
}

/// Bug №235: ids belong to emitters, not to pool slots, and must survive pool
/// reshuffles; the atomic makes them unique across the process.
static NEXT_EMITTER_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

impl ParticleEmitter {
    pub fn new(config: ParticleEmitterConfig) -> Self {
        Self::with_id(config, NEXT_EMITTER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    fn with_id(config: ParticleEmitterConfig, emitter_id: u32) -> Self {
        let mut particles = Vec::with_capacity(config.max_particles as usize);
        for _ in 0..config.max_particles {
            particles.push(Particle {
                emitter_id,
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
            burst_timer: 0.0,
            bursts_fired: 0,
            emitter_id,
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

        // Bug №236: `burst_count`/`burst_interval` were declared in the
        // config and never read — a scripted burst schedule silently emitted
        // nothing. A burst fires on its own timer, independent of the
        // continuous emission window, and stops when the emitter stops.
        if self.emitting && self.config.burst_count > 0 && self.config.burst_interval > 0.0 {
            let interval = self.config.burst_interval.max(0.001);
            self.burst_timer += delta_seconds;
            while self.burst_timer >= interval {
                self.burst_timer -= interval;
                self.burst(self.config.burst_count);
                self.bursts_fired += 1;
            }
        }
    }

    fn emit_particle(&mut self) {
        if self.free_particles.is_empty() {
            return;
        }

        let index = self.free_particles.pop().unwrap();

        // Bug №234: `set_scale` wrote a value nothing read — the emission
        // volume never grew with the emitter. The random shape offset is a
        // local quantity and must scale with the emitter.
        // Compute values before taking a mutable borrow of `particles`
        let position = self.position + self.scale * self.get_random_position_in_shape();
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
        // Bug №235: the id used to be the pool slot, so it changed on every
        // respawn and one emitter's particles carried many different ids.
        particle.emitter_id = self.emitter_id;

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

                // Bug №189: `gen_range(-s..s)` with s == 0.0 is an empty range
                // that panics on a path the default configuration can reach. A
                // zero extent must sample to (almost) zero, not abort.
                let hx = self.config.emitter_size.x.abs().max(0.001);
                let hy = self.config.emitter_size.y.abs().max(0.001);
                let hz = self.config.emitter_size.z.abs().max(0.001);
                Vec3::new(
                    rng.gen_range(-hx..hx),
                    rng.gen_range(-hy..hy),
                    rng.gen_range(-hz..hz),
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
                // Bug №189: the height range is empty when the extent is 0.0.
                let hy = self.config.emitter_size.y.abs().max(0.001);
                let h: f32 = rng.gen_range(-hy..hy);
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
        // Bug №236: a reset starts a fresh run — the leftover fractional
        // emission charge and the burst timer of the old run would fire
        // within the first tick of the new one.
        self.emission_accumulator = 0.0;
        self.burst_timer = 0.0;
        self.bursts_fired = 0;

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

#[cfg(test)]
mod bug_234_235_236_189 {
    use super::*;
    use std::collections::BTreeSet;
    use std::time::Duration;

    fn burst_config(rate: f32) -> ParticleEmitterConfig {
        let mut config = ParticleEmitterConfig::default();
        config.emission_rate = rate;
        config.burst_count = 7;
        config.burst_interval = 0.1;
        config
    }

    // Bug №234: set_scale wrote a value emit_particle never read.
    #[test]
    fn emitter_scale_grows_the_emission_volume() {
        let mut config = ParticleEmitterConfig::default();
        config.max_particles = 400;
        config.emission_rate = 40_000.0;
        config.emitter_shape = EmitterShape::Box;
        config.emitter_size = Vec3::new(1.0, 1.0, 1.0);

        let mut emitter = ParticleEmitter::new(config);
        emitter.set_scale(Vec3::new(5.0, 5.0, 5.0));
        emitter.update(Duration::from_millis(1));

        let mut max_offset = 0.0_f32;
        let mut min_offset = f32::MAX;
        for particle in emitter.live_particles() {
            let offset = (particle.position - emitter.position()).length();
            max_offset = max_offset.max(offset);
            min_offset = min_offset.min(offset);
        }
        // The unscaled box reaches sqrt(3) ~1.73; with the scale applied the
        // same shape reaches 5·sqrt(3) ~8.66. A value past the unscaled
        // envelope proves the scale is read at all.
        assert!(
            max_offset > 2.5,
            "largest offset {max_offset} — the scale never grew the volume (RED \
             of №234 stays under 1.74)"
        );
        // ...and nothing may leave the scaled envelope.
        assert!(
            max_offset <= 5.0 * 3.0f32.sqrt() + 0.1,
            "offset {max_offset} is outside the scaled box"
        );
    }

    // Bug №235: particles carried the pool slot as their emitter_id, and
    // one emitter's particles disagreed about who emitted them.
    #[test]
    fn all_particles_of_one_emitter_share_one_stable_id() {
        let mut emitter = ParticleEmitter::default();
        emitter.update(Duration::from_millis(200));

        let live = emitter.live_particles();
        assert!(!live.is_empty(), "the default config must spawn particles");
        let ids: BTreeSet<u32> = live.iter().map(|p| p.emitter_id).collect();
        assert_eq!(
            ids.len(),
            1,
            "one emitter must own one id, got {ids:?}"
        );
        assert_eq!(*ids.iter().next().unwrap(), emitter.emitter_id);
    }

    // Bug №236: burst_count/burst_interval were never read in update.
    #[test]
    fn burst_schedule_fires_on_its_interval() {
        let mut emitter = ParticleEmitter::new(burst_config(0.0));
        assert_eq!(emitter.particle_count(), 0);

        emitter.update(Duration::from_millis(100));
        assert_eq!(emitter.particle_count(), 7, "the first interval fires a burst");
        emitter.update(Duration::from_millis(100));
        assert_eq!(emitter.particle_count(), 14, "the second interval fires again");
    }

    #[test]
    fn reset_clears_the_burst_schedule() {
        let mut emitter = ParticleEmitter::new(burst_config(0.0));
        emitter.update(Duration::from_millis(100));
        assert_eq!(emitter.particle_count(), 7);

        emitter.reset();
        assert_eq!(emitter.particle_count(), 0);

        // Half an interval after the reset the schedule has not fired yet; a
        // carry-over timer from the old run would have.
        emitter.update(Duration::from_millis(95));
        assert_eq!(emitter.particle_count(), 0, "the reset must clear the burst timer");
        emitter.update(Duration::from_millis(5));
        assert_eq!(emitter.particle_count(), 7, "then the next interval fires");
    }

    #[test]
    fn bursts_respect_set_emitting_false() {
        let mut config = burst_config(0.0);
        config.max_particles = 5; // the burst clamp holds: 7 requested, 5 free
        let mut emitter = ParticleEmitter::new(config);

        emitter.set_emitting(false);
        emitter.update(Duration::from_millis(100));
        assert_eq!(emitter.particle_count(), 0, "a stopped emitter must not burst");
    }

    // Bug №189: zero extents reached gen_range(-0..0), which panics.
    #[test]
    fn zero_sized_box_and_cylinder_configs_spawn_without_panic() {
        for shape in [EmitterShape::Box, EmitterShape::Cylinder] {
            let mut config = ParticleEmitterConfig::default();
            config.max_particles = 100;
            config.emission_rate = 100.0;
            config.emitter_shape = shape;
            config.emitter_size = Vec3::ZERO;

            let mut emitter = ParticleEmitter::new(config);
            emitter.update(Duration::from_millis(16));
            assert!(
                emitter.particle_count() > 0,
                "shape {shape:?} must still spawn with zero extents"
            );
        }
    }

    #[test]
    fn default_config_spawns_without_panic() {
        let mut emitter = ParticleEmitter::default();
        emitter.update(Duration::from_millis(200));
        assert_eq!(
            emitter.particle_count(),
            2,
            "rate 10/s over 0.2 s must spawn 2 particles"
        );
    }
}
