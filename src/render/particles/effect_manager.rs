//! Particle Effect Manager
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Manages all particle effects in the scene

use super::effects::{
    BloodEffect, DustEffect, ExplosionEffect, FireEffect, MagicEffect, SmokeEffect, SparksEffect,
    SplashEffect,
};
use super::{ParticleEffect, ParticleSystem};
use crate::render::core::{RenderContext, Renderer};
use std::collections::HashMap;

/// Effect handle for referencing effects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectHandle(u64);

impl EffectHandle {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn id(&self) -> u64 {
        self.0
    }
}

/// Particle effect manager
pub struct ParticleEffectManager {
    /// All active effects
    effects: HashMap<EffectHandle, Box<dyn ParticleEffect>>,
    /// Particle system for GPU-based rendering
    particle_system: ParticleSystem,
    /// Next effect ID
    next_id: u64,
    /// Maximum number of effects
    _max_effects: usize,
    /// Global effect settings
    settings: EffectSettings,
}

/// Global effect settings
#[derive(Debug, Clone)]
pub struct EffectSettings {
    pub enabled: bool,
    pub quality: EffectQuality,
    pub max_particles: u32,
    pub particle_quality: u32,
}

/// Global effect quality
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            quality: EffectQuality::Medium,
            max_particles: 10000,
            particle_quality: 1, // 0 = low, 1 = high
        }
    }
}

impl ParticleEffectManager {
    /// Creates a new particle effect manager
    pub fn new(context: &RenderContext, max_particles: u32) -> Self {
        Self {
            effects: HashMap::new(),
            particle_system: ParticleSystem::new(context.device.clone(), max_particles),
            next_id: 0,
            _max_effects: 1000,
            settings: EffectSettings::default(),
        }
    }

    /// Creates a new smoke effect
    pub fn create_smoke_effect(
        &mut self,
        config: super::effects::SmokeEffectConfig,
    ) -> EffectHandle {
        let handle = EffectHandle::new(self.next_id);
        self.next_id += 1;

        let effect = SmokeEffect::new(config);
        self.effects.insert(handle, Box::new(effect));
        handle
    }

    /// Creates a new fire effect
    pub fn create_fire_effect(&mut self, config: super::effects::FireEffectConfig) -> EffectHandle {
        let handle = EffectHandle::new(self.next_id);
        self.next_id += 1;

        let effect = FireEffect::new(config);
        self.effects.insert(handle, Box::new(effect));
        handle
    }

    /// Creates a new splash effect
    pub fn create_splash_effect(
        &mut self,
        config: super::effects::SplashEffectConfig,
    ) -> EffectHandle {
        let handle = EffectHandle::new(self.next_id);
        self.next_id += 1;

        let effect = SplashEffect::new(config);
        self.effects.insert(handle, Box::new(effect));
        handle
    }

    /// Creates a new explosion effect
    pub fn create_explosion_effect(
        &mut self,
        config: super::effects::ExplosionEffectConfig,
    ) -> EffectHandle {
        let handle = EffectHandle::new(self.next_id);
        self.next_id += 1;

        let effect = ExplosionEffect::new(config);
        self.effects.insert(handle, Box::new(effect));
        handle
    }

    /// Creates a new sparks effect
    pub fn create_sparks_effect(
        &mut self,
        config: super::effects::SparksEffectConfig,
    ) -> EffectHandle {
        let handle = EffectHandle::new(self.next_id);
        self.next_id += 1;

        let effect = SparksEffect::new(config);
        self.effects.insert(handle, Box::new(effect));
        handle
    }

    /// Creates a new dust effect
    pub fn create_dust_effect(&mut self, config: super::effects::DustEffectConfig) -> EffectHandle {
        let handle = EffectHandle::new(self.next_id);
        self.next_id += 1;

        let effect = DustEffect::new(config);
        self.effects.insert(handle, Box::new(effect));
        handle
    }

    /// Creates a new blood effect
    pub fn create_blood_effect(
        &mut self,
        config: super::effects::BloodEffectConfig,
    ) -> EffectHandle {
        let handle = EffectHandle::new(self.next_id);
        self.next_id += 1;

        let effect = BloodEffect::new(config);
        self.effects.insert(handle, Box::new(effect));
        handle
    }

    /// Creates a new magic effect
    pub fn create_magic_effect(
        &mut self,
        config: super::effects::MagicEffectConfig,
    ) -> EffectHandle {
        let handle = EffectHandle::new(self.next_id);
        self.next_id += 1;

        let effect = MagicEffect::new(config);
        self.effects.insert(handle, Box::new(effect));
        handle
    }

    /// Gets a reference to an effect
    pub fn get_effect(&self, handle: EffectHandle) -> Option<&dyn ParticleEffect> {
        self.effects.get(&handle).map(|e| e.as_ref())
    }

    /// Gets a mutable reference to an effect
    pub fn get_effect_mut(&mut self, handle: EffectHandle) -> Option<&mut (dyn ParticleEffect + 'static)> {
        self.effects.get_mut(&handle).map(|e| e.as_mut())
    }

    /// Removes an effect
    pub fn remove_effect(&mut self, handle: EffectHandle) -> bool {
        self.effects.remove(&handle).is_some()
    }

    /// Updates all effects
    pub fn update(&mut self, dt: f32) {
        if !self.settings.enabled {
            return;
        }

        for effect in self.effects.values_mut() {
            effect.update(dt);
        }
    }

    /// Renders all effects
    pub fn render(&self, renderer: &mut Renderer) {
        if !self.settings.enabled {
            return;
        }

        for effect in self.effects.values() {
            effect.render(renderer);
        }
    }

    /// Updates effect settings
    pub fn update_settings(&mut self, settings: EffectSettings) {
        self.settings = settings;

        // Update all effects with new quality
        for _effect in self.effects.values_mut() {
            // Each effect type would have its own quality setting
            // This is a simplified approach
        }
    }

    /// Clears all effects
    pub fn clear(&mut self) {
        self.effects.clear();
        self.next_id = 0;
    }

    /// Gets the number of active effects
    pub fn effect_count(&self) -> usize {
        self.effects.len()
    }

    /// Gets the particle system
    pub fn particle_system(&self) -> &ParticleSystem {
        &self.particle_system
    }

    /// Gets mutable reference to the particle system
    pub fn particle_system_mut(&mut self) -> &mut ParticleSystem {
        &mut self.particle_system
    }

    /// Sets global enabled state
    pub fn set_enabled(&mut self, enabled: bool) {
        self.settings.enabled = enabled;
    }

    /// Is global effects enabled
    pub fn is_enabled(&self) -> bool {
        self.settings.enabled
    }

    /// Sets effect quality
    pub fn set_quality(&mut self, quality: EffectQuality) {
        self.settings.quality = quality;
    }

    /// Gets effect quality
    pub fn quality(&self) -> EffectQuality {
        self.settings.quality
    }
}

impl Default for ParticleEffectManager {
    fn default() -> Self {
        // Requires a render context - constructed explicitly by callers
        panic!("ParticleEffectManager requires a render context")
    }
}

/// Helper trait for downcasting effect references
pub trait EffectDowncast {
    fn as_smoke(&self) -> Option<&SmokeEffect>;
    fn as_smoke_mut(&mut self) -> Option<&mut SmokeEffect>;
    fn as_fire(&self) -> Option<&FireEffect>;
    fn as_fire_mut(&mut self) -> Option<&mut FireEffect>;
    fn as_splash(&self) -> Option<&SplashEffect>;
    fn as_splash_mut(&mut self) -> Option<&mut SplashEffect>;
    fn as_explosion(&self) -> Option<&ExplosionEffect>;
    fn as_explosion_mut(&mut self) -> Option<&mut ExplosionEffect>;
    fn as_sparks(&self) -> Option<&SparksEffect>;
    fn as_sparks_mut(&mut self) -> Option<&mut SparksEffect>;
    fn as_dust(&self) -> Option<&DustEffect>;
    fn as_dust_mut(&mut self) -> Option<&mut DustEffect>;
    fn as_blood(&self) -> Option<&BloodEffect>;
    fn as_blood_mut(&mut self) -> Option<&mut BloodEffect>;
    fn as_magic(&self) -> Option<&MagicEffect>;
    fn as_magic_mut(&mut self) -> Option<&mut MagicEffect>;
}

// Implement downcast trait for Box<dyn ParticleEffect>
// This would require dynamic casting which is complex in Rust
// For now, we'll leave this as a marker for future implementation
