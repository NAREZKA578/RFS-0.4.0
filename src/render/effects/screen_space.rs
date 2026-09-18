//! Screen Space Effects
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Screen space ambient occlusion and screen space reflections

use glam::Vec3;

/// SSAO (Screen Space Ambient Occlusion) configuration
#[derive(Debug, Clone)]
pub struct SSAOConfig {
    /// Number of samples
    pub sample_count: u32,
    /// Sample radius
    pub radius: f32,
    /// Bias
    pub bias: f32,
    /// Power
    pub power: f32,
    /// Noise texture scale
    pub noise_scale: f32,
    /// Blur enabled
    pub blur_enabled: bool,
    /// Blur radius
    pub blur_radius: u32,
    /// AO color
    pub ao_color: Vec3,
    /// Intensity
    pub intensity: f32,
}

impl Default for SSAOConfig {
    fn default() -> Self {
        Self {
            sample_count: 64,
            radius: 0.5,
            bias: 0.025,
            power: 1.0,
            noise_scale: 1.0,
            blur_enabled: true,
            blur_radius: 2,
            ao_color: Vec3::new(0.0, 0.0, 0.0),
            intensity: 1.0,
        }
    }
}

/// SSR (Screen Space Reflections) configuration
#[derive(Debug, Clone)]
pub struct SSRConfig {
    /// Maximum ray march steps
    pub max_steps: u32,
    /// Minimum ray march steps
    pub min_steps: u32,
    /// Step size
    pub step_size: f32,
    /// Maximum distance
    pub max_distance: f32,
    /// Roughness threshold
    pub roughness_threshold: f32,
    /// Depth bias
    pub depth_bias: f32,
    /// Reflection fade
    pub fade: f32,
    /// Intensity
    pub intensity: f32,
    /// Use temporal accumulation
    pub temporal: bool,
}

impl Default for SSRConfig {
    fn default() -> Self {
        Self {
            max_steps: 100,
            min_steps: 20,
            step_size: 0.05,
            max_distance: 100.0,
            roughness_threshold: 0.3,
            depth_bias: 0.01,
            fade: 0.5,
            intensity: 1.0,
            temporal: true,
        }
    }
}

/// Screen space effect trait
pub trait ScreenSpaceEffect {
    /// Updates the effect
    fn update(&mut self, dt: f32);

    /// Gets the effect configuration
    fn config(&self) -> &dyn std::any::Any;

    /// Sets the effect configuration
    fn set_config(&mut self, config: &dyn std::any::Any);

    /// Is the effect enabled
    fn is_enabled(&self) -> bool;

    /// Enables or disables the effect
    fn set_enabled(&mut self, enabled: bool);
}

/// SSAO effect
pub struct SSAOEffect {
    config: SSAOConfig,
    enabled: bool,
}

impl SSAOEffect {
    pub fn new(config: SSAOConfig) -> Self {
        Self {
            config,
            enabled: true,
        }
    }

    pub fn kernel(&self) -> Vec<Vec3> {
        // Generate SSAO kernel samples
        let mut kernel = Vec::with_capacity(self.config.sample_count as usize);

        

        for i in 0..self.config.sample_count {
            // Generate random point on hemisphere
            let sample = self.hemisphere_sample(i);

            // Scale sample
            let scale = i as f32 / self.config.sample_count as f32;
            let scaled_sample = sample * (0.1 + 0.9 * scale);

            kernel.push(scaled_sample);
        }

        kernel
    }

    fn hemisphere_sample(&self, index: u32) -> Vec3 {
        // Simple pseudo-random hemisphere sampling
        // In a real implementation, this would use proper random number generation
        let i = index as f32;
        let seed_x = (i * 0.1031).fract();
        let seed_y = (i * 0.2031).fract();
        let seed_z = (i * 0.3031).fract();

        // Convert to hemisphere coordinates
        let x = seed_x * 2.0 - 1.0;
        let y = seed_y * 2.0 - 1.0;
        let z = seed_z * 2.0 - 1.0;

        let len = (x * x + y * y + z * z).sqrt();
        if len > 0.0 {
            Vec3::new(x, y, z) / len
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        }
    }
}

impl ScreenSpaceEffect for SSAOEffect {
    fn update(&mut self, _dt: f32) {
        // SSAO doesn't need per-frame updates
    }

    fn config(&self) -> &dyn std::any::Any {
        &self.config
    }

    fn set_config(&mut self, config: &dyn std::any::Any) {
        if let Some(c) = config.downcast_ref::<SSAOConfig>() {
            self.config = c.clone();
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

/// SSR effect
pub struct SSREffect {
    config: SSRConfig,
    enabled: bool,
}

impl SSREffect {
    pub fn new(config: SSRConfig) -> Self {
        Self {
            config,
            enabled: true,
        }
    }
}

impl ScreenSpaceEffect for SSREffect {
    fn update(&mut self, _dt: f32) {
        // SSR doesn't need per-frame updates
    }

    fn config(&self) -> &dyn std::any::Any {
        &self.config
    }

    fn set_config(&mut self, config: &dyn std::any::Any) {
        if let Some(c) = config.downcast_ref::<SSRConfig>() {
            self.config = c.clone();
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}
