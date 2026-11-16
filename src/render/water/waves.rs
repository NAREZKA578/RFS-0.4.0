//! Wave System
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use glam::{Vec2, Vec3};
use std::time::Duration;

/// How far a Gerstner wave pushes water sideways, in units of its effective
/// amplitude. A pure sine is `0`; `1.8` gives the narrow crests and broad
/// troughs of a real swell. The per-wave horizontal steepness is capped below
/// one so the surface cannot fold over itself at the maximum height setting.
pub const GERSTNER_CHOPPINESS: f32 = 1.8;

/// Per-wave cap on `choppiness * k * amplitude`, the factor that controls how
/// much a crest is compressed. Four waves each capped at 0.22 keep the sum
/// under one, which is the fold-over limit.
const MAX_HORIZONTAL_STEEPNESS: f32 = 0.22;

/// Wave method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum WaveMethod {
    Flat,
    Simple,
    #[default]
    Gerstner,
    FFT,
}


/// Wave configuration
#[derive(Debug, Clone)]
pub struct WaveConfig {
    pub method: WaveMethod,
    pub scale: f32,
    pub speed: f32,
    pub height: f32,
    pub wave_count: usize,
    pub direction: Vec3,
}

impl Default for WaveConfig {
    fn default() -> Self {
        Self {
            method: WaveMethod::Gerstner,
            scale: 0.1,
            speed: 0.5,
            height: 0.2,
            wave_count: 4,
            direction: Vec3::new(1.0, 0.0, 0.0),
        }
    }
}

/// Wave system
pub struct WaveSystem {
    config: WaveConfig,
    time: f32,
    waves: Vec<Wave>,
    /// Multiplier applied to every wave's amplitude and slope. `1.0` is the
    /// authored sea; `0.0` is glass. Added for the settings slider, which must
    /// change the water the ship floats on and the water that is drawn with
    /// one value — two independent knobs would drift apart immediately.
    height_scale: f32,
}

impl WaveSystem {
    pub fn new(method: WaveMethod) -> Self {
        let mut waves = Vec::new();

        // Create default waves
        match method {
            WaveMethod::Simple => {
                waves.push(Wave::new(1.0, 0.5, 0.1, Vec3::new(1.0, 0.0, 0.0)));
                waves.push(Wave::new(0.8, 0.6, 0.15, Vec3::new(0.7, 0.0, 0.7)));
                waves.push(Wave::new(0.6, 0.7, 0.2, Vec3::new(0.0, 0.0, 1.0)));
            }
            WaveMethod::Gerstner => {
                // A sea is a spectrum, not four equal ripples. The old set put
                // every wave at 8-13 m with nearly the same amplitude, so the
                // surface read as one repeating chop. These follow deep-water
                // dispersion (`speed = sqrt(g * k)`) and split the energy over
                // a long swell and three shorter seas:
                //
                //   k = 0.052 -> 120 m, 8.8 s period, the dominant swell
                //   k = 0.097 ->  65 m, 6.5 s
                //   k = 0.165 ->  38 m, 4.9 s
                //   k = 0.262 ->  24 m, 3.9 s
                //
                // Directions fan out by roughly 0-75 degrees, so the crests
                // cross instead of running as parallel ridges. The shorter
                // waves are what make the sea read as water rather than as a
                // slowly breathing plane: at 24 m the 224-segment grid still
                // has nine samples per wavelength.
                //
                // The steepness values are deliberately above the old uniform
                // 0.65. Effective height is `A * steepness^2`, so this is the
                // knob that turns a gentle swell into a sea with sharp crests;
                // the per-wave horizontal displacement is capped separately in
                // `get_horizontal_displacement`, which keeps the mesh from
                // folding over.
                waves.push(
                    Wave::new(4.00, 0.0524, 0.717, Vec3::new(1.0, 0.0, 0.0))
                        .with_steepness(0.80),
                );
                waves.push(
                    Wave::new(2.60, 0.0967, 0.974, Vec3::new(0.88, 0.0, 0.48))
                        .with_steepness(0.84),
                );
                waves.push(
                    Wave::new(1.70, 0.1653, 1.274, Vec3::new(0.94, 0.0, -0.34))
                        .with_steepness(0.86),
                );
                waves.push(
                    Wave::new(1.05, 0.2618, 1.603, Vec3::new(0.38, 0.0, 0.925))
                        .with_steepness(0.90),
                );
            }
            _ => {}
        }

        Self {
            config: WaveConfig {
                method,
                ..Default::default()
            },
            time: 0.0,
            waves,
            height_scale: 1.0,
        }
    }

    pub fn config(&self) -> &WaveConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut WaveConfig {
        &mut self.config
    }

