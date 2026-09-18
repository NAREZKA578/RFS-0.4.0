//! Integration tests for the render::core::settings module.
//!
//! Covers: GraphicsSettings and its nested post-process/effect/water/particle/
//! performance/advanced settings, the related enum types, GraphicsSettingsManager
//! (defaults, changes, reset, save/load and quality presets), plus RendererConfig,
//! RendererSettings and RayTracingSettings.

use glam::Vec2;
use rfs_client::render::core::settings::{
    AdvancedSettings, BloomSettings, DepthOfFieldSettings, DisplayMode, EffectSettings,
    FxaaQuality, FxaaSettings, GraphicsSettings, GraphicsSettingsManager, HdrSettings,
    MotionBlurSettings, MsaaSamples, ParticleQuality, ParticleSortMode,
    PerformanceSettings, PostProcessSettings, QualityPreset, RayTracingQuality,
    RayTracingSettings, RenderApi, RendererConfig, RendererSettings, ShadowQuality,
    TextureQuality, ToneMappingMode, WaterQuality, WaterSettings, WaveMethod,
};

#[test]
fn display_mode_and_render_api_variants() {
    assert!(matches!(DisplayMode::Windowed, DisplayMode::Windowed));
    assert!(matches!(DisplayMode::Fullscreen, DisplayMode::Fullscreen));
    assert!(matches!(DisplayMode::Borderless, DisplayMode::Borderless));

    assert!(matches!(RenderApi::Auto, RenderApi::Auto));
    assert!(matches!(RenderApi::Vulkan, RenderApi::Vulkan));
    assert!(matches!(RenderApi::Direct3D12, RenderApi::Direct3D12));
    assert!(matches!(RenderApi::Direct3D11, RenderApi::Direct3D11));
    assert!(matches!(RenderApi::OpenGL, RenderApi::OpenGL));
}

#[test]
fn graphics_settings_defaults() {
    let settings = GraphicsSettings::default();

    assert_eq!(settings.resolution, Vec2::new(1920.0, 1080.0));
    assert!(!settings.fullscreen);
    assert!(settings.vsync);
    assert_eq!(settings.refresh_rate, 60);
    assert_eq!(settings.display_mode, DisplayMode::Windowed);

    assert_eq!(settings.quality_preset, QualityPreset::High);
    assert_eq!(settings.texture_quality, TextureQuality::High);
    assert_eq!(settings.shadow_quality, ShadowQuality::High);
    assert_eq!(settings.particle_quality, 1);
    assert_eq!(settings.lod_distance, 50.0);
    assert_eq!(settings.max_lod_level, 3);

    assert_eq!(settings.render_api, RenderApi::Auto);
    assert_eq!(settings.msaa_samples, MsaaSamples::X4);
    assert_eq!(settings.anisotropy_level, 16);

    assert_eq!(settings.max_lights, 32);
    assert_eq!(settings.shadow_resolution, Vec2::new(2048.0, 2048.0));
    assert_eq!(settings.shadow_cascade_count, 4);
    assert_eq!(settings.shadow_distance, 100.0);
    assert_eq!(settings.shadow_bias, 0.001);
    assert_eq!(settings.shadow_softness, 1.0);
}

#[test]
fn post_process_settings_defaults() {
    let settings = GraphicsSettings::default();
    let post = &settings.post_process;

    assert!(post.bloom.enabled);
    assert_eq!(post.bloom.intensity, 0.5);
    assert_eq!(post.bloom.threshold, 0.8);
    assert_eq!(post.bloom.radius, 0.5);
    assert_eq!(post.bloom.iterations, 4);

    assert!(post.motion_blur.enabled);
    assert_eq!(post.motion_blur.intensity, 0.5);
    assert_eq!(post.motion_blur.sample_count, 8);
    assert_eq!(post.motion_blur.max_velocity, 100.0);

    assert!(post.depth_of_field.enabled);
    assert_eq!(post.depth_of_field.focus_distance, 10.0);
    assert_eq!(post.depth_of_field.focus_range, 5.0);
    assert_eq!(post.depth_of_field.blur_radius, 0.5);
    assert_eq!(post.depth_of_field.blur_iterations, 2);

    assert!(post.hdr.enabled);
    assert_eq!(post.hdr.exposure, 1.0);
    assert_eq!(post.hdr.gamma, 2.2);
    assert_eq!(post.hdr.tone_mapping, ToneMappingMode::ACES);

    assert!(post.fxaa.enabled);
    assert_eq!(post.fxaa.quality, FxaaQuality::Medium);
}

#[test]
fn effect_and_water_settings_defaults() {
    let settings = GraphicsSettings::default();
    let effects = &settings.effects;
    assert!(effects.ssao);
    assert!(effects.ssr);
    assert!(effects.god_rays);
    assert!(effects.lens_flare);
    assert!(effects.volumetric_fog);
    assert!(effects.underwater);

    let water = &settings.water;
    assert_eq!(water.quality, WaterQuality::Medium);
    assert_eq!(water.wave_method, WaveMethod::Gerstner);
    assert!(water.tessellation);
    assert!(water.reflections);
    assert!(water.refractions);
    assert!(water.foam);
    assert!(water.caustics);
}

#[test]
fn particle_performance_advanced_settings_defaults() {
    let settings = GraphicsSettings::default();

    assert_eq!(settings.particles.max_particles, 10000);
    assert_eq!(settings.particles.quality, ParticleQuality::High);
    assert_eq!(settings.particles.sort_mode, ParticleSortMode::BackToFront);

    assert_eq!(settings.performance.fps_limit, 144);
    assert_eq!(settings.performance.frame_budget_ms, 16.67);
    assert_eq!(settings.performance.gpu_memory_limit_mb, 4096);
    assert_eq!(settings.performance.cpu_threads, 0);
    assert!(settings.performance.async_compute);

    assert!(settings.advanced.ray_tracing);
    assert_eq!(settings.advanced.ray_tracing_quality, RayTracingQuality::Medium);
    assert!(!settings.advanced.mesh_shading);
    assert!(!settings.advanced.variable_rate_shading);
    assert!(!settings.advanced.bindless_resources);
    assert!(!settings.advanced.debug_overlay);
    assert!(settings.advanced.stats_overlay);
}

#[test]
fn graphics_settings_fields_are_mutable() {
    let mut settings = GraphicsSettings {
        resolution: Vec2::new(2560.0, 1440.0),
        fullscreen: true,
        vsync: false,
        refresh_rate: 144,
        msaa_samples: MsaaSamples::X8,
        max_lights: 64,
        shadow_bias: 0.005,
        ..Default::default()
    };
    settings.post_process.bloom.iterations = 5;
    settings.advanced.mesh_shading = true;

    assert_eq!(settings.resolution, Vec2::new(2560.0, 1440.0));
    assert!(settings.fullscreen);
    assert!(!settings.vsync);
    assert_eq!(settings.refresh_rate, 144);
    assert_eq!(settings.msaa_samples, MsaaSamples::X8);
    assert_eq!(settings.max_lights, 64);
    assert_eq!(settings.shadow_bias, 0.005);
    assert_eq!(settings.post_process.bloom.iterations, 5);
    assert!(settings.advanced.mesh_shading);
}

#[test]
fn manager_current_mut_marks_changes_and_apply_clears() {
    let mut manager = GraphicsSettingsManager::new();
    assert!(!manager.has_changes());

    manager.current_mut().msaa_samples = MsaaSamples::X16;
    assert!(manager.has_changes());

    manager.apply();
    assert!(!manager.has_changes());
    assert_eq!(manager.current().msaa_samples, MsaaSamples::X16);
}

#[test]
fn manager_reset_restores_defaults() {
    let mut manager = GraphicsSettingsManager::new();
    manager.current_mut().msaa_samples = MsaaSamples::X16;
    manager.current_mut().max_lights = 8;
    manager.reset();

    let current = manager.current();
    assert_eq!(current.msaa_samples, MsaaSamples::X4);
    assert_eq!(current.max_lights, 32);
    assert!(manager.has_changes());
}

