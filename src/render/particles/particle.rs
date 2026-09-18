//! Particle
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use glam::{Vec2, Vec3, Vec4};

/// Particle struct
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub size: Vec2,
    pub color: Vec4,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub rotation: f32,
    pub rotation_speed: f32,
    pub texture_index: u32,
    pub emitter_id: u32,
}

impl Particle {
    pub fn new() -> Self {
        Self {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            size: Vec2::ONE,
            color: Vec4::ONE,
            lifetime: 0.0,
            max_lifetime: 1.0,
            rotation: 0.0,
            rotation_speed: 0.0,
            texture_index: 0,
            emitter_id: 0,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.lifetime < self.max_lifetime
    }

    pub fn update(&mut self, delta_time: std::time::Duration) {
        let delta_seconds = delta_time.as_secs_f32();

        self.position += self.velocity * delta_seconds;
        self.lifetime += delta_seconds;
        self.rotation += self.rotation_speed * delta_seconds;
    }
}

impl Default for Particle {
    fn default() -> Self {
        Self::new()
    }
}