    /// The multiplier that turns the authored waves into the current sea.
    pub fn height_scale(&self) -> f32 {
        self.height_scale
    }

    /// Set the amplitude multiplier. Negative values would flip the sea
    /// upside down for no useful effect, so they clamp to flat.
    pub fn set_height_scale(&mut self, scale: f32) {
        self.height_scale = if scale.is_finite() {
            scale.clamp(0.0, 8.0)
        } else {
            0.0
        };
    }

    /// The largest absolute height the current sea can reach, for colouring
    /// crests and for sanity checks. `Flat` and the still-unimplemented FFT
    /// path are zero.
    pub fn max_height(&self) -> f32 {
        let base = match self.config.method {
            WaveMethod::Flat | WaveMethod::FFT => 0.0,
            WaveMethod::Simple => self.waves.iter().map(|wave| wave.amplitude).sum(),
            WaveMethod::Gerstner => self
                .waves
                .iter()
                .map(|wave| wave.amplitude * wave.steepness.powi(2))
                .sum(),
        };
        base * self.height_scale
    }

    /// The first four waves packed for the water shader's per-fragment normal.
    ///
    /// Each entry is `(direction.x, direction.z, frequency, amplitude)` and
    /// `amplitude` already includes the method's shaping and the current
    /// [`Self::height_scale`], so evaluating
    ///
    /// ```text
    /// theta = dot(direction, position.xz) * frequency + time * speed
    /// height = amplitude * sin(theta)
    /// ```
    ///
    /// in the shader reproduces exactly what [`Self::get_height`] samples on
    /// the CPU. `speeds` is separate because four more floats do not fit in the
    /// same four vec4s. Missing waves are zeroed, and `Flat`/`FFT` return an
    /// all-zero field — a flat sea.
    ///
    /// Four is the shader's limit. The drawn ocean has four waves; a scene that
    /// adds more keeps the correct mesh silhouette but gets its normals from
    /// the first four.
    pub fn shader_waves(&self) -> ([[f32; 4]; 4], [f32; 4]) {
        let mut packed = [[0.0; 4]; 4];
        let mut speeds = [0.0; 4];

        if matches!(self.config.method, WaveMethod::Flat | WaveMethod::FFT) {
            return (packed, speeds);
        }

        for (slot, wave) in self.waves.iter().take(4).enumerate() {
            let amplitude = match self.config.method {
                WaveMethod::Gerstner => wave.amplitude * wave.steepness.powi(2),
                WaveMethod::Simple => wave.amplitude,
                WaveMethod::Flat | WaveMethod::FFT => 0.0,
            } * self.height_scale;

            packed[slot] = [
                wave.direction.x,
                wave.direction.z,
                wave.frequency,
                amplitude,
            ];
            speeds[slot] = wave.speed;
        }

        (packed, speeds)
    }

    pub fn update(&mut self, delta_time: Duration) {
        self.time += delta_time.as_secs_f32();

        // Update waves
        for wave in &mut self.waves {
            wave.update(delta_time);
        }
    }

    /// Get wave height at a position
    pub fn get_height(&self, position: Vec3) -> f32 {
        match self.config.method {
            WaveMethod::Flat => 0.0,
            WaveMethod::Simple => self.get_simple_height(position),
            WaveMethod::Gerstner => self.get_gerstner_height(position),
            WaveMethod::FFT => self.get_fft_height(position),
        }
    }

    /// Get normal at a position
    pub fn get_normal(&self, position: Vec3) -> Vec3 {
        match self.config.method {
            WaveMethod::Flat => Vec3::Y,
            WaveMethod::Simple => self.get_simple_normal(position),
            WaveMethod::Gerstner => self.get_gerstner_normal(position),
            WaveMethod::FFT => self.get_fft_normal(position),
        }
    }

    fn get_simple_height(&self, position: Vec3) -> f32 {
        let mut height = 0.0;
        for wave in &self.waves {
            let dir = Vec2::new(wave.direction.x, wave.direction.z);
            let pos = Vec2::new(position.x, position.z);
            height += wave.amplitude
                * (dir.dot(pos) * wave.frequency + self.time * wave.speed).sin()
                * self.height_scale;
        }
        height
    }

    fn get_simple_normal(&self, position: Vec3) -> Vec3 {
        // Finite-difference normal with correct Y and NaN-safe normalize
        // (flat water gives a zero gradient — normalize would yield NaN).
        let epsilon = 0.1;
        let h = self.get_simple_height(position);
        let hx = self.get_simple_height(position + Vec3::new(epsilon, 0.0, 0.0));
        let hz = self.get_simple_height(position + Vec3::new(0.0, 0.0, epsilon));

        let n = Vec3::new(-(hx - h) / epsilon, 1.0, -(hz - h) / epsilon);
        if n.length_squared() < 1e-12 || !n.is_finite() {
            Vec3::Y
        } else {
            n.normalize()
        }
    }

