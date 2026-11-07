//! Camera
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use glam::{Mat3, Mat4, Quat, Vec3};
use std::any::Any;
use std::fmt::Debug;

/// The rotation that looks from `position` to `target`, with the caller's
/// `up` fixing the roll.
///
/// Bug №212: both cameras used to compute the orthogonalised `up` and then
/// throw it away (`let _ = up_safe;`, `_up`), so the roll was always the
/// minimal turn from +Z and a camera facing straight down produced the same
/// picture whatever up the caller asked for. The basis here is the real
/// orthonormal triad: local +Z along `forward`, local +Y along the caller's
/// up projected onto the plane orthogonal to the look direction, local +X
/// completing the right-handed set (`up × forward`). Degenerate directions
/// fall back instead of producing NaN.
fn look_rotation(position: Vec3, target: Vec3, up: Vec3) -> Quat {
    let dir = target - position;
    let forward = if !dir.is_finite() || dir.length_squared() < 1e-12 {
        Vec3::NEG_Z
    } else {
        dir.normalize()
    };
    let up_n = if !up.is_finite() || up.length_squared() < 1e-12 {
        Vec3::Y
    } else {
        up.normalize()
    };
    let up_picked = if forward.cross(up_n).length_squared() < 1e-12 {
        // `up` is (anti)parallel to the look direction: any axis not parallel
        // to it fixes the roll instead.
        if forward.y.abs() < 0.99 {
            Vec3::Y
        } else {
            Vec3::X
        }
    } else {
        up_n
    };
    // Gram–Schmidt: the caller's up projected onto the plane ⊥ forward.
    let true_up = (up_picked - forward * up_picked.dot(forward)).normalize();
    let right = true_up.cross(forward);
    Quat::from_mat3(&Mat3::from_cols(right, true_up, forward))
}

/// Camera trait
pub trait Camera: Send + Sync + Debug {
    /// Get view matrix
    fn view_matrix(&self) -> Mat4;

    /// Get projection matrix
    fn projection_matrix(&self) -> Mat4;

    /// Get view-projection matrix
    fn view_projection_matrix(&self) -> Mat4 {
        self.projection_matrix() * self.view_matrix()
    }

    /// Get position
    fn position(&self) -> Vec3;

    /// Get forward direction
    fn forward(&self) -> Vec3;

    /// Get up direction
    fn up(&self) -> Vec3;

    /// Get right direction (NaN-safe: parallel forward/up yields fallback axis).
    fn right(&self) -> Vec3 {
        let c = self.forward().cross(self.up());
        if c.length_squared() < 1e-12 || !c.is_finite() {
            Vec3::X
        } else {
            c.normalize()
        }
    }

    /// Cast to `Any` (for downcasting)
    fn as_any(&self) -> &dyn Any;

    /// Cast to `Any` mutably (for downcasting)
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Get near plane
    fn near_plane(&self) -> f32;

    /// Get far plane
    fn far_plane(&self) -> f32;

    /// Get aspect ratio
    fn aspect_ratio(&self) -> f32;

    /// Get field of view (in radians)
    fn fov(&self) -> f32;

    /// Set position
    fn set_position(&mut self, position: Vec3);

    /// Set rotation
    fn set_rotation(&mut self, rotation: Quat);

    /// Get frustum planes
    fn frustum_planes(&self) -> [Mat4; 6];
}

/// Perspective camera
#[derive(Debug, Clone)]
pub struct PerspectiveCamera {
    pub position: Vec3,
    pub rotation: Quat,
    pub fov: f32,
    pub aspect_ratio: f32,
    pub near_plane: f32,
    pub far_plane: f32,
    /// Viewport
    pub viewport: (f32, f32, f32, f32),
}

impl PerspectiveCamera {
    pub fn new(position: Vec3, fov: f32, aspect_ratio: f32, near: f32, far: f32) -> Self {
        Self {
            position,
            rotation: Quat::IDENTITY,
            fov,
            aspect_ratio,
            near_plane: near,
            far_plane: far,
            viewport: (0.0, 0.0, aspect_ratio * near, near),
        }
    }

    pub fn look_at(
        position: Vec3,
        target: Vec3,
        up: Vec3,
        fov: f32,
        aspect_ratio: f32,
        near: f32,
        far: f32,
    ) -> Self {
        Self::new(position, fov, aspect_ratio, near, far)
            .with_rotation(look_rotation(position, target, up))
    }

    pub fn with_position(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    pub fn with_rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn with_fov(mut self, fov: f32) -> Self {
        self.fov = fov;
        self
    }

    pub fn with_aspect_ratio(mut self, aspect_ratio: f32) -> Self {
        self.aspect_ratio = aspect_ratio;
        self
    }

    pub fn with_near_plane(mut self, near: f32) -> Self {
        self.near_plane = near;
        self
    }

    pub fn with_far_plane(mut self, far: f32) -> Self {
        self.far_plane = far;
        self
    }

    pub fn with_viewport(mut self, x: f32, y: f32, width: f32, height: f32) -> Self {
        self.viewport = (x, y, width, height);
        // Guard divide-by-zero: degenerate height must not produce inf aspect.
        self.aspect_ratio = if height.abs() > 1e-6 && width.is_finite() && height.is_finite() {
            (width / height).clamp(0.1, 10.0)
        } else {
            16.0 / 9.0
        };
        self
    }
}

impl Camera for PerspectiveCamera {
    fn view_matrix(&self) -> Mat4 {
        let forward = self.rotation * Vec3::Z;
        let up = self.rotation * Vec3::Y;
        Mat4::look_at_rh(self.position, self.position + forward, up)
    }

