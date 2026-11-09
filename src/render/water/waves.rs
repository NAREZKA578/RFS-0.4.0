//! Wave System
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use glam::{Vec2, Vec3};
use std::time::Duration;

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
                waves.push(Wave::new(1.0, 0.5, 0.1, Vec3::new(1.0, 0.0, 0.0)));
                waves.push(Wave::new(0.8, 0.6, 0.15, Vec3::new(0.5, 0.0, 0.5)));
                waves.push(Wave::new(0.6, 0.7, 0.2, Vec3::new(0.0, 0.0, 1.0)));
                waves.push(Wave::new(0.4, 0.8, 0.25, Vec3::new(-0.5, 0.0, 0.5)));
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

    fn get_gerstner_normal(&self, position: Vec3) -> Vec3 {
        let mut normal = Vec3::new(0.0, 1.0, 0.0);

        for wave in &self.waves {
            let dir = Vec2::new(wave.direction.x, wave.direction.z);
            let pos = Vec2::new(position.x, position.z);
            let theta = dir.dot(pos) * wave.frequency + self.time * wave.speed;
            let c = theta.cos();
            let s = theta.sin();
            let qa = wave.amplitude * wave.steepness * self.height_scale;

            let kx = wave.direction.x * wave.frequency * qa * c;
            let kz = wave.direction.z * wave.frequency * qa * c;
            let ky = wave.frequency * qa * s;

            normal += Vec3::new(-kx, ky, -kz);
        }

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
            steepness: 0.5,
        }
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
