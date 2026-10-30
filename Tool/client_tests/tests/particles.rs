//! Integration tests for the render::particles module.
//!
//! Covers: Particle, ParticleEmitter, ParticleEmitterConfig,
//! ParticleEmitterBuilder and EmitterShape. The built-in effect presets
//! (Smoke, Fire, Splash, Explosion, Sparks, Dust, Blood, Magic) and
//! `ParticleEffectManager` were removed 2026-10-28 by owner decision
//! (№4 of the owner's list): the manager was never constructed anywhere in
//! the crate, and the presets spawned particles that nothing drew.

use glam::{Vec2, Vec3, Vec4};
use rfs_client::render::particles::emitter::ParticleEmitterBuilder;
use rfs_client::render::particles::{EmitterShape, Particle, ParticleEmitter, ParticleEmitterConfig};
use std::time::Duration;

const EPSILON: f32 = 1e-3;

// ParticleEmitterConfig::default() has zero velocity/size/color/rotation
// variations, and emit_particle samples `gen_range(-v..v)`, so rand panics
// on the empty `0.0..0.0` range. Give spawn points a tiny non-zero spread.
fn enable_non_empty_ranges(emitter: &mut ParticleEmitter) {
    let c = emitter.config_mut();
    c.particle_velocity_variation = Vec3::splat(1e-4);
    c.particle_size_variation = Vec2::splat(1e-4);
    c.particle_color_variation = Vec4::splat(1e-4);
    c.rotation_speed_variation = 1e-4;
}

#[test]
fn particle_lifecycle() {
    let mut particle = Particle::new();
    assert!(particle.is_alive());
    assert_eq!(particle.position, Vec3::ZERO);
    assert_eq!(particle.velocity, Vec3::ZERO);

    particle.update(Duration::from_secs_f32(0.5));
    assert!(particle.is_alive());

    particle.update(Duration::from_secs_f32(1.0));
    assert!(!particle.is_alive());
    assert_eq!(particle.lifetime, 1.5);
    assert_eq!(particle.max_lifetime, 1.0);
}

#[test]
fn particle_update_moves_position() {
    let mut particle = Particle::new();
    particle.velocity = Vec3::new(2.0, 0.0, 0.0);
    particle.update(Duration::from_secs_f32(0.5));
    assert_eq!(particle.position, Vec3::new(1.0, 0.0, 0.0));
}

#[test]
fn emitter_default_config() {
    let config = ParticleEmitterConfig::default();
    assert_eq!(config.name, "default");
    assert_eq!(config.max_particles, 1000);
    assert_eq!(config.emission_rate, 10.0);
    assert_eq!(config.particle_lifetime, 1.0);
    assert_eq!(config.particle_size, Vec2::new(0.1, 0.1));
    assert_eq!(config.particle_color, Vec4::ONE);
    assert_eq!(config.particle_velocity, Vec3::ZERO);
    assert_eq!(config.emitter_shape, EmitterShape::Point);
    assert_eq!(config.gravity, Vec3::ZERO);
    assert!(config.loop_emission);
}

#[test]
fn emitter_builder_creates_configuration() {
    let emitter = ParticleEmitterBuilder::new("test")
        .max_particles(50)
        .emission_rate(5.0)
        .particle_lifetime(2.0)
        .particle_size(Vec2::new(0.5, 0.5))
        .particle_color(Vec4::new(1.0, 0.0, 0.0, 1.0))
        .particle_velocity(Vec3::new(0.0, 5.0, 0.0))
        .gravity(Vec3::new(0.0, -9.8, 0.0))
        .emitter_shape(EmitterShape::Sphere)
        .loop_emission(false)
        .build();

    let config = emitter.config();
    assert_eq!(config.name, "test");
    assert_eq!(config.max_particles, 50);
    assert_eq!(config.emission_rate, 5.0);
    assert_eq!(config.particle_lifetime, 2.0);
    assert_eq!(config.particle_size, Vec2::new(0.5, 0.5));
    assert_eq!(config.particle_velocity, Vec3::new(0.0, 5.0, 0.0));
    assert_eq!(config.gravity, Vec3::new(0.0, -9.8, 0.0));
    assert_eq!(config.emitter_shape, EmitterShape::Sphere);
    assert!(!config.loop_emission);
}

