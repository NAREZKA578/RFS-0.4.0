//! Effect Manager
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Central manager for all visual effects

use crate::render::core::{RenderContext, Renderer};
use crate::render::particles::ParticleEffectManager;
use crate::render::postprocess::PostProcessConfig;
use crate::render::water::{WaterConfig, WaterRenderer};
use glam::Vec3;
use std::collections::HashMap;

/// Effect type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisualEffectType {
    // Post-process effects
    Bloom,
    MotionBlur,
    DepthOfField,
    HDR,
    FXAA,
    SSAO,
    SSR,

    // Water effects
    Water,
    Reflection,
    Refraction,

    // Particle effects
    Smoke,
    Fire,
    Splash,
    Explosion,
    Sparks,
    Dust,
    Blood,
    Magic,

    // Other effects
    Underwater,
    GodRays,
    LensFlare,
    VolumetricFog,
    ScreenSpaceReflection,
}

/// Effect settings for each type
#[derive(Debug, Clone)]
pub struct EffectSettings {
    pub enabled: bool,
    pub quality: EffectQuality,
    pub intensity: f32,
}

/// Global effect quality
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectQuality {
    Off,
    Low,
    Medium,
    High,
    Ultra,
}

impl Default for EffectQuality {
    fn default() -> Self {
        Self::Medium
    }
}

/// Central effect manager
pub struct EffectManager {
    /// Render context
    _context: RenderContext,

    /// Post-process effects
    post_process_config: PostProcessConfig,

    /// Water renderer
    water_renderer: Option<WaterRenderer>,

    /// Particle effect manager
    particle_manager: ParticleEffectManager,

    /// Underwater effect
    underwater_effect: Option<super::underwater::UnderwaterEffect>,

    /// Screen space effects
    _ssao_config: Option<super::screen_space::SSAOConfig>,
    _ssr_config: Option<super::screen_space::SSRConfig>,

    /// Per-effect settings
    effect_settings: HashMap<VisualEffectType, EffectSettings>,

    /// Global settings
    global_settings: GlobalEffectSettings,
}

/// Global effect settings
#[derive(Debug, Clone)]
pub struct GlobalEffectSettings {
    pub enabled: bool,
    pub quality: EffectQuality,
    pub max_particles: u32,
    pub motion_blur_enabled: bool,
    pub dof_enabled: bool,
    pub bloom_enabled: bool,
    pub hdr_enabled: bool,
    pub ssao_enabled: bool,
    pub ssr_enabled: bool,
    pub underwater_enabled: bool,
}

impl Default for GlobalEffectSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            quality: EffectQuality::Medium,
            max_particles: 10000,
            motion_blur_enabled: true,
            dof_enabled: true,
            bloom_enabled: true,
            hdr_enabled: true,
            ssao_enabled: true,
            ssr_enabled: true,
            underwater_enabled: true,
        }
    }
}

impl EffectManager {
    /// Creates a new effect manager
    pub fn new(context: RenderContext) -> Self {
        let particle_manager = ParticleEffectManager::new(&context, 10000);
        Self {
            _context: context,
            post_process_config: PostProcessConfig::default(),
            water_renderer: None,
            particle_manager,
            underwater_effect: None,
            _ssao_config: Some(super::screen_space::SSAOConfig::default()),
            _ssr_config: Some(super::screen_space::SSRConfig::default()),
            effect_settings: HashMap::new(),
            global_settings: GlobalEffectSettings::default(),
        }
    }

    /// Initializes the effect manager with default settings
    pub fn initialize(&mut self) {
        // Initialize default settings for each effect type
        let effect_types = [
            VisualEffectType::Bloom,
            VisualEffectType::MotionBlur,
            VisualEffectType::DepthOfField,
            VisualEffectType::HDR,
            VisualEffectType::FXAA,
            VisualEffectType::SSAO,
            VisualEffectType::SSR,
            VisualEffectType::Water,
            VisualEffectType::Smoke,
            VisualEffectType::Fire,
            VisualEffectType::Splash,
            VisualEffectType::Explosion,
            VisualEffectType::Sparks,
            VisualEffectType::Dust,
            VisualEffectType::Blood,
            VisualEffectType::Magic,
            VisualEffectType::Underwater,
        ];

        for effect_type in effect_types.iter() {
            self.effect_settings.insert(
                *effect_type,
                EffectSettings {
                    enabled: true,
                    quality: EffectQuality::Medium,
                    intensity: 1.0,
                },
            );
        }

        // Initialize water renderer
        self.water_renderer = Some(WaterRenderer::new(WaterConfig::default()));

        // Initialize underwater effect
        self.underwater_effect = Some(super::underwater::UnderwaterEffect::new());
    }

    /// Updates all effects
    pub fn update(&mut self, dt: f32, camera_position: Vec3, camera_forward: Vec3) {
        if !self.global_settings.enabled {
            return;
        }

        // Update particle effects
        self.particle_manager.update(dt);

        // Update water
        if let Some(water) = &mut self.water_renderer {
            water.update(std::time::Duration::from_secs_f32(dt));
        }

        // Update underwater effect
        if let Some(underwater) = &mut self.underwater_effect {
            underwater.update(dt, camera_position, camera_forward);
        }
    }

