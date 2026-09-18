//! Graphics Settings
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Central graphics settings management

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Graphics settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphicsSettings {
    // Display settings
    #[serde(skip)]
    pub resolution: Vec2,
    pub fullscreen: bool,
    pub vsync: bool,
    pub refresh_rate: u32,
    pub display_mode: DisplayMode,

    // Quality settings
    pub quality_preset: QualityPreset,
    pub texture_quality: TextureQuality,
    pub shadow_quality: ShadowQuality,
    pub particle_quality: u32,
    pub lod_distance: f32,
    pub max_lod_level: u32,

    // Rendering settings
    pub render_api: RenderApi,
    pub msaa_samples: MsaaSamples,
    pub anisotropy_level: u32,

    // Lighting settings
    pub max_lights: u32,
    #[serde(skip)]
    pub shadow_resolution: Vec2,
    pub shadow_cascade_count: u32,
    pub shadow_distance: f32,
    pub shadow_bias: f32,
    pub shadow_softness: f32,

    // Post-process settings
    pub post_process: PostProcessSettings,

    // Effect settings
    pub effects: EffectSettings,

    // Water settings
    pub water: WaterSettings,

    // Particle settings
    pub particles: ParticleSettings,

    // Performance settings
    pub performance: PerformanceSettings,

    // Advanced settings
    pub advanced: AdvancedSettings,
}

/// Display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    Windowed,
    Fullscreen,
    Borderless,
}

/// Quality preset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityPreset {
    Low,
    Medium,
    High,
    Ultra,
    Custom,
}

/// Texture quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureQuality {
    Low,
    Medium,
    High,
    Ultra,
}

/// Shadow quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowQuality {
    Off,
    Low,
    Medium,
    High,
    Ultra,
}

/// Render API
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderApi {
    Auto,
    Vulkan,
    Direct3D12,
    Direct3D11,
    OpenGL,
}

/// MSAA samples
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MsaaSamples {
    X1,
    X2,
    X4,
    X8,
    X16,
}

/// Post-process settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostProcessSettings {
    pub bloom: BloomSettings,
    pub motion_blur: MotionBlurSettings,
    pub depth_of_field: DepthOfFieldSettings,
    pub hdr: HdrSettings,
    pub fxaa: FxaaSettings,
}

/// Bloom settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BloomSettings {
    pub enabled: bool,
    pub intensity: f32,
    pub threshold: f32,
    pub radius: f32,
    pub iterations: u32,
}

/// Motion blur settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotionBlurSettings {
    pub enabled: bool,
    pub intensity: f32,
    pub sample_count: u32,
    pub max_velocity: f32,
}

/// Depth of field settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthOfFieldSettings {
    pub enabled: bool,
    pub focus_distance: f32,
    pub focus_range: f32,
    pub blur_radius: f32,
    pub blur_iterations: u32,
}

/// HDR settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdrSettings {
    pub enabled: bool,
    pub exposure: f32,
    pub gamma: f32,
    pub tone_mapping: ToneMappingMode,
}

/// Tone mapping mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToneMappingMode {
    Linear,
    Reinhard,
    ACES,
    Filmic,
}

/// FXAA settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxaaSettings {
    pub enabled: bool,
    pub quality: FxaaQuality,
}

/// FXAA quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FxaaQuality {
    Low,
    Medium,
    High,
    Ultra,
}

/// Effect settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectSettings {
    pub ssao: bool,
    pub ssr: bool,
    pub god_rays: bool,
    pub lens_flare: bool,
    pub volumetric_fog: bool,
    pub underwater: bool,
}

/// Water settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaterSettings {
    pub quality: WaterQuality,
    pub wave_method: WaveMethod,
    pub tessellation: bool,
    pub reflections: bool,
    pub refractions: bool,
    pub foam: bool,
    pub caustics: bool,
}

/// Water quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaterQuality {
    Low,
    Medium,
    High,
    Ultra,
}

/// Wave method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaveMethod {
    Flat,
    Simple,
    Gerstner,
    FFT,
}

/// Particle settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleSettings {
    pub max_particles: u32,
    pub quality: ParticleQuality,
    pub sort_mode: ParticleSortMode,
}

/// Particle quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticleQuality {
    Low,
    Medium,
    High,
    Ultra,
}

/// Particle sort mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticleSortMode {
    None,
    BackToFront,
    FrontToBack,
}

/// Performance settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSettings {
    pub fps_limit: u32,
    pub frame_budget_ms: f32,
    pub gpu_memory_limit_mb: u32,
    pub cpu_threads: u32,
    pub async_compute: bool,
}

/// Advanced settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedSettings {
    pub ray_tracing: bool,
    pub ray_tracing_quality: RayTracingQuality,
    pub mesh_shading: bool,
    pub variable_rate_shading: bool,
    pub bindless_resources: bool,
    pub debug_overlay: bool,
    pub stats_overlay: bool,
}

