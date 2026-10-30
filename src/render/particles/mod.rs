//! Particles Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod emitter;
pub mod particle;
pub mod system;

pub use emitter::{EmitterShape, ParticleEmitter, ParticleEmitterConfig};
pub use particle::Particle;
pub use system::ParticleSystem;
