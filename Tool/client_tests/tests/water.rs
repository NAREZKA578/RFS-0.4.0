//! Integration tests for the render::water module.
//!
//! Covers: WaterConfig, WaveSystem, WaveConfig, Wave, WaterInteraction,
//! WaterSurface, WaveMethod.

use glam::{Vec2, Vec3};
use rfs_client::render::water::waves::Wave;
use rfs_client::render::water::{
    WaveConfig, WaveMethod, WaveSystem, WaterConfig, WaterInteraction, WaterSurface,
};
use std::time::Duration;

#[test]
fn water_config_defaults() {
    let config = WaterConfig::default();
    assert_eq!(config.wave_method, WaveMethod::Gerstner);
    assert_eq!(config.color, [0.0, 0.1, 0.3, 0.7]);
    assert_eq!(config.deep_color, [0.0, 0.05, 0.2, 0.7]);
    assert_eq!(config.shallow_color, [0.0, 0.2, 0.4, 0.6]);
    assert!((config.wave_scale - 0.1).abs() < 1e-5);
    assert!((config.wave_speed - 0.5).abs() < 1e-5);
    assert!((config.wave_height - 0.2).abs() < 1e-5);
    assert!((config.depth - 100.0).abs() < 1e-5);
    assert!((config.clarity - 0.7).abs() < 1e-5);
    assert!(config.reflection_enabled);
    assert!(config.refraction_enabled);
    assert!(config.foam_enabled);
    assert!((config.fresnel_factor - 0.02).abs() < 1e-5);
}

#[test]
fn wave_system_config_matches_method() {
    let system = WaveSystem::new(WaveMethod::Simple);
    assert_eq!(system.config().method, WaveMethod::Simple);
}

#[test]
fn wave_system_config_mut_roundtrip() {
    let mut system = WaveSystem::new(WaveMethod::Gerstner);
    system.config_mut().scale = 2.5;
    system.config_mut().height = 0.8;
    assert!((system.config().scale - 2.5).abs() < 1e-5);
    assert!((system.config().height - 0.8).abs() < 1e-5);
}

#[test]
fn wave_system_heights_and_normals_are_finite_for_all_methods() {
    let methods = vec![
        WaveMethod::Flat,
        WaveMethod::Simple,
        WaveMethod::Gerstner,
        WaveMethod::FFT,
    ];
    for method in methods {
        let mut system = WaveSystem::new(method);
        system.update(Duration::from_secs_f32(0.5));

        let positions = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 10.0),
            Vec3::new(-5.0, 0.0, 3.0),
        ];
        for position in positions {
            let height = system.get_height(position);
            let normal = system.get_normal(position);
            assert!(height.is_finite(), "height must be finite for {:?}", method);
            assert!(normal.is_finite(), "normal must be finite for {:?}", method);
        }
    }
}

#[test]
fn wave_config_fields_are_accessible() {
    let config = WaveConfig::default();
    assert_eq!(config.method, WaveMethod::Gerstner);
    assert!(config.scale > 0.0);
    assert!(config.wave_count > 0);
    assert_ne!(config.direction, Vec3::ZERO);
}

#[test]
fn wave_system_manual_wave_management() {
    let mut system = WaveSystem::new(WaveMethod::Simple);
    system.clear_waves();
    assert_eq!(system.get_height(Vec3::ZERO), 0.0);

    // add_wave feeds the internal wave list, not config.wave_count, so the
    // effect is observable through get_height after time advances.
    system.add_wave(Wave::new(1.0, 1.0, 1.0, Vec3::new(1.0, 0.0, 0.0)));
    system.update(Duration::from_secs_f32(1.0));
    let h = system.get_height(Vec3::ZERO);
    assert!(
        h.abs() > 0.5,
        "a single added wave should produce height, got {h}"
    );

    system.clear_waves();
    assert!(system.get_height(Vec3::ZERO).abs() < 1e-5);
}

#[test]
fn water_interaction_object_roundtrip() {
    let mut interaction = WaterInteraction::new();
    interaction.add_object(1, Vec3::new(5.0, 0.0, 5.0), Vec3::new(20.0, 5.0, 10.0));
    interaction.update_object(1, Vec3::new(6.0, 0.0, 5.0), Vec3::new(1.0, 0.0, 0.0));
    interaction.update(0.0, Duration::from_secs_f32(0.5));

    let displacement = interaction.get_displacement(Vec3::new(6.0, 0.0, 5.0));
    assert!(displacement.is_finite());

    interaction.remove_object(1);
    interaction.update(0.0, Duration::from_secs_f32(0.5));
}

#[test]
fn water_interaction_ripple_and_splash() {
    let mut interaction = WaterInteraction::new();
    interaction.add_ripple(Vec3::new(0.0, 0.0, 0.0), 5.0, 1.0);
    interaction.add_splash(Vec3::new(10.0, 0.0, 10.0), 2.0, 1.0);
    interaction.update(1.0, Duration::from_secs_f32(0.1));

    let displacement = interaction.get_displacement(Vec3::new(0.5, 0.0, 0.5));
    assert!(displacement.is_finite());

    let foam = interaction.get_foam(Vec3::new(10.0, 0.0, 10.0), 1.0, 0.5);
    assert!(foam.is_finite());
    assert!(foam >= 0.0);
    assert!(foam <= 1.0);
}

#[test]
fn water_surface_create_mesh() {
    let mut surface = WaterSurface::new();
    assert!(surface.mesh.is_none());
    surface.set_size(Vec3::new(100.0, 0.0, 50.0));
    surface.create_mesh();
    let mesh = surface
        .mesh
        .as_ref()
        .expect("create_mesh must populate the mesh");
    assert!(!mesh.name.is_empty());
    assert!(mesh.vertex_count() > 0);
    assert!(mesh.index_count() > 0);

    let model = surface.model_matrix();
    assert!(model.is_finite());
}

#[test]
fn water_surface_setters() {
    let mut surface = WaterSurface::new();
    surface.set_size(Vec3::new(200.0, 0.0, 100.0));
    assert_eq!(surface.size, Vec3::new(200.0, 0.0, 100.0));
    surface.set_resolution(Vec2::new(32.0, 32.0));
    assert_eq!(surface.resolution, Vec2::new(32.0, 32.0));
    surface.set_tessellation_factor(6.0);
    assert!((surface.tessellation_factor - 6.0).abs() < 1e-5);
}