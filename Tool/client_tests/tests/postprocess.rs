//! Integration tests for the render::postprocess module.
//!
//! Covers: PostProcessConfig defaults, the per-effect config types and their
//! defaults, ToneMapping, and lifecycle of BloomEffect, MotionBlurEffect,
//! DepthOfFieldEffect, HDREffect and FXAAEffect plus the BloomEffectBuilder.

use rfs_client::render::postprocess::{
    BloomConfig, BloomEffect, DepthOfFieldConfig, DepthOfFieldEffect, DofConfig, FXAAEffect,
    HDRConfig, HdrConfig, HDREffect, MotionBlurConfig, MotionBlurEffect, PostProcessConfig,
    PostProcessManager, ToneMapping,
};
use rfs_client::render::RenderContext;
use rfs_client::render::scene::Scene;
use rhi::command::{
    CommandBuffer, CommandBufferDesc, CommandBufferFlags, CommandBufferLevel, CommandEncoder,
    CommandPool, CommandPoolDesc, CommandPoolFlags,
};
use rhi::core::device::DeviceDesc;
use rhi::types::Features;
use rhi::{
    Format, RhiError, Texture, TextureAspectFlags, TextureDesc, TextureUsage, TextureView,
    TextureViewDesc, TextureViewType,
};
use std::sync::Arc;
use std::sync::Arc;

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

// ---------------------------------------------------------------------------
// Bug №204: the honest shells must refuse to fake work. Each refusal is
// machine-checked, so the honest form cannot rot back into silent lying.
// ---------------------------------------------------------------------------

