//! Water Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod interaction;
pub mod surface;
pub mod waves;

pub use interaction::WaterInteraction;
pub use surface::WaterSurface;
pub use waves::{WaveConfig, WaveMethod, WaveSystem};

/// Water configuration
#[derive(Debug, Clone)]
pub struct WaterConfig {
    pub wave_method: WaveMethod,
    pub wave_scale: f32,
    pub wave_speed: f32,
    pub wave_height: f32,
    pub color: [f32; 4],
    pub deep_color: [f32; 4],
    pub shallow_color: [f32; 4],
    pub depth: f32,
    pub clarity: f32,
    pub tessellation_factor: f32,
    pub reflection_enabled: bool,
    pub refraction_enabled: bool,
    pub foam_enabled: bool,
    pub foam_threshold: f32,
    pub foam_intensity: f32,
    pub fresnel_factor: f32,
    pub fresnel_bias: f32,
    pub fresnel_power: f32,
}

impl Default for WaterConfig {
    fn default() -> Self {
        Self {
            wave_method: WaveMethod::Gerstner,
            wave_scale: 0.1,
            wave_speed: 0.5,
            wave_height: 0.2,
            color: [0.0, 0.1, 0.3, 0.7],
            deep_color: [0.0, 0.05, 0.2, 0.7],
            shallow_color: [0.0, 0.2, 0.4, 0.6],
            depth: 100.0,
            clarity: 0.7,
            tessellation_factor: 8.0,
            reflection_enabled: true,
            refraction_enabled: true,
            foam_enabled: true,
            foam_threshold: 1.0,
            foam_intensity: 0.5,
            fresnel_factor: 0.02,
            fresnel_bias: 0.1,
            fresnel_power: 5.0,
        }
    }
}

/// Water renderer
pub struct WaterRenderer {
    pub config: WaterConfig,
    pub surface: WaterSurface,
    pub waves: WaveSystem,
    pub interaction: WaterInteraction,
}

impl WaterRenderer {
    pub fn new(config: WaterConfig) -> Self {
        let wave_method = config.wave_method;
        Self {
            config,
            surface: WaterSurface::new(),
            waves: WaveSystem::new(wave_method),
            interaction: WaterInteraction::new(),
        }
    }

    pub fn update(&mut self, delta_time: std::time::Duration) {
        self.waves.update(delta_time);
    }

    pub fn render(&self) {
        // Render water
        // This would be called by the water render pass
    }
}

impl Default for WaterRenderer {
    fn default() -> Self {
        Self::new(WaterConfig::default())
    }
}
