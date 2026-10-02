//! Post-Processing Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod bloom;
pub mod dof;
pub mod fxaa;
pub mod hdr;
pub mod manager;
pub mod motion_blur;

pub use bloom::BloomEffect;
pub use dof::DepthOfFieldEffect;
pub use fxaa::FXAAEffect;
pub use hdr::HDREffect;
pub use manager::PostProcessManager;
pub use motion_blur::MotionBlurEffect;

// Re-export config types for convenience
pub use self::DepthOfFieldConfig as DofConfig;
pub use self::HDRConfig as HdrConfig;

/// Post-process configuration
#[derive(Debug, Clone)]
pub struct PostProcessConfig {
    pub bloom: bool,
    pub motion_blur: bool,
    pub depth_of_field: bool,
    pub hdr: bool,
    pub fxaa: bool,
    pub bloom_config: BloomConfig,
    pub motion_blur_config: MotionBlurConfig,
    pub dof_config: DepthOfFieldConfig,
    pub hdr_config: HDRConfig,
}

impl Default for PostProcessConfig {
    fn default() -> Self {
        Self {
            bloom: true,
            motion_blur: true,
            depth_of_field: false,
            hdr: true,
            fxaa: true,
            bloom_config: BloomConfig::default(),
            motion_blur_config: MotionBlurConfig::default(),
            dof_config: DepthOfFieldConfig::default(),
            hdr_config: HDRConfig::default(),
        }
    }
}

/// Bloom configuration
#[derive(Debug, Clone)]
pub struct BloomConfig {
    pub threshold: f32,
    pub intensity: f32,
    pub blur_passes: u32,
    pub blur_radius: f32,
}

impl Default for BloomConfig {
    fn default() -> Self {
        Self {
            threshold: 0.8,
            intensity: 0.5,
            blur_passes: 4,
            blur_radius: 2.0,
        }
    }
}

/// Motion blur configuration
#[derive(Debug, Clone)]
pub struct MotionBlurConfig {
    pub intensity: f32,
    pub max_velocity: f32,
    pub sample_count: u32,
}

impl Default for MotionBlurConfig {
    fn default() -> Self {
        Self {
            intensity: 0.5,
            max_velocity: 10.0,
            sample_count: 8,
        }
    }
}

/// Depth of field configuration
#[derive(Debug, Clone)]
pub struct DepthOfFieldConfig {
    pub focus_distance: f32,
    pub focus_range: f32,
    pub blur_radius: f32,
    pub sample_count: u32,
}

impl Default for DepthOfFieldConfig {
    fn default() -> Self {
        Self {
            focus_distance: 5.0,
            focus_range: 1.0,
            blur_radius: 2.0,
            sample_count: 8,
        }
    }
}

/// HDR configuration
#[derive(Debug, Clone)]
pub struct HDRConfig {
    pub exposure: f32,
    pub gamma: f32,
    pub tone_mapping: ToneMapping,
}

impl Default for HDRConfig {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            gamma: 2.2,
            tone_mapping: ToneMapping::ACES,
        }
    }
}

/// Tone mapping methods
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum ToneMapping {
    Linear,
    Reinhard,
    #[default]
    ACES,
    Filmic,
}