    /// Renders all effects
    pub fn render(&self, renderer: &mut Renderer) {
        if !self.global_settings.enabled {
            return;
        }

        // Render particles
        self.particle_manager.render(renderer);

        // Render water (handled by water render pass)
        // Render underwater (handled by underwater render pass)
    }

    /// Applies post-process effects
    pub fn apply_post_process(&self, _renderer: &mut Renderer) {
        if !self.global_settings.enabled {
            return;
        }

        // Apply post-process effects based on settings
        if self.global_settings.bloom_enabled && self.is_effect_enabled(VisualEffectType::Bloom) {
            // Bloom is applied in the post-process pass
        }

        if self.global_settings.motion_blur_enabled
            && self.is_effect_enabled(VisualEffectType::MotionBlur)
        {
            // Motion blur is applied in the post-process pass
        }

        if self.global_settings.dof_enabled
            && self.is_effect_enabled(VisualEffectType::DepthOfField)
        {
            // DoF is applied in the post-process pass
        }

        if self.global_settings.hdr_enabled && self.is_effect_enabled(VisualEffectType::HDR) {
            // HDR is applied in the post-process pass
        }
    }

    /// Gets the particle manager
    pub fn particle_manager(&self) -> &ParticleEffectManager {
        &self.particle_manager
    }

    /// Gets mutable reference to the particle manager
    pub fn particle_manager_mut(&mut self) -> &mut ParticleEffectManager {
        &mut self.particle_manager
    }

    /// Gets the water renderer
    pub fn water_renderer(&self) -> Option<&WaterRenderer> {
        self.water_renderer.as_ref()
    }

    /// Gets mutable reference to the water renderer
    pub fn water_renderer_mut(&mut self) -> Option<&mut WaterRenderer> {
        self.water_renderer.as_mut()
    }

    /// Gets the underwater effect
    pub fn underwater_effect(&self) -> Option<&super::underwater::UnderwaterEffect> {
        self.underwater_effect.as_ref()
    }

    /// Gets mutable reference to the underwater effect
    pub fn underwater_effect_mut(&mut self) -> Option<&mut super::underwater::UnderwaterEffect> {
        self.underwater_effect.as_mut()
    }

    /// Gets post-process config
    pub fn post_process_config(&self) -> &PostProcessConfig {
        &self.post_process_config
    }

    /// Gets mutable reference to post-process config
    pub fn post_process_config_mut(&mut self) -> &mut PostProcessConfig {
        &mut self.post_process_config
    }

    /// Enables or disables a specific effect
    pub fn set_effect_enabled(&mut self, effect_type: VisualEffectType, enabled: bool) {
        if let Some(settings) = self.effect_settings.get_mut(&effect_type) {
            settings.enabled = enabled;
        }
    }

    /// Checks if a specific effect is enabled
    pub fn is_effect_enabled(&self, effect_type: VisualEffectType) -> bool {
        self.global_settings.enabled
            && self
                .effect_settings
                .get(&effect_type)
                .map_or(false, |s| s.enabled)
    }

    /// Sets effect quality
    pub fn set_effect_quality(&mut self, effect_type: VisualEffectType, quality: EffectQuality) {
        if let Some(settings) = self.effect_settings.get_mut(&effect_type) {
            settings.quality = quality;
        }
    }

    /// Sets effect intensity
    pub fn set_effect_intensity(&mut self, effect_type: VisualEffectType, intensity: f32) {
        if let Some(settings) = self.effect_settings.get_mut(&effect_type) {
            settings.intensity = intensity;
        }
    }

    /// Updates global settings
    pub fn update_global_settings(&mut self, settings: GlobalEffectSettings) {
        // Update particle manager settings
        self.particle_manager.set_enabled(settings.enabled);

        // Update post-process config
        self.post_process_config.bloom = settings.bloom_enabled;
        self.post_process_config.motion_blur = settings.motion_blur_enabled;
        self.post_process_config.depth_of_field = settings.dof_enabled;
        self.post_process_config.hdr = settings.hdr_enabled;

        self.global_settings = settings;
    }

    /// Gets global settings
    pub fn global_settings(&self) -> &GlobalEffectSettings {
        &self.global_settings
    }

    /// Toggles all effects on/off
    pub fn toggle_all_effects(&mut self, enabled: bool) {
        self.global_settings.enabled = enabled;
        for settings in self.effect_settings.values_mut() {
            settings.enabled = enabled;
        }
    }

    /// Resets all effects to default settings
    pub fn reset_to_defaults(&mut self) {
        self.global_settings = GlobalEffectSettings::default();
        self.initialize();
    }
}

impl Default for EffectManager {
    fn default() -> Self {
        // Requires a render context - constructed explicitly by callers
        panic!("EffectManager requires a render context")
    }
}
