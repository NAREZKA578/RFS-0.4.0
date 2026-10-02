//! Camera Controller
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Handles camera movement and control.

use super::camera::{Camera, CameraMode, OrthographicCamera, PerspectiveCamera};
use glam::{Quat, Vec3};
use std::time::Duration;

/// Camera controller
pub struct CameraController {
    /// Camera being controlled
    camera: Box<dyn Camera>,
    /// Movement speed
    pub move_speed: f32,
    /// Rotation speed
    pub rotate_speed: f32,
    /// Zoom speed (for orthographic cameras)
    pub zoom_speed: f32,
    /// Current rotation (for FPS-style control)
    pub yaw: f32,
    pub pitch: f32,
    /// Mouse sensitivity
    pub mouse_sensitivity: f32,
    /// Invert Y axis
    pub invert_y: bool,
    /// Movement state
    movement: CameraMovement,
    /// Is the camera being controlled
    pub enabled: bool,
    /// Camera control mode
    pub mode: CameraMode,
    /// Orbital target (for orbital mode)
    pub orbital_target: Vec3,
    /// Orbital distance (for orbital mode)
    pub orbital_distance: f32,
}

/// Camera movement state
#[derive(Debug, Clone, Default)]
pub struct CameraMovement {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub rotate_left: bool,
    pub rotate_right: bool,
    pub rotate_up: bool,
    pub rotate_down: bool,
}

impl CameraController {
    pub fn new(camera: Box<dyn Camera>) -> Self {
        Self {
            camera,
            move_speed: 5.0,
            rotate_speed: 1.5,
            zoom_speed: 1.0,
            yaw: 0.0,
            pitch: 0.0,
            mouse_sensitivity: 0.1,
            invert_y: false,
            movement: CameraMovement::default(),
            enabled: true,
            mode: CameraMode::FirstPerson,
            orbital_target: Vec3::ZERO,
            orbital_distance: 10.0,
        }
    }

    pub fn with_camera(mut self, camera: Box<dyn Camera>) -> Self {
        self.camera = camera;
        self
    }

    pub fn with_move_speed(mut self, speed: f32) -> Self {
        self.move_speed = speed;
        self
    }

    pub fn with_rotate_speed(mut self, speed: f32) -> Self {
        self.rotate_speed = speed;
        self
    }

    pub fn with_zoom_speed(mut self, speed: f32) -> Self {
        self.zoom_speed = speed;
        self
    }

    pub fn with_mouse_sensitivity(mut self, sensitivity: f32) -> Self {
        self.mouse_sensitivity = sensitivity;
        self
    }

    pub fn with_invert_y(mut self, invert: bool) -> Self {
        self.invert_y = invert;
        self
    }

    /// Get the camera
    pub fn camera(&self) -> &dyn Camera {
        self.camera.as_ref()
    }

    /// Get mutable camera
    pub fn camera_mut(&mut self) -> &mut Box<dyn Camera> {
        &mut self.camera
    }

    /// Update the camera based on input and time
    pub fn update(&mut self, delta_time: Duration) {
        if !self.enabled {
            return;
        }

        let delta_seconds = delta_time.as_secs_f32();

        // Handle rotation
        if self.movement.rotate_left {
            self.yaw -= self.rotate_speed * delta_seconds;
        }
        if self.movement.rotate_right {
            self.yaw += self.rotate_speed * delta_seconds;
        }
        if self.movement.rotate_up {
            self.pitch +=
                self.rotate_speed * delta_seconds * if self.invert_y { -1.0 } else { 1.0 };
        }
        if self.movement.rotate_down {
            self.pitch -=
                self.rotate_speed * delta_seconds * if self.invert_y { -1.0 } else { 1.0 };
        }

        // Clamp pitch
        self.pitch = self.pitch.clamp(-1.5, 1.5); // ~86 degrees up/down

        // Calculate rotation quaternion
        let rotation = Quat::from_euler(glam::EulerRot::YXZ, self.yaw, self.pitch, 0.0);

        // Apply rotation to camera
        self.camera.set_rotation(rotation);

        // Handle movement
        let forward = self.camera.forward();
        let right = self.camera.right();
        let up = self.camera.up();

        let mut move_dir = Vec3::ZERO;

        if self.movement.forward {
            move_dir += forward;
        }
        if self.movement.backward {
            move_dir -= forward;
        }
        if self.movement.left {
            move_dir -= right;
        }
        if self.movement.right {
            move_dir += right;
        }
        if self.movement.up {
            move_dir += up;
        }
        if self.movement.down {
            move_dir -= up;
        }

        // Normalize and scale
        if move_dir != Vec3::ZERO {
            move_dir = move_dir.normalize();
            let position = self.camera.position();
            self.camera
                .set_position(position + move_dir * self.move_speed * delta_seconds);
        }
    }