fn open_device() -> Arc<rhi::Device> {
    let backend = rhi::backend::create_backend(&rhi::RhiConfig {
        validation: true,
        ..rhi::RhiConfig::default()
    })
    .expect("backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    assert!(!devices.is_empty(), "no Vulkan device available");
    Arc::new(
        backend
            .create_device(
                &devices[0],
                &DeviceDesc {
                    features: Features::default(),
                    ..Default::default()
                },
            )
            .expect("create device"),
    )
}

/// CPU views suffice for the refusal paths: nothing in them touches a GPU
/// object. The real device exists only to build the `RenderContext` the
/// effect signatures require.
fn cpu_view(width: u32, height: u32) -> TextureView {
    let texture = Texture::new(TextureDesc {
        width,
        height,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::COLOR_ATTACHMENT | TextureUsage::SAMPLED,
        ..Default::default()
    });
    texture.create_view(TextureViewDesc {
        texture: texture.clone(),
        format: None,
        view_type: TextureViewType::D2,
        aspects: TextureAspectFlags::COLOR,
        base_mip_level: 0,
        mip_level_count: 1,
        base_array_layer: 0,
        array_layer_count: 1,
    })
}

fn cpu_encoder() -> CommandEncoder {
    let pool = CommandPool::new(CommandPoolDesc {
        queue_family_index: 0,
        flags: CommandPoolFlags::TRANSIENT | CommandPoolFlags::RESET_COMMAND_BUFFER,
    });
    let buffer = CommandBuffer::new(
        pool,
        CommandBufferDesc {
            level: CommandBufferLevel::Primary,
            queue_family_index: Some(0),
            flags: CommandBufferFlags::ONE_TIME_SUBMIT,
        },
    );
    let mut encoder = CommandEncoder::new(buffer);
    encoder.begin().expect("begin");
    encoder
}

fn manager_with_every_effect_disabled(device: Arc<rhi::Device>) -> PostProcessManager {
    let mut manager = PostProcessManager::new(device);
    manager.bloom_mut().set_enabled(false);
    manager.motion_blur_mut().set_enabled(false);
    manager.hdr_mut().set_enabled(false);
    manager.fxaa_mut().set_enabled(false);
    // depth_of_field is disabled by default; assert that, so the helper's
    // meaning cannot silently drift if the default ever changes.
    assert!(!manager.depth_of_field().enabled());
    manager
}

#[test]
fn fxaa_refuses_to_initialize_instead_of_faking_a_target() {
    let device = open_device();
    let context = RenderContext::headless(device.clone());
    let mut effect = FXAAEffect::new();

    let err = effect
        .initialize(&device, &context, Format::RGBA8_UNORM)
        .expect_err("the unbuilt FXAA stage must refuse to prepare its target");
    assert!(
        matches!(err, RhiError::NotSupported(_)),
        "the refusal must be NotSupported, got {err:?}"
    );
}

#[test]
fn fxaa_refuses_to_apply_instead_of_faking_antialiasing() {
    let device = open_device();
    let context = RenderContext::headless(device);
    let mut effect = FXAAEffect::new();
    let input = cpu_view(16, 16);
    let output = cpu_view(16, 16);
    let mut encoder = cpu_encoder();

    assert!(effect.enabled());
    let err = effect
        .apply(&mut encoder, &context, &input, &output)
        .expect_err("an enabled unbuilt effect must refuse, not fake");
    assert!(
        matches!(err, RhiError::NotSupported(_)),
        "the refusal must be NotSupported, got {err:?}"
    );

    effect.set_enabled(false);
    let result = effect.apply(&mut encoder, &context, &input, &output);
    assert!(result.is_ok(), "a disabled effect is an honest no-op");
}

#[test]
fn an_enabled_unbuilt_effect_refuses_through_the_manager() {
    let device = open_device();
    let context = RenderContext::headless(device.clone());
    // FXAA stays enabled by default; every strictly-earlier stage is disabled
    // so the refusal that travels up the chain is exactly FXAA's.
    let mut manager = PostProcessManager::new(device);
    manager.bloom_mut().set_enabled(false);
    manager.motion_blur_mut().set_enabled(false);
    manager.hdr_mut().set_enabled(false);

    let input = cpu_view(16, 16);
    let output = cpu_view(16, 16);
    let scene = Scene::new("fxaa-gate");
    let mut encoder = cpu_encoder();

    let err = manager
        .apply(&mut encoder, &context, &scene, &input, None, &output)
        .expect_err("the manager must propagate an enabled effect's refusal");
    assert!(
        matches!(err, RhiError::NotSupported(_)),
        "the propagated refusal must be NotSupported, got {err:?}"
    );
}

#[test]
fn the_chain_with_every_effect_disabled_is_an_honest_noop_for_the_same_view() {
    let device = open_device();
    let context = RenderContext::headless(device.clone());
    let mut manager = manager_with_every_effect_disabled(device);
    let view = cpu_view(16, 16);
    let scene = Scene::new("chain");
    let mut encoder = cpu_encoder();

    let result = manager.apply(&mut encoder, &context, &scene, &view, None, &view);
    assert!(
        result.is_ok(),
        "all effects disabled and one shared view: nothing to deliver, \
         the no-op is honest"
    );
}

#[test]
fn the_chain_with_every_effect_disabled_refuses_to_copy_distinct_views() {
    let device = open_device();
    let context = RenderContext::headless(device.clone());
    let mut manager = manager_with_every_effect_disabled(device);
    let input = cpu_view(16, 16);
    let output = cpu_view(16, 16);
    let scene = Scene::new("chain");
    let mut encoder = cpu_encoder();

    let err = manager
        .apply(&mut encoder, &context, &scene, &input, None, &output)
        .expect_err("real frame delivery between distinct views does not exist yet");
    assert!(
        matches!(err, RhiError::NotSupported(_)),
        "the refusal must be NotSupported, got {err:?}"
    );
}

#[test]
fn an_enabled_depth_of_field_refuses_a_missing_depth_view() {
    let device = open_device();
    let context = RenderContext::headless(device.clone());
    let mut manager = PostProcessManager::new(device);
    manager.bloom_mut().set_enabled(false);
    manager.motion_blur_mut().set_enabled(false);
    manager.hdr_mut().set_enabled(false);
    manager.fxaa_mut().set_enabled(false);
    manager.depth_of_field_mut().set_enabled(true);

    let input = cpu_view(16, 16);
    let output = cpu_view(16, 16);
    let scene = Scene::new("dof-gate");
    let mut encoder = cpu_encoder();

    let err = manager
        .apply(&mut encoder, &context, &scene, &input, None, &output)
        .expect_err("an enabled effect that cannot run must refuse, not skip");
    assert!(
        matches!(err, RhiError::NotSupported(_)),
        "the refusal must be NotSupported, got {err:?}"
    );
}