#[test]
fn manager_save_load_roundtrip() {
    let mut manager = GraphicsSettingsManager::new();
    manager.current_mut().fullscreen = true;
    manager.current_mut().msaa_samples = MsaaSamples::X8;
    manager.current_mut().max_lights = 128;
    manager.current_mut().post_process.bloom.iterations = 6;

    let data = manager.save().expect("settings serialize");
    let mut restored = GraphicsSettingsManager::new();
    restored.load(&data).expect("settings deserialize");

    let current = restored.current();
    assert!(current.fullscreen);
    assert_eq!(current.msaa_samples, MsaaSamples::X8);
    assert_eq!(current.max_lights, 128);
    assert_eq!(current.post_process.bloom.iterations, 6);
    // resolution and shadow_resolution are #[serde(skip)] so they come back default (zero)
    assert_eq!(current.resolution, Vec2::ZERO);
    assert_eq!(current.shadow_resolution, Vec2::ZERO);
    assert!(restored.has_changes());
}

#[test]
fn set_preset_low_applies_low_values() {
    let mut manager = GraphicsSettingsManager::new();
    manager.set_preset(QualityPreset::Low);
    let s = manager.current();

    assert_eq!(s.quality_preset, QualityPreset::Low);
    assert_eq!(s.texture_quality, TextureQuality::Low);
    assert_eq!(s.shadow_quality, ShadowQuality::Low);
    assert_eq!(s.shadow_resolution, Vec2::new(1024.0, 1024.0));
    assert_eq!(s.shadow_cascade_count, 2);
    assert_eq!(s.msaa_samples, MsaaSamples::X2);
    assert_eq!(s.anisotropy_level, 2);
    assert_eq!(s.max_lights, 16);
    assert_eq!(s.post_process.bloom.iterations, 2);
    assert_eq!(s.post_process.motion_blur.sample_count, 4);
    assert_eq!(s.water.quality, WaterQuality::Low);
    assert!(!s.water.tessellation);
    assert_eq!(s.particles.max_particles, 5000);
    assert_eq!(s.particles.quality, ParticleQuality::Low);
    assert!(!s.advanced.ray_tracing);
}

#[test]
fn set_preset_medium_applies_medium_values() {
    let mut manager = GraphicsSettingsManager::new();
    manager.set_preset(QualityPreset::Medium);
    let s = manager.current();

    assert_eq!(s.quality_preset, QualityPreset::Medium);
    assert_eq!(s.texture_quality, TextureQuality::Medium);
    assert_eq!(s.shadow_quality, ShadowQuality::Medium);
    assert_eq!(s.shadow_resolution, Vec2::new(1536.0, 1536.0));
    assert_eq!(s.shadow_cascade_count, 3);
    assert_eq!(s.msaa_samples, MsaaSamples::X4);
    assert_eq!(s.anisotropy_level, 8);
    assert_eq!(s.max_lights, 32);
    assert_eq!(s.post_process.bloom.iterations, 3);
    assert_eq!(s.post_process.motion_blur.sample_count, 6);
    assert_eq!(s.water.quality, WaterQuality::Medium);
    assert_eq!(s.particles.max_particles, 7500);
    assert_eq!(s.particles.quality, ParticleQuality::Medium);
    assert!(s.advanced.ray_tracing);
}

