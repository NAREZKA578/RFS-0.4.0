//! Particle Effects Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod blood;
pub mod dust;
pub mod explosion;
pub mod fire;
pub mod magic;
pub mod smoke;
pub mod sparks;
pub mod splash;

pub use blood::{BloodEffect, BloodEffectConfig};
pub use dust::{DustEffect, DustEffectConfig};
pub use explosion::{ExplosionEffect, ExplosionEffectConfig};
pub use fire::{FireEffect, FireEffectConfig};
pub use magic::{MagicEffect, MagicEffectConfig};
pub use smoke::{SmokeEffect, SmokeEffectConfig};
pub use sparks::{SparksEffect, SparksEffectConfig};
pub use splash::{SplashEffect, SplashEffectConfig};

/// Particle effect type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParticleEffectType {
    Smoke,
    Fire,
    Splash,
    Explosion,
    Sparks,
    Dust,
    Blood,
    Magic,
}

/// Particle effect trait
pub trait ParticleEffect {
    fn update(&mut self, dt: f32);
    fn render(&self, renderer: &mut crate::render::core::Renderer);
    fn is_enabled(&self) -> bool;
    fn set_enabled(&mut self, enabled: bool);
    fn effect_type(&self) -> ParticleEffectType;
}
