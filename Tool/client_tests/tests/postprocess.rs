//! Integration tests for the render::postprocess module.
//!
//! Covers: PostProcessConfig defaults, the per-effect config types and their
//! defaults, ToneMapping, and lifecycle of BloomEffect, MotionBlurEffect,
//! DepthOfFieldEffect, HDREffect and FXAAEffect plus the BloomEffectBuilder.

use rfs_client::render::postprocess::{
    BloomConfig, BloomEffect, DepthOfFieldConfig, DepthOfFieldEffect, DofConfig, FXAAEffect,
    HDRConfig, HdrConfig, HDREffect, MotionBlurConfig, MotionBlurEffect, PostProcessConfig,
    ToneMapping,
};

fn assert_f32_eq(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn post_process_config_defaults() {
    let config = PostProcessConfig::default();
    assert!(config.bloom);
    assert!(config.motion_blur);
    assert!(!config.depth_of_field);
    assert!(config.hdr);
    assert!(config.fxaa);
}

#[test]
fn bloom_config_defaults() {
    let config = BloomConfig::default();
    assert_f32_eq(config.threshold, 0.8);
    assert_f32_eq(config.intensity, 0.5);
    assert_eq!(config.blur_passes, 4);
    assert_f32_eq(config.blur_radius, 2.0);
}

#[test]
fn motion_blur_config_defaults() {
    let config = MotionBlurConfig::default();
    assert_f32_eq(config.intensity, 0.5);
    assert_f32_eq(config.max_velocity, 10.0);
    assert_eq!(config.sample_count, 8);
}

#[test]
fn depth_of_field_config_defaults() {
    let config = DepthOfFieldConfig::default();
    assert_f32_eq(config.focus_distance, 5.0);
    assert_f32_eq(config.focus_range, 1.0);
    assert_f32_eq(config.blur_radius, 2.0);
    assert_eq!(config.sample_count, 8);
}

#[test]
fn hdr_config_and_tone_mapping_defaults() {
    let config = HDRConfig::default();
    assert_f32_eq(config.exposure, 1.0);
    assert_f32_eq(config.gamma, 2.2);
    assert_eq!(config.tone_mapping, ToneMapping::ACES);
    assert_eq!(ToneMapping::default(), ToneMapping::ACES);
}

#[test]
fn bloom_effect_lifecycle() {
    let mut effect = BloomEffect::new(BloomConfig::default());
    assert!(effect.enabled());
    assert_eq!(effect.config().blur_passes, 4);

    effect.set_enabled(false);
    assert!(!effect.enabled());

    effect.config_mut().intensity = 0.9;
    assert_f32_eq(effect.config().intensity, 0.9);
    assert_eq!(effect.config().threshold, 0.8);
}

#[test]
fn bloom_effect_builder() {
    let effect = rfs_client::render::postprocess::bloom::BloomEffectBuilder::new()
        .with_threshold(0.6)
        .with_intensity(1.2)
        .with_blur_passes(6)
        .with_blur_radius(3.0)
        .enabled(false)
        .build();

    assert_f32_eq(effect.config().threshold, 0.6);
    assert_f32_eq(effect.config().intensity, 1.2);
    assert_eq!(effect.config().blur_passes, 6);
    assert_f32_eq(effect.config().blur_radius, 3.0);
    assert!(!effect.enabled());
}

#[test]
fn motion_blur_effect_lifecycle() {
    let mut effect = MotionBlurEffect::new(MotionBlurConfig::default());
    assert!(effect.enabled());
    assert_eq!(effect.config().sample_count, 8);

    effect.config_mut().sample_count = 16;
    effect.config_mut().max_velocity = 20.0;
    assert_eq!(effect.config().sample_count, 16);
    assert_f32_eq(effect.config().max_velocity, 20.0);

    effect.set_enabled(false);
    assert!(!effect.enabled());
    effect.set_enabled(true);
    assert!(effect.enabled());
}

#[test]
fn depth_of_field_effect_lifecycle() {
    let mut effect = DepthOfFieldEffect::new(DepthOfFieldConfig::default());
    // Depth of field is disabled by default (performance intensive).
    assert!(!effect.enabled());
    assert_f32_eq(effect.config().focus_distance, 5.0);

    effect.config_mut().focus_distance = 8.0;
    effect.config_mut().focus_range = 2.5;
    assert_f32_eq(effect.config().focus_distance, 8.0);
    assert_f32_eq(effect.config().focus_range, 2.5);

    effect.set_enabled(false);
    assert!(!effect.enabled());
}

#[test]
fn hdr_effect_lifecycle() {
    let mut effect = HDREffect::new(HDRConfig::default());
    assert!(effect.enabled());
    assert_eq!(effect.config().tone_mapping, ToneMapping::ACES);

    effect.config_mut().exposure = 1.5;
    assert_f32_eq(effect.config().exposure, 1.5);
    assert_f32_eq(effect.config().gamma, 2.2);

    effect.set_enabled(false);
    assert!(!effect.enabled());
}

#[test]
fn fxaa_effect_lifecycle() {
    let mut effect = FXAAEffect::new();
    assert!(effect.enabled());

    effect.set_enabled(false);
    assert!(!effect.enabled());
}

#[test]
fn config_type_aliases_resolve() {
    // DofConfig and HdrConfig are documented aliases for the structs above.
    let dof = DofConfig {
        focus_distance: 1.0,
        focus_range: 0.5,
        blur_radius: 1.0,
        sample_count: 4,
    };
    let hdr = HdrConfig {
        exposure: 2.0,
        gamma: 1.8,
        tone_mapping: ToneMapping::Reinhard,
    };
    assert_f32_eq(dof.focus_distance, 1.0);
    assert_f32_eq(hdr.exposure, 2.0);
    assert_eq!(hdr.tone_mapping, ToneMapping::Reinhard);
}

#[test]
fn custom_configs_drive_effects() {
    let bloom = BloomEffect::new(BloomConfig {
        threshold: 0.5,
        intensity: 1.0,
        blur_passes: 2,
        blur_radius: 1.0,
    });
    assert_eq!(bloom.config().blur_passes, 2);
    assert_f32_eq(bloom.config().threshold, 0.5);

    let motion_blur = MotionBlurEffect::new(MotionBlurConfig {
        intensity: 0.25,
        max_velocity: 5.0,
        sample_count: 4,
    });
    assert_eq!(motion_blur.config().sample_count, 4);
}