/// Ray tracing quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RayTracingQuality {
    Low,
    #[default]
    Medium,
    High,
    Ultra,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            resolution: Vec2::new(1920.0, 1080.0),
            fullscreen: false,
            vsync: true,
            refresh_rate: 60,
            display_mode: DisplayMode::Windowed,

            quality_preset: QualityPreset::High,
            texture_quality: TextureQuality::High,
            shadow_quality: ShadowQuality::High,
            particle_quality: 1,
            lod_distance: 50.0,
            max_lod_level: 3,

            render_api: RenderApi::Auto,
            msaa_samples: MsaaSamples::X4,
            anisotropy_level: 16,

            max_lights: 32,
            shadow_resolution: Vec2::new(2048.0, 2048.0),
            shadow_cascade_count: 4,
            shadow_distance: 100.0,
            shadow_bias: 0.001,
            shadow_softness: 1.0,

            post_process: PostProcessSettings::default(),
            effects: EffectSettings::default(),
            water: WaterSettings::default(),
            particles: ParticleSettings::default(),
            performance: PerformanceSettings::default(),
            advanced: AdvancedSettings::default(),
        }
    }
}

impl Default for PostProcessSettings {
    fn default() -> Self {
        Self {
            bloom: BloomSettings::default(),
            motion_blur: MotionBlurSettings::default(),
            depth_of_field: DepthOfFieldSettings::default(),
            hdr: HdrSettings::default(),
            fxaa: FxaaSettings::default(),
        }
    }
}

impl Default for BloomSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            intensity: 0.5,
            threshold: 0.8,
            radius: 0.5,
            iterations: 4,
        }
    }
}

impl Default for MotionBlurSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            intensity: 0.5,
            sample_count: 8,
            max_velocity: 100.0,
        }
    }
}

impl Default for DepthOfFieldSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            focus_distance: 10.0,
            focus_range: 5.0,
            blur_radius: 0.5,
            blur_iterations: 2,
        }
    }
}

impl Default for HdrSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            exposure: 1.0,
            gamma: 2.2,
            tone_mapping: ToneMappingMode::ACES,
        }
    }
}

impl Default for FxaaSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            quality: FxaaQuality::Medium,
        }
    }
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            ssao: true,
            ssr: true,
            god_rays: true,
            lens_flare: true,
            volumetric_fog: true,
            underwater: true,
        }
    }
}

impl Default for WaterSettings {
    fn default() -> Self {
        Self {
            quality: WaterQuality::Medium,
            wave_method: WaveMethod::Gerstner,
            tessellation: true,
            reflections: true,
            refractions: true,
            foam: true,
            caustics: true,
        }
    }
}

impl Default for ParticleSettings {
    fn default() -> Self {
        Self {
            max_particles: 10000,
            quality: ParticleQuality::High,
            sort_mode: ParticleSortMode::BackToFront,
        }
    }
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            fps_limit: 144,
            frame_budget_ms: 16.67, // ~60 FPS
            gpu_memory_limit_mb: 4096,
            cpu_threads: 0, // 0 = auto
            async_compute: true,
        }
    }
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            ray_tracing: true,
            ray_tracing_quality: RayTracingQuality::Medium,
            mesh_shading: false,
            variable_rate_shading: false,
            bindless_resources: false,
            debug_overlay: false,
            stats_overlay: true,
        }
    }
}

/// Graphics settings manager
pub struct GraphicsSettingsManager {
    current: GraphicsSettings,
    default: GraphicsSettings,
    changed: bool,
}

impl GraphicsSettingsManager {
    pub fn new() -> Self {
        Self {
            current: GraphicsSettings::default(),
            default: GraphicsSettings::default(),
            changed: false,
        }
    }

    pub fn current(&self) -> &GraphicsSettings {
        &self.current
    }

    pub fn current_mut(&mut self) -> &mut GraphicsSettings {
        self.changed = true;
        &mut self.current
    }

    pub fn apply(&mut self) {
        // Apply settings to the rendering system
        // This would be called after changing settings
        self.changed = false;
    }

    pub fn has_changes(&self) -> bool {
        self.changed
    }

    pub fn reset(&mut self) {
        self.current = self.default.clone();
        self.changed = true;
    }

    pub fn save(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let json = serde_json::to_vec(&self.current)?;
        Ok(json)
    }

    pub fn load(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        self.current = serde_json::from_slice(data)?;
        self.changed = true;
        Ok(())
    }

    pub fn set_preset(&mut self, preset: QualityPreset) {
        match preset {
            QualityPreset::Low => self.apply_low_preset(),
            QualityPreset::Medium => self.apply_medium_preset(),
            QualityPreset::High => self.apply_high_preset(),
            QualityPreset::Ultra => self.apply_ultra_preset(),
            QualityPreset::Custom => {}
        }
        self.current.quality_preset = preset;
        self.changed = true;
    }

    fn apply_low_preset(&mut self) {
        self.current.shadow_quality = ShadowQuality::Low;
        self.current.texture_quality = TextureQuality::Low;
        self.current.shadow_resolution = Vec2::new(1024.0, 1024.0);
        self.current.shadow_cascade_count = 2;
        self.current.msaa_samples = MsaaSamples::X2;
        self.current.anisotropy_level = 2;
        self.current.max_lights = 16;
        self.current.post_process.bloom.iterations = 2;
        self.current.post_process.motion_blur.sample_count = 4;
        self.current.water.quality = WaterQuality::Low;
        self.current.water.tessellation = false;
        self.current.particles.max_particles = 5000;
        self.current.particles.quality = ParticleQuality::Low;
        self.current.advanced.ray_tracing = false;
    }

    fn apply_medium_preset(&mut self) {
        self.current.shadow_quality = ShadowQuality::Medium;
        self.current.texture_quality = TextureQuality::Medium;
        self.current.shadow_resolution = Vec2::new(1536.0, 1536.0);
        self.current.shadow_cascade_count = 3;
        self.current.msaa_samples = MsaaSamples::X4;
        self.current.anisotropy_level = 8;
        self.current.max_lights = 32;
        self.current.post_process.bloom.iterations = 3;
        self.current.post_process.motion_blur.sample_count = 6;
        self.current.water.quality = WaterQuality::Medium;
        self.current.water.tessellation = true;
        self.current.particles.max_particles = 7500;
        self.current.particles.quality = ParticleQuality::Medium;
        self.current.advanced.ray_tracing = true;
    }

    fn apply_high_preset(&mut self) {
        self.current.shadow_quality = ShadowQuality::High;
        self.current.texture_quality = TextureQuality::High;
        self.current.shadow_resolution = Vec2::new(2048.0, 2048.0);
        self.current.shadow_cascade_count = 4;
        self.current.msaa_samples = MsaaSamples::X4;
        self.current.anisotropy_level = 16;
        self.current.max_lights = 64;
        self.current.post_process.bloom.iterations = 4;
        self.current.post_process.motion_blur.sample_count = 8;
        self.current.water.quality = WaterQuality::High;
        self.current.water.tessellation = true;
        self.current.particles.max_particles = 10000;
        self.current.particles.quality = ParticleQuality::High;
        self.current.advanced.ray_tracing = true;
        self.current.advanced.ray_tracing_quality = RayTracingQuality::Medium;
    }

    fn apply_ultra_preset(&mut self) {
        self.current.shadow_quality = ShadowQuality::Ultra;
        self.current.texture_quality = TextureQuality::Ultra;
        self.current.shadow_resolution = Vec2::new(4096.0, 4096.0);
        self.current.shadow_cascade_count = 4;
        self.current.msaa_samples = MsaaSamples::X8;
        self.current.anisotropy_level = 16;
        self.current.max_lights = 128;
        self.current.post_process.bloom.iterations = 5;
        self.current.post_process.motion_blur.sample_count = 12;
        self.current.water.quality = WaterQuality::Ultra;
        self.current.water.tessellation = true;
        self.current.particles.max_particles = 15000;
        self.current.particles.quality = ParticleQuality::Ultra;
        self.current.advanced.ray_tracing = true;
        self.current.advanced.ray_tracing_quality = RayTracingQuality::High;
        self.current.advanced.mesh_shading = true;
    }
}

impl Default for GraphicsSettingsManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Renderer configuration (detected from hardware)
#[derive(Debug, Clone)]
pub struct RendererConfig {
    pub backend: RenderApi,
    pub device_name: String,
    pub vendor: String,
    pub memory: u64,
    pub supports_ray_tracing: bool,
    pub supports_mesh_shading: bool,
    pub supports_bindless: bool,
    pub max_texture_size: u32,
    pub max_compute_work_group_size: [u32; 3],
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            backend: RenderApi::Auto,
            device_name: String::new(),
            vendor: String::new(),
            memory: 0,
            supports_ray_tracing: false,
            supports_mesh_shading: false,
            supports_bindless: false,
            max_texture_size: 4096,
            max_compute_work_group_size: [64, 64, 64],
        }
    }
}

/// Renderer settings (user configurable)
#[derive(Debug, Clone)]
pub struct RendererSettings {
    pub ray_tracing: RayTracingSettings,
    pub shadow_quality: ShadowQuality,
}

/// Ray tracing settings
#[derive(Debug, Clone)]
pub struct RayTracingSettings {
    pub enabled: bool,
    pub quality: RayTracingQuality,
}

impl Default for RayTracingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            quality: RayTracingQuality::Medium,
        }
    }
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self {
            ray_tracing: RayTracingSettings::default(),
            shadow_quality: ShadowQuality::High,
        }
    }
}
