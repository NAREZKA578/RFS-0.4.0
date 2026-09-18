//! Underwater Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Handles underwater rendering effects including distortion, color filtering, and caustics

use glam::{Vec3, Vec4};

/// Underwater effect configuration
#[derive(Debug, Clone)]
pub struct UnderwaterConfig {
    /// Color tint when underwater
    pub color_tint: Vec4,
    /// Fog density
    pub fog_density: f32,
    /// Fog color
    pub fog_color: Vec4,
    /// Distortion strength
    pub distortion_strength: f32,
    /// Distortion scale
    pub distortion_scale: f32,
    /// Distortion speed
    pub distortion_speed: f32,
    /// Caustics enabled
    pub caustics_enabled: bool,
    /// Caustics texture path
    pub caustics_texture: String,
    /// Caustics scale
    pub caustics_scale: f32,
    /// Caustics speed
    pub caustics_speed: f32,
    /// Light absorption
    pub light_absorption: Vec4,
    /// Depth threshold for underwater effect
    pub depth_threshold: f32,
    /// Transition smoothness
    pub transition_smoothness: f32,
}

impl Default for UnderwaterConfig {
    fn default() -> Self {
        Self {
            color_tint: Vec4::new(0.3, 0.5, 0.7, 0.3),
            fog_density: 0.05,
            fog_color: Vec4::new(0.1, 0.3, 0.6, 1.0),
            distortion_strength: 0.02,
            distortion_scale: 0.5,
            distortion_speed: 0.5,
            caustics_enabled: true,
            caustics_texture: String::from("textures/caustics.png"),
            caustics_scale: 0.3,
            caustics_speed: 0.2,
            light_absorption: Vec4::new(0.1, 0.3, 0.7, 0.0),
            depth_threshold: -0.5,
            transition_smoothness: 2.0,
        }
    }
}

/// Underwater effect
pub struct UnderwaterEffect {
    /// Configuration
    config: UnderwaterConfig,
    /// Current underwater state
    underwater: bool,
    /// Current depth
    depth: f32,
    /// Underwater factor (0.0 to 1.0)
    underwater_factor: f32,
    /// Time accumulator for animations
    time: f32,
    /// Previous camera position
    prev_camera_position: Vec3,
}

impl UnderwaterEffect {
    /// Creates a new underwater effect
    pub fn new() -> Self {
        Self {
            config: UnderwaterConfig::default(),
            underwater: false,
            depth: 0.0,
            underwater_factor: 0.0,
            time: 0.0,
            prev_camera_position: Vec3::ZERO,
        }
    }

    /// Creates a new underwater effect with custom configuration
    pub fn with_config(config: UnderwaterConfig) -> Self {
        Self {
            config,
            underwater: false,
            depth: 0.0,
            underwater_factor: 0.0,
            time: 0.0,
            prev_camera_position: Vec3::ZERO,
        }
    }

    /// Updates the underwater effect
    pub fn update(&mut self, dt: f32, camera_position: Vec3, _camera_forward: Vec3) {
        self.time += dt;

        // Calculate depth (Y coordinate)
        self.depth = camera_position.y;

        // Check if underwater
        let _was_underwater = self.underwater;
        self.underwater = self.depth < self.config.depth_threshold;

        // Calculate underwater factor with smooth transition
        if self.underwater {
            let depth_below = self.config.depth_threshold - self.depth;
            self.underwater_factor =
                (depth_below / (self.config.depth_threshold.abs() + 0.1)).clamp(0.0, 1.0);
        } else {
            let depth_above = self.depth - self.config.depth_threshold;
            self.underwater_factor =
                1.0 - (depth_above / (self.config.depth_threshold.abs() + 0.1)).clamp(0.0, 1.0);
        }

        // Smooth the transition
        let smooth_factor = self.config.transition_smoothness;
        let prev_factor = self.underwater_factor;
        self.underwater_factor =
            prev_factor + (self.underwater_factor - prev_factor) * dt * smooth_factor;

        self.prev_camera_position = camera_position;
    }

    /// Gets the underwater color tint
    pub fn color_tint(&self) -> Vec4 {
        if self.underwater_factor > 0.0 {
            Vec4::lerp(Vec4::ONE, self.config.color_tint, self.underwater_factor)
        } else {
            Vec4::ONE
        }
    }

    /// Gets the underwater fog color
    pub fn fog_color(&self) -> Vec4 {
        if self.underwater_factor > 0.0 {
            Vec4::lerp(
                Vec4::new(0.5, 0.7, 1.0, 0.0),
                self.config.fog_color,
                self.underwater_factor,
            )
        } else {
            Vec4::new(0.5, 0.7, 1.0, 0.0)
        }
    }

    /// Gets the underwater fog density
    pub fn fog_density(&self) -> f32 {
        self.config.fog_density * self.underwater_factor
    }

    /// Gets the distortion parameters
    pub fn distortion_params(&self) -> (f32, f32, f32) {
        if self.underwater_factor > 0.0 {
            (
                self.config.distortion_strength * self.underwater_factor,
                self.config.distortion_scale,
                self.config.distortion_speed * self.time,
            )
        } else {
            (0.0, 0.0, 0.0)
        }
    }

    /// Gets the caustics parameters
    pub fn caustics_params(&self) -> (bool, f32, f32, f32) {
        if self.config.caustics_enabled && self.underwater_factor > 0.0 {
            (
                true,
                self.config.caustics_scale,
                self.config.caustics_speed * self.time,
                self.underwater_factor,
            )
        } else {
            (false, 0.0, 0.0, 0.0)
        }
    }

    /// Gets the light absorption
    pub fn light_absorption(&self) -> Vec4 {
        if self.underwater_factor > 0.0 {
            Vec4::lerp(
                Vec4::ONE,
                self.config.light_absorption,
                self.underwater_factor,
            )
        } else {
            Vec4::ONE
        }
    }

    /// Is the camera underwater
    pub fn is_underwater(&self) -> bool {
        self.underwater
    }

    /// Gets the underwater factor (0.0 to 1.0)
    pub fn underwater_factor(&self) -> f32 {
        self.underwater_factor
    }

    /// Gets the current depth
    pub fn depth(&self) -> f32 {
        self.depth
    }

    /// Sets the configuration
    pub fn set_config(&mut self, config: UnderwaterConfig) {
        self.config = config;
    }

    /// Gets reference to the configuration
    pub fn config(&self) -> &UnderwaterConfig {
        &self.config
    }

    /// Gets mutable reference to the configuration
    pub fn config_mut(&mut self) -> &mut UnderwaterConfig {
        &mut self.config
    }

    /// Resets the effect
    pub fn reset(&mut self) {
        self.underwater = false;
        self.depth = 0.0;
        self.underwater_factor = 0.0;
        self.time = 0.0;
        self.prev_camera_position = Vec3::ZERO;
    }
}

impl Default for UnderwaterEffect {
    fn default() -> Self {
        Self::new()
    }
}