#[test]
fn emitter_initial_state() {
    let emitter = ParticleEmitter::default();
    assert!(emitter.is_active());
    assert!(emitter.is_emitting());
    assert_eq!(emitter.particle_count(), 0);
    assert_eq!(emitter.position(), Vec3::ZERO);
    assert_eq!(emitter.scale(), Vec3::ONE);
}

#[test]
fn emitter_burst_emits_exact_count() {
    let mut emitter = ParticleEmitter::default();
    enable_non_empty_ranges(&mut emitter);
    emitter.burst(5);
    assert_eq!(emitter.particle_count(), 5);
    emitter.burst(2);
    assert_eq!(emitter.particle_count(), 7);
}

#[test]
fn emitter_burst_never_exceeds_particle_pool() {
    let emitter = ParticleEmitterBuilder::new("bounded")
        .max_particles(16)
        .build();
    let mut emitter = emitter;
    enable_non_empty_ranges(&mut emitter);
    emitter.burst(u32::MAX);
    assert!(emitter.particle_count() <= 16);
}

#[test]
fn emitter_update_emits_at_rate() {
    let mut emitter = ParticleEmitter::default();
    enable_non_empty_ranges(&mut emitter);
    emitter.update(Duration::from_secs_f32(1.0));
    assert_eq!(emitter.particle_count(), 10);
}

#[test]
fn emitter_velocity_moves_particles() {
    let mut emitter = ParticleEmitterBuilder::new("flow")
        .max_particles(100)
        .particle_velocity(Vec3::new(0.0, 10.0, 0.0))
        .particle_lifetime(10.0)
        .build();
    enable_non_empty_ranges(&mut emitter);
    emitter.burst(1);
    emitter.update(Duration::from_secs_f32(0.5));

    let emitted = emitter
        .particles()
        .iter()
        .find(|p| p.lifetime > 0.0)
        .expect("an emitted particle exists");
    assert!(
        (emitted.position.y - 5.0).abs() < EPSILON,
        "particle should rise 5 units, got {}",
        emitted.position.y
    );
}

#[test]
fn emitter_gravity_applies_to_particles() {
    let mut emitter = ParticleEmitterBuilder::new("gravity")
        .max_particles(100)
        .particle_velocity(Vec3::new(0.0, 5.0, 0.0))
        .gravity(Vec3::new(0.0, -9.8, 0.0))
        .particle_lifetime(10.0)
        .build();
    enable_non_empty_ranges(&mut emitter);
    emitter.burst(1);
    emitter.update(Duration::from_secs_f32(1.0));

    let emitted = emitter
        .particles()
        .iter()
        .find(|p| p.lifetime > 0.0)
        .expect("an emitted particle exists");
    assert!(
        (emitted.velocity.y - (-4.8)).abs() < 0.1,
        "velocity.y should be -4.8, got {}",
        emitted.velocity.y
    );
}

#[test]
fn emitter_can_be_paused() {
    let mut emitter = ParticleEmitter::default();
    emitter.set_emitting(false);
    emitter.update(Duration::from_secs_f32(1.0));
    assert_eq!(emitter.particle_count(), 0);
    assert!(!emitter.is_emitting());
}

#[test]
fn emitter_config_mut_allows_runtime_changes() {
    let mut emitter = ParticleEmitterBuilder::new("tuned").build();
    emitter.config_mut().emission_rate = 25.0;
    emitter.config_mut().emitter_shape = EmitterShape::Circle;
    assert_eq!(emitter.config().emission_rate, 25.0);
    assert_eq!(emitter.config().emitter_shape, EmitterShape::Circle);
}