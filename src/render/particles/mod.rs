//! Particles Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod effect_manager;
pub mod effects;
pub mod emitter;
pub mod particle;
pub mod system;

pub use effect_manager::{EffectHandle, EffectQuality, EffectSettings, ParticleEffectManager};
pub use effects::{
    BloodEffect, BloodEffectConfig, DustEffect, DustEffectConfig, ExplosionEffect,
    ExplosionEffectConfig, FireEffect, FireEffectConfig, MagicEffect, MagicEffectConfig,
    ParticleEffect, ParticleEffectType, SmokeEffect, SmokeEffectConfig, SparksEffect,
    SparksEffectConfig, SplashEffect, SplashEffectConfig,
};
pub use emitter::{EmitterShape, ParticleEmitter, ParticleEmitterConfig};
pub use particle::Particle;
pub use system::ParticleSystem;
