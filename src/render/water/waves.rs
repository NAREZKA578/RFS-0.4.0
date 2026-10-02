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
        }
    }

    pub fn config(&self) -> &WaveConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut WaveConfig {
        &mut self.config
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
                * (dir.dot(pos) * wave.frequency + self.time * wave.speed).sin();
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
            height += wave.amplitude * wave.steepness.powi(2) * theta.sin();
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
            let qa = wave.amplitude * wave.steepness;

            let kx = wave.direction.x * wave.frequency * qa * c;
            let kz = wave.direction.z * wave.frequency * qa * c;
            let ky = wave.frequency * qa * s;

            normal += Vec3::new(-kx, ky, -kz);
        }

        normal.normalize()
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
