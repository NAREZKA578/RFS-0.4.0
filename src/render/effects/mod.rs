//! Effects Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Contains all visual effects systems

pub mod manager;
pub mod screen_space;
pub mod underwater;

pub use manager::{
    EffectManager, EffectQuality, EffectSettings, GlobalEffectSettings, VisualEffectType,
};
pub use screen_space::{SSAOConfig, SSRConfig, ScreenSpaceEffect};
pub use underwater::{UnderwaterConfig, UnderwaterEffect};