    fn projection_matrix(&self) -> Mat4 {
        Mat4::perspective_rh_gl(self.fov, self.aspect_ratio, self.near_plane, self.far_plane)
    }

    fn position(&self) -> Vec3 {
        self.position
    }

    fn forward(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }

    fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    fn near_plane(&self) -> f32 {
        self.near_plane
    }

    fn far_plane(&self) -> f32 {
        self.far_plane
    }

    fn aspect_ratio(&self) -> f32 {
        self.aspect_ratio
    }

    fn fov(&self) -> f32 {
        self.fov
    }

    fn set_position(&mut self, position: Vec3) {
        self.position = position;
    }

    fn set_rotation(&mut self, rotation: Quat) {
        self.rotation = rotation;
    }

    fn frustum_planes(&self) -> [Mat4; 6] {
        let vp = self.view_projection_matrix();
        let planes = [
            vp.row(3) + vp.row(0),
            vp.row(3) - vp.row(0),
            vp.row(3) + vp.row(1),
            vp.row(3) - vp.row(1),
            vp.row(3) + vp.row(2),
            vp.row(3) - vp.row(2),
        ];
        let mut result = [Mat4::IDENTITY; 6];
        for (i, plane) in planes.iter().enumerate() {
            let len = plane.truncate().length();
            if len > 1e-6 {
                let n = *plane / len;
                result[i] = Mat4::from_cols(n, n, n, n);
            }
        }
        result
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Default for PerspectiveCamera {
    fn default() -> Self {
        Self::new(
            Vec3::new(0.0, 0.0, 5.0),
            std::f32::consts::PI / 3.0, // 60 degrees
            16.0 / 9.0,
            0.1,
            1000.0,
        )
    }
}

/// Orthographic camera
#[derive(Debug, Clone)]
pub struct OrthographicCamera {
    pub position: Vec3,
    pub rotation: Quat,
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub top: f32,
    pub near_plane: f32,
    pub far_plane: f32,
    /// Viewport
    pub viewport: (f32, f32, f32, f32),
}

impl OrthographicCamera {
    pub fn new(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            left,
            right,
            bottom,
            top,
            near_plane: near,
            far_plane: far,
            viewport: (left, bottom, right - left, top - bottom),
        }
    }

    pub fn look_at(position: Vec3, target: Vec3, up: Vec3, size: f32, near: f32, far: f32) -> Self {
        Self::new(-size, size, -size, size, near, far)
            .with_position(position)
            .with_rotation(look_rotation(position, target, up))
    }

    pub fn with_position(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    pub fn with_rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn with_viewport(mut self, x: f32, y: f32, width: f32, height: f32) -> Self {
        self.viewport = (x, y, width, height);
        self.left = x;
        self.right = x + width;
        self.bottom = y;
        self.top = y + height;
        self
    }

    pub fn set_viewport(&mut self, x: f32, width: f32, height: f32, y: f32) {
        self.left = x;
        self.right = x + width;
        self.bottom = y;
        self.top = y + height;
        self.viewport = (x, y, width, height);
    }
}

impl Camera for OrthographicCamera {
    fn view_matrix(&self) -> Mat4 {
        let forward = self.rotation * Vec3::Z;
        let up = self.rotation * Vec3::Y;
        Mat4::look_at_rh(self.position, self.position + forward, up)
    }

    fn projection_matrix(&self) -> Mat4 {
        Mat4::orthographic_rh_gl(
            self.left,
            self.right,
            self.bottom,
            self.top,
            self.near_plane,
            self.far_plane,
        )
    }

    fn position(&self) -> Vec3 {
        self.position
    }

    fn forward(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }

    fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    fn near_plane(&self) -> f32 {
        self.near_plane
    }

    fn far_plane(&self) -> f32 {
        self.far_plane
    }

    fn aspect_ratio(&self) -> f32 {
        (self.right - self.left) / (self.top - self.bottom)
    }

    fn fov(&self) -> f32 {
        0.0 // Orthographic cameras don't have a FOV
    }

    fn set_position(&mut self, position: Vec3) {
        self.position = position;
    }

    fn set_rotation(&mut self, rotation: Quat) {
        self.rotation = rotation;
    }

    fn frustum_planes(&self) -> [Mat4; 6] {
        let vp = self.view_projection_matrix();
        let planes = [
            vp.row(3) + vp.row(0),
            vp.row(3) - vp.row(0),
            vp.row(3) + vp.row(1),
            vp.row(3) - vp.row(1),
            vp.row(3) + vp.row(2),
            vp.row(3) - vp.row(2),
        ];
        let mut result = [Mat4::IDENTITY; 6];
        for (i, plane) in planes.iter().enumerate() {
            let len = plane.truncate().length();
            if len > 1e-6 {
                let n = *plane / len;
                result[i] = Mat4::from_cols(n, n, n, n);
            }
        }
        result
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Default for OrthographicCamera {
    fn default() -> Self {
        Self::new(-10.0, 10.0, -10.0, 10.0, 0.1, 1000.0)
    }
}

/// Camera builder
pub struct CameraBuilder {
    camera_type: CameraType,
    position: Vec3,
    rotation: Quat,
    fov: f32,
    aspect_ratio: f32,
    near: f32,
    far: f32,
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
}

/// Camera type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraType {
    Perspective,
    Orthographic,
}

/// Camera control mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraMode {
    #[default]
    FirstPerson,
    ThirdPerson,
    Orbital,
}

impl CameraBuilder {
    pub fn new(camera_type: CameraType) -> Self {
        Self {
            camera_type,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            fov: std::f32::consts::PI / 3.0,
            aspect_ratio: 16.0 / 9.0,
            near: 0.1,
            far: 1000.0,
            left: -10.0,
            right: 10.0,
            bottom: -10.0,
            top: 10.0,
        }
    }

    pub fn with_position(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    pub fn with_rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn with_fov(mut self, fov: f32) -> Self {
        self.fov = fov;
        self
    }

    pub fn with_aspect_ratio(mut self, aspect_ratio: f32) -> Self {
        self.aspect_ratio = aspect_ratio;
        self
    }

    pub fn with_near(mut self, near: f32) -> Self {
        self.near = near;
        self
    }

    pub fn with_far(mut self, far: f32) -> Self {
        self.far = far;
        self
    }

    pub fn with_viewport(mut self, left: f32, right: f32, bottom: f32, top: f32) -> Self {
        self.left = left;
        self.right = right;
        self.bottom = bottom;
        self.top = top;
        self
    }

    pub fn build(self) -> Box<dyn Camera> {
        match self.camera_type {
            CameraType::Perspective => Box::new(
                PerspectiveCamera::new(
                    self.position,
                    self.fov,
                    self.aspect_ratio,
                    self.near,
                    self.far,
                )
                .with_rotation(self.rotation),
            ),
            CameraType::Orthographic => Box::new(
                OrthographicCamera::new(
                    self.left,
                    self.right,
                    self.bottom,
                    self.top,
                    self.near,
                    self.far,
                )
                .with_position(self.position)
                .with_rotation(self.rotation),
            ),
        }
    }
}

#[cfg(test)]
mod bug_212 {
    use super::*;

    // Bug №212: the caller's up used to be computed and thrown away
    // (`let _ = up_safe;` / `_up`), so the roll never followed it.

    #[test]
    fn look_at_rolls_with_the_callers_up() {
        // Facing straight down (forward = -Y) with up = +X: the camera's
        // local up must point along +X, not fall back to the world's Y.
        let cam = PerspectiveCamera::look_at(
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::ZERO,
            Vec3::X,
            1.0,
            1.0,
            0.1,
            100.0,
        );
        assert!((cam.forward() - Vec3::NEG_Y).length() < 1e-4);
        assert!((cam.up() - Vec3::X).length() < 1e-3);
    }

    #[test]
    fn look_at_horizon_look_keeps_the_worlds_y_up() {
        let cam = PerspectiveCamera::look_at(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::ZERO,
            Vec3::Y,
            1.0,
            1.0,
            0.1,
            100.0,
        );
        assert!((cam.forward() - Vec3::NEG_Z).length() < 1e-4);
        assert!((cam.up() - Vec3::Y).length() < 1e-3);
    }

    #[test]
    fn look_at_up_parallel_to_forward_degrades_to_a_pickable_axis() {
        // Looking straight down with up = Y — the degenerate case. The roll
        // axis falls back (X here), but stays finite and orthogonal, and
        // forward is exact.
        let cam = PerspectiveCamera::look_at(
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::ZERO,
            Vec3::Y,
            1.0,
            1.0,
            0.1,
            100.0,
        );
        assert!((cam.forward() - Vec3::NEG_Y).length() < 1e-4);
        let up = cam.up();
        assert!(up.is_finite() && (up.length() - 1.0).abs() < 1e-3);
        assert!(up.cross(cam.forward()).length() > 0.99);
    }

    #[test]
    fn ortho_look_at_honours_the_callers_up_too() {
        let cam = OrthographicCamera::look_at(
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::ZERO,
            Vec3::X,
            10.0,
            0.1,
            100.0,
        );
        assert!((cam.forward() - Vec3::NEG_Y).length() < 1e-4);
        assert!((cam.up() - Vec3::X).length() < 1e-3);
    }

    #[test]
    fn look_at_degenerate_directions_do_not_produce_nan() {
        let cam = PerspectiveCamera::look_at(
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::Y,
            1.0,
            1.0,
            0.1,
            100.0,
        );
        assert!(cam.position.is_finite());
        assert!(cam.forward().is_finite() && cam.up().is_finite());
        assert!((cam.forward().length() - 1.0).abs() < 1e-3);
    }
}