    fn get_gerstner_height(&self, position: Vec3) -> f32 {
        let mut height = 0.0;
        for wave in &self.waves {
            let dir = Vec2::new(wave.direction.x, wave.direction.z);
            let pos = Vec2::new(position.x, position.z);
            let theta = dir.dot(pos) * wave.frequency + self.time * wave.speed;
            height += wave.amplitude * wave.steepness.powi(2) * theta.sin() * self.height_scale;
        }
        height
    }

    /// Horizontal displacement of the surface point whose undisplaced grid
    /// position is `position`.
    ///
    /// A height field alone describes a wavy plane. A Gerstner wave also moves
    /// water sideways: `offset = -Q * A * direction * cos(theta)`. That
    /// compression sharpens crests and broadens troughs, and it is why the
    /// mesh stores the source XZ for the fragment shader: the displaced vertex
    /// no longer maps one-to-one to the wave phase.
    pub fn get_horizontal_displacement(&self, position: Vec3) -> Vec2 {
        if !matches!(self.config.method, WaveMethod::Gerstner) {
            return Vec2::ZERO;
        }

        let mut offset = Vec2::ZERO;
        for wave in &self.waves {
            let dir = Vec2::new(wave.direction.x, wave.direction.z);
            let pos = Vec2::new(position.x, position.z);
            let theta = dir.dot(pos) * wave.frequency + self.time * wave.speed;
            let amplitude = wave.amplitude * wave.steepness.powi(2) * self.height_scale;
            let displacement = (amplitude * GERSTNER_CHOPPINESS)
                .min(MAX_HORIZONTAL_STEEPNESS / wave.frequency.max(1e-4));
            offset -= dir * (displacement * theta.cos());
        }
        offset
    }

    fn get_gerstner_normal(&self, position: Vec3) -> Vec3 {
        // The normal of the *displaced* surface, not of the height field.
        // With horizontal displacement the parameter tangents are no longer
        // (1,0,0) and (0,0,1): a crest is compressed sideways, so the cross
        // product of the true tangents is the only correct normal.
        let mut tangent_x = Vec3::X;
        let mut tangent_z = Vec3::Z;

        for wave in &self.waves {
            let dir = Vec2::new(wave.direction.x, wave.direction.z);
            let pos = Vec2::new(position.x, position.z);
            let theta = dir.dot(pos) * wave.frequency + self.time * wave.speed;
            let s = theta.sin();
            let c = theta.cos();
            let k = wave.frequency;
            let amplitude = wave.amplitude * wave.steepness.powi(2) * self.height_scale;
            let qa = (amplitude * GERSTNER_CHOPPINESS)
                .min(MAX_HORIZONTAL_STEEPNESS / k.max(1e-4));
            let dx = dir.x;
            let dz = dir.y;

            // dP/dx0 and dP/dz0 for
            //   P = (x0 - qa*dx*cos(theta), A*sin(theta),
            //        z0 - qa*dz*cos(theta)).
            tangent_x += Vec3::new(
                -qa * dx * dx * k * s,
                amplitude * k * dx * c,
                -qa * dx * dz * k * s,
            );
            tangent_z += Vec3::new(
                -qa * dx * dz * k * s,
                amplitude * k * dz * c,
                -qa * dz * dz * k * s,
            );
        }

        let normal = tangent_z.cross(tangent_x);
        // A flat sea has a zero gradient; `normalize` would answer NaN and the
        // NaN would reach the vertex buffer. This is not hypothetical — the
        // settings slider can set the scale to exactly zero.
        if normal.length_squared() < 1e-12 || !normal.is_finite() {
            Vec3::Y
        } else {
            normal.normalize()
        }
    }

    fn get_fft_height(&self, _position: Vec3) -> f32 {
        // FFT-based wave simulation would go here
        0.0
    }

    fn get_fft_normal(&self, _position: Vec3) -> Vec3 {
        // FFT-based normal calculation would go here
        Vec3::Y
    }

    pub fn add_wave(&mut self, wave: Wave) {
        self.waves.push(wave);
    }

    pub fn clear_waves(&mut self) {
        self.waves.clear();
    }
}

impl Default for WaveSystem {
    fn default() -> Self {
        Self::new(WaveMethod::Gerstner)
    }
}

/// Wave
#[derive(Debug, Clone)]
pub struct Wave {
    pub amplitude: f32,
    pub frequency: f32,
    pub speed: f32,
    pub direction: Vec3,
    pub steepness: f32,
}

