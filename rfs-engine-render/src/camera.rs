//! Камера движка: yaw/pitch + перспектива на `glam`.
//!
//! Донор: `RFS-0.3/src/graphics/camera.rs` (там же уже был guard aspect от нуля —
//! перенесён сюда как есть).

use glam::{Mat4, Vec3};

/// Перспективная камера от первого/третьего лица.
#[derive(Debug, Clone)]
pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_y_rad: f32,
    pub near: f32,
    pub far: f32,
    aspect: f32,
}

impl Camera {
    pub fn new(position: Vec3, aspect: f32) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            fov_y_rad: 75.0_f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            aspect: if aspect > 0.0 { aspect } else { 16.0 / 9.0 },
        }
    }

    /// Защита от нулевой высоты окна (донор: `camera.rs:54-55`).
    pub fn resize(&mut self, width: f32, height: f32) {
        self.aspect = if height > 0.0 {
            (width / height).clamp(0.1, 10.0)
        } else {
            1.0
        };
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            -self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        )
    }

    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.position, self.position + self.forward(), Vec3::Y)
    }

    pub fn proj_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov_y_rad, self.aspect, self.near, self.far)
    }

    pub fn view_proj(&self) -> Mat4 {
        self.proj_matrix() * self.view_matrix()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_guards_zero_height() {
        let mut c = Camera::new(Vec3::ZERO, 1.0);
        c.resize(800.0, 0.0);
        assert!((c.view_proj().determinant()).is_finite());
    }
}