#[test]
fn set_preset_high_applies_high_values() {
    let mut manager = GraphicsSettingsManager::new();
    manager.set_preset(QualityPreset::High);
    let s = manager.current();

    assert_eq!(s.quality_preset, QualityPreset::High);
    assert_eq!(s.texture_quality, TextureQuality::High);
    assert_eq!(s.shadow_quality, ShadowQuality::High);
    assert_eq!(s.shadow_resolution, Vec2::new(2048.0, 2048.0));
    assert_eq!(s.shadow_cascade_count, 4);
    assert_eq!(s.msaa_samples, MsaaSamples::X4);
    assert_eq!(s.anisotropy_level, 16);
    assert_eq!(s.max_lights, 64);
    assert_eq!(s.post_process.bloom.iterations, 4);
    assert_eq!(s.post_process.motion_blur.sample_count, 8);
    assert_eq!(s.water.quality, WaterQuality::High);
    assert_eq!(s.particles.max_particles, 10000);
    assert_eq!(s.particles.quality, ParticleQuality::High);
    assert!(s.advanced.ray_tracing);
    assert_eq!(s.advanced.ray_tracing_quality, RayTracingQuality::Medium);
}

#[test]
fn set_preset_ultra_applies_ultra_values() {
    let mut manager = GraphicsSettingsManager::new();
    manager.set_preset(QualityPreset::Ultra);
    let s = manager.current();

    assert_eq!(s.quality_preset, QualityPreset::Ultra);
    assert_eq!(s.texture_quality, TextureQuality::Ultra);
    assert_eq!(s.shadow_quality, ShadowQuality::Ultra);
    assert_eq!(s.shadow_resolution, Vec2::new(4096.0, 4096.0));
    assert_eq!(s.msaa_samples, MsaaSamples::X8);
    assert_eq!(s.anisotropy_level, 16);
    assert_eq!(s.max_lights, 128);
    assert_eq!(s.post_process.bloom.iterations, 5);
    assert_eq!(s.post_process.motion_blur.sample_count, 12);
    assert_eq!(s.water.quality, WaterQuality::Ultra);
    assert_eq!(s.particles.max_particles, 15000);
    assert_eq!(s.particles.quality, ParticleQuality::Ultra);
    assert!(s.advanced.ray_tracing);
    assert_eq!(s.advanced.ray_tracing_quality, RayTracingQuality::High);
    assert!(s.advanced.mesh_shading);
}

#[test]
fn set_preset_custom_keeps_current_values() {
    let mut manager = GraphicsSettingsManager::new();
    manager.current_mut().msaa_samples = MsaaSamples::X16;
    manager.current_mut().max_lights = 4;

    manager.set_preset(QualityPreset::Custom);
    assert_eq!(manager.current().quality_preset, QualityPreset::Custom);
    assert_eq!(manager.current().msaa_samples, MsaaSamples::X16);
    assert_eq!(manager.current().max_lights, 4);
}

#[test]
fn renderer_config_defaults() {
    let config = RendererConfig::default();
    assert_eq!(config.backend, RenderApi::Auto);
    assert!(config.device_name.is_empty());
    assert!(config.vendor.is_empty());
    assert_eq!(config.memory, 0);
    assert!(!config.supports_ray_tracing);
    assert!(!config.supports_mesh_shading);
    assert!(!config.supports_bindless);
    assert_eq!(config.max_texture_size, 4096);
    assert_eq!(config.max_compute_work_group_size, [64, 64, 64]);
}

#[test]
fn renderer_settings_defaults() {
    let settings = RendererSettings::default();
    assert!(settings.ray_tracing.enabled);
    assert_eq!(settings.ray_tracing.quality, RayTracingQuality::Medium);
    assert_eq!(settings.shadow_quality, ShadowQuality::High);
    assert_eq!(RayTracingQuality::default(), RayTracingQuality::Medium);
}

// Ensure the nested settings types can be constructed independently.
#[test]
fn nested_settings_construct_independently() {
    assert!(BloomSettings::default().enabled);
    assert!(MotionBlurSettings::default().enabled);
    assert!(DepthOfFieldSettings::default().enabled);
    assert!(HdrSettings::default().enabled);
    assert!(FxaaSettings::default().enabled);
    assert!(EffectSettings::default().ssao);
    assert!(WaterSettings::default().foam);
    assert!(PerformanceSettings::default().async_compute);
    assert!(AdvancedSettings::default().stats_overlay);
    assert!(RayTracingSettings::default().enabled);
    assert!(PostProcessSettings::default().bloom.enabled);
}