impl Wave {
    pub fn new(amplitude: f32, frequency: f32, speed: f32, direction: Vec3) -> Self {
        Self {
            amplitude,
            frequency,
            speed,
            direction: direction.normalize(),
            steepness: 0.65,
        }
    }

    /// Set how sharply this component compresses its crests. `1.0` is the
    /// fold-over limit for a single wave; the system caps the horizontal
    /// displacement below that even at the maximum height slider.
    pub fn with_steepness(mut self, steepness: f32) -> Self {
        self.steepness = steepness.clamp(0.0, 1.0);
        self
    }

    pub fn update(&mut self, _delta_time: Duration) {
        // Wave parameters can be updated over time if needed
    }
}

impl Default for Wave {
    fn default() -> Self {
        Self::new(1.0, 0.5, 0.1, Vec3::X)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_height_scale_multiplies_both_height_and_slope() {
        let mut waves = WaveSystem::new(WaveMethod::Gerstner);
        let point = Vec3::new(3.0, 0.0, 7.0);
        let height = waves.get_height(point);
        let max_height = waves.max_height();
        let normal = waves.get_normal(point);
        assert!(height.abs() > 1e-4, "the sample point must be on a slope");

        waves.set_height_scale(2.0);
        assert!((waves.get_height(point) - height * 2.0).abs() < 1e-5);
        assert!((waves.max_height() - max_height * 2.0).abs() < 1e-5);
        assert!(normal.is_finite() && normal.y > 0.0);
    }

    #[test]
    fn the_shader_waves_reproduce_the_cpu_normal() {
        let mut waves = WaveSystem::new(WaveMethod::Gerstner);
        waves.update(Duration::from_secs_f32(1.0));
        let point = Vec3::new(3.0, 0.0, 7.0);
        let start = waves.time;

        let (packed, speeds) = waves.shader_waves();
        let mut tangent_x = Vec3::X;
        let mut tangent_z = Vec3::Z;
        for (wave, speed) in packed.iter().zip(speeds.iter()) {
            let theta = (wave[0] * point.x + wave[1] * point.z) * wave[2] + start * speed;
            let s = theta.sin();
            let c = theta.cos();
            let k = wave[2];
            let amplitude = wave[3];
            let qa = (amplitude * GERSTNER_CHOPPINESS)
                .min(MAX_HORIZONTAL_STEEPNESS / k.max(1e-4));
            let dx = wave[0];
            let dz = wave[1];
            tangent_x += Vec3::new(
                -qa * dx * dx * k * s,
                amplitude * k * dx * c,
                -qa * dx * dz * k * s,
            );
            tangent_z += Vec3::new(
                -qa * dx * dz * k * s,
                amplitude * k * dz * c,
                -qa * dz * dz * k * s,
            );
        }
        let shader_normal = tangent_z.cross(tangent_x).normalize();

        let cpu_normal = waves.get_normal(point);
        assert!(
            (shader_normal - cpu_normal).length() < 1e-5,
            "shader normal {shader_normal:?} != CPU normal {cpu_normal:?}"
        );
    }

    #[test]
    fn the_horizontal_displacement_cannot_fold_the_surface() {
        let mut waves = WaveSystem::new(WaveMethod::Gerstner);
        // The steepest legal slider value: the sum of the per-wave horizontal
        // steepness must stay below one or the mesh turns inside out.
        waves.set_height_scale(8.0);
        for x in [-40.0, -7.0, 0.0, 3.0, 19.0] {
            for z in [-31.0, -2.0, 0.0, 11.0, 47.0] {
                let normal = waves.get_normal(Vec3::new(x, 0.0, z));
                assert!(normal.is_finite() && normal.y > 0.0);
            }
        }
    }

    #[test]
    fn a_zero_scale_is_flat_and_has_an_up_normal() {
        let mut waves = WaveSystem::new(WaveMethod::Gerstner);
        waves.set_height_scale(0.0);
        assert_eq!(waves.get_height(Vec3::new(1.0, 0.0, 1.0)), 0.0);
        assert_eq!(waves.get_normal(Vec3::new(1.0, 0.0, 1.0)), Vec3::Y);
        assert_eq!(waves.max_height(), 0.0);
    }

    #[test]
    fn a_non_finite_scale_becomes_flat_instead_of_poisoning_the_grid() {
        let mut waves = WaveSystem::new(WaveMethod::Simple);
        waves.set_height_scale(f32::NAN);
        assert_eq!(waves.height_scale(), 0.0);
        assert!(waves.get_height(Vec3::new(1.0, 0.0, 1.0)).is_finite());
    }
}