    /// Handle mouse input
    pub fn handle_mouse(&mut self, dx: f32, dy: f32) {
        if !self.enabled {
            return;
        }

        self.yaw -= dx * self.mouse_sensitivity;
        self.pitch += dy * self.mouse_sensitivity * if self.invert_y { -1.0 } else { 1.0 };

        // Clamp pitch
        self.pitch = self.pitch.clamp(-1.5, 1.5);

        // Apply rotation
        let rotation = Quat::from_euler(glam::EulerRot::YXZ, self.yaw, self.pitch, 0.0);
        self.camera.set_rotation(rotation);
    }

    /// Handle keyboard input
    pub fn handle_keyboard(&mut self, key: CameraKey, pressed: bool) {
        if !self.enabled {
            return;
        }

        match key {
            CameraKey::Forward => self.movement.forward = pressed,
            CameraKey::Backward => self.movement.backward = pressed,
            CameraKey::Left => self.movement.left = pressed,
            CameraKey::Right => self.movement.right = pressed,
            CameraKey::Up => self.movement.up = pressed,
            CameraKey::Down => self.movement.down = pressed,
            CameraKey::RotateLeft => self.movement.rotate_left = pressed,
            CameraKey::RotateRight => self.movement.rotate_right = pressed,
            CameraKey::RotateUp => self.movement.rotate_up = pressed,
            CameraKey::RotateDown => self.movement.rotate_down = pressed,
        }
    }

    /// Handle mouse wheel (for zoom)
    pub fn handle_mouse_wheel(&mut self, delta: f32) {
        if !self.enabled {
            return;
        }

        // For orthographic cameras, adjust the viewport
        if let Some(camera) = self
            .camera
            .as_any_mut()
            .downcast_mut::<OrthographicCamera>()
        {
            let current_size = camera.right - camera.left;
            let new_size = (current_size - delta * self.zoom_speed * 0.1).max(0.1);
            let center = (camera.left + camera.right) / 2.0;
            camera.left = center - new_size / 2.0;
            camera.right = center + new_size / 2.0;

            let current_height = camera.top - camera.bottom;
            let new_height = (current_height - delta * self.zoom_speed * 0.1).max(0.1);
            let center_y = (camera.bottom + camera.top) / 2.0;
            camera.bottom = center_y - new_height / 2.0;
            camera.top = center_y + new_height / 2.0;
        }
    }

    /// Enable the controller
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the controller
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Reset the controller
    pub fn reset(&mut self) {
        self.yaw = 0.0;
        self.pitch = 0.0;
        self.movement = CameraMovement::default();
    }

    /// Set camera mode
    pub fn set_mode(&mut self, mode: CameraMode) {
        self.mode = mode;
    }

    /// Get camera mode
    pub fn mode(&self) -> CameraMode {
        self.mode
    }

    /// Set orbital target
    pub fn set_orbital_target(&mut self, target: Vec3) {
        self.orbital_target = target;
    }

    /// Set orbital distance
    pub fn set_orbital_distance(&mut self, distance: f32) {
        self.orbital_distance = distance.max(0.1);
    }

    /// Update camera based on mode
    pub fn update_with_mode(&mut self, delta_time: Duration) {
        match self.mode {
            CameraMode::FirstPerson => self.update(delta_time),
            CameraMode::ThirdPerson => self.update_third_person(delta_time),
            CameraMode::Orbital => self.update_orbital(delta_time),
        }
    }

    fn update_third_person(&mut self, delta_time: Duration) {
        self.update(delta_time);
    }

    fn update_orbital(&mut self, _delta_time: Duration) {
        let forward = Quat::from_euler(glam::EulerRot::YXZ, self.yaw, self.pitch, 0.0) * Vec3::NEG_Z;
        let position = self.orbital_target - forward * self.orbital_distance;
        self.camera.set_position(position);
        let rotation = Quat::from_rotation_arc(Vec3::Z, forward);
        self.camera.set_rotation(rotation);
    }
}

impl Default for CameraController {
    fn default() -> Self {
        Self::new(Box::new(PerspectiveCamera::default()))
    }
}

/// Camera keys
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraKey {
    Forward,
    Backward,
    Left,
    Right,
    Up,
    Down,
    RotateLeft,
    RotateRight,
    RotateUp,
    RotateDown,
}

/// Camera controller builder
pub struct CameraControllerBuilder {
    controller: CameraController,
}

impl CameraControllerBuilder {
    pub fn new(camera: Box<dyn Camera>) -> Self {
        Self {
            controller: CameraController::new(camera),
        }
    }

    pub fn with_move_speed(mut self, speed: f32) -> Self {
        self.controller.move_speed = speed;
        self
    }

    pub fn with_rotate_speed(mut self, speed: f32) -> Self {
        self.controller.rotate_speed = speed;
        self
    }

    pub fn with_zoom_speed(mut self, speed: f32) -> Self {
        self.controller.zoom_speed = speed;
        self
    }

    pub fn with_mouse_sensitivity(mut self, sensitivity: f32) -> Self {
        self.controller.mouse_sensitivity = sensitivity;
        self
    }

    pub fn with_invert_y(mut self, invert: bool) -> Self {
        self.controller.invert_y = invert;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.controller.enabled = enabled;
        self
    }

    pub fn build(self) -> CameraController {
        self.controller
    }
}
