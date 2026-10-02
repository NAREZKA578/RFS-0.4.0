use glam::{Vec3, Quat, Mat4};
use serde::{Deserialize, Serialize};
use bytemuck::{Pod, Zeroable};
use std::ops::{Add, Sub, Mul, Div, Neg, AddAssign, SubAssign, MulAssign};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Pod, Zeroable)]
pub struct Vec3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3f {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);
    pub const UP: Self = Self::new(0.0, 1.0, 0.0);
    pub const DOWN: Self = Self::new(0.0, -1.0, 0.0);
    pub const FORWARD: Self = Self::new(0.0, 0.0, 1.0);
    pub const BACK: Self = Self::new(0.0, 0.0, -1.0);
    pub const LEFT: Self = Self::new(-1.0, 0.0, 0.0);
    pub const RIGHT: Self = Self::new(1.0, 0.0, 0.0);

    /// The largest absolute component, used to keep the length and the
    /// normalization from overflowing.
    fn max_abs(self) -> f32 {
        self.x.abs().max(self.y.abs()).max(self.z.abs())
    }

    pub fn length(self) -> f32 {
        // Divided through by the largest component first.
        //
        // Squaring the components directly overflows to infinity above about
        // 1.8e19, which made `length()` return `inf` for a vector that has a
        // perfectly good finite length. The scaled form cannot overflow, and
        // scaling back only overflows when the true length genuinely does not
        // fit in an f32 — which `inf` is the honest answer for.
        let m = self.max_abs();
        if m == 0.0 {
            return 0.0;
        }
        if !m.is_finite() {
            return f32::INFINITY;
        }
        let scaled = Self::new(self.x / m, self.y / m, self.z / m);
        m * scaled.length_squared().sqrt()
    }

    pub fn length_squared(self) -> f32 {
        // Kept as the plain sum, because that is what callers that already hold
        // a safe range (the broadphase tests, the collision code) are asking
        // for, and a silently scaled result would not compare equal to a
        // hand-computed one. Callers that care about the overflow use `length`.
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn normalize(self) -> Self {
        // Normalizing is scale invariant, so the vector is divided by its
        // largest component and only then measured. Every component of the
        // scaled vector is at most 1, so nothing here can overflow.
        //
        // The previous version divided by `length()`, and for any vector longer
        // than about 1.8e19 that length was `inf`, so the guard `len > 0.0`
        // passed and the result was `self / inf` — the zero vector. That is
        // not a rounding error: a zero normal or a zero velocity direction is
        // a physical answer, and it propagated into the projectile normal, the
        // reflected velocity and every distance filter downstream.
        let m = self.max_abs();
        if m == 0.0 || !m.is_finite() {
            // No direction can be recovered from a zero or a non-finite
            // vector, and returning something plausible instead would invent
            // physics.
            return Self::ZERO;
        }
        let scaled = Self::new(self.x / m, self.y / m, self.z / m);
        let len = scaled.length_squared().sqrt();
        if len == 0.0 || !len.is_finite() {
            return Self::ZERO;
        }
        scaled / len
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn is_nan(self) -> bool {
        self.x.is_nan() || self.y.is_nan() || self.z.is_nan()
    }
}

impl Add for Vec3f {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl AddAssign for Vec3f {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
        self.z += other.z;
    }
}

impl SubAssign for Vec3f {
    fn sub_assign(&mut self, other: Self) {
        self.x -= other.x;
        self.y -= other.y;
        self.z -= other.z;
    }
}

impl MulAssign<f32> for Vec3f {
    fn mul_assign(&mut self, scalar: f32) {
        self.x *= scalar;
        self.y *= scalar;
        self.z *= scalar;
    }
}

impl Sub for Vec3f {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f32> for Vec3f {
    type Output = Self;
    fn mul(self, scalar: f32) -> Self {
        Self::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }
}

impl Neg for Vec3f {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<Vec3f> for Vec3f {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        Self::new(self.x * other.x, self.y * other.y, self.z * other.z)
    }
}

impl Mul<Vec3f> for f32 {
    type Output = Vec3f;
    fn mul(self, vector: Vec3f) -> Vec3f {
        vector * self
    }
}

impl Div<f32> for Vec3f {
    type Output = Self;
    fn div(self, scalar: f32) -> Self {
        Self::new(self.x / scalar, self.y / scalar, self.z / scalar)
    }
}

impl From<Vec3> for Vec3f {
    fn from(v: Vec3) -> Self {
        Self::new(v.x, v.y, v.z)
    }
}

impl From<Vec3f> for Vec3 {
    fn from(v: Vec3f) -> Self {
        Self::new(v.x, v.y, v.z)
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, Pod, Zeroable)]
pub struct Quatf {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Default for Quatf {
    /// A zero quaternion is degenerate (normalizes to NaN and collapses
    /// transforms), so the default is the identity rotation.
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Quatf {
    pub const IDENTITY: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    pub fn from_euler(yaw: f32, pitch: f32, roll: f32) -> Self {
        let q = Quat::from_euler(glam::EulerRot::YXZ, yaw, pitch, roll);
        Self::new(q.x, q.y, q.z, q.w)
    }

    pub fn from_axis_angle(axis: Vec3f, angle: f32) -> Self {
        let q = Quat::from_axis_angle(axis.into(), angle);
        Self::new(q.x, q.y, q.z, q.w)
    }

    pub fn mul_vec3(self, v: Vec3f) -> Vec3f {
        let q = Quat::from_xyzw(self.x, self.y, self.z, self.w);
        let v3: Vec3 = v.into();
        (q * v3).into()
    }

    pub fn slerp(self, other: Self, t: f32) -> Self {
        let q1 = Quat::from_xyzw(self.x, self.y, self.z, self.w);
        let q2 = Quat::from_xyzw(other.x, other.y, other.z, other.w);
        let q = q1.slerp(q2, t);
        Self::new(q.x, q.y, q.z, q.w)
    }

    pub fn normalize(self) -> Self {
        let len_sq = self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w;
        if !len_sq.is_finite() || len_sq < 1e-12 {
            return Self::IDENTITY;
        }
        let q = Quat::from_xyzw(self.x, self.y, self.z, self.w).normalize();
        Self::new(q.x, q.y, q.z, q.w)
    }

    pub fn inverse(self) -> Self {
        let q = Quat::from_xyzw(self.x, self.y, self.z, self.w).inverse();
        Self::new(q.x, q.y, q.z, q.w)
    }
}

impl From<Quat> for Quatf {
    fn from(q: Quat) -> Self {
        Self::new(q.x, q.y, q.z, q.w)
    }
}

impl From<Quatf> for Quat {
    fn from(q: Quatf) -> Self {
        Quat::from_xyzw(q.x, q.y, q.z, q.w)
    }
}

impl Mul<Quatf> for Quatf {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        let q1: Quat = self.into();
        let q2: Quat = other.into();
        (q1 * q2).into()
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Pod, Zeroable)]
pub struct Transform {
    pub position: Vec3f,
    pub rotation: Quatf,
    pub scale: Vec3f,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        position: Vec3f::ZERO,
        rotation: Quatf::IDENTITY,
        scale: Vec3f::ONE,
    };

    pub fn new(position: Vec3f, rotation: Quatf, scale: Vec3f) -> Self {
        Self { position, rotation, scale }
    }

    pub fn from_position(position: Vec3f) -> Self {
        Self::new(position, Quatf::IDENTITY, Vec3f::ONE)
    }

    pub fn from_rotation(rotation: Quatf) -> Self {
        Self::new(Vec3f::ZERO, rotation, Vec3f::ONE)
    }

    pub fn transform_point(&self, point: Vec3f) -> Vec3f {
        self.rotation.mul_vec3(point * self.scale) + self.position
    }

    pub fn transform_vector(&self, vector: Vec3f) -> Vec3f {
        self.rotation.mul_vec3(vector * self.scale)
    }

    pub fn inverse(&self) -> Self {
        let inv_rot = self.rotation.inverse();
        let inv_scale = Vec3f::new(1.0 / self.scale.x, 1.0 / self.scale.y, 1.0 / self.scale.z);
        let inv_pos = inv_rot.mul_vec3(-self.position * inv_scale);
        Self::new(inv_pos, inv_rot, inv_scale)
    }

    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            self.scale.into(),
            self.rotation.into(),
            self.position.into(),
        )
    }
}

impl Mul<Transform> for Transform {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self::new(
            self.transform_point(rhs.position),
            self.rotation * rhs.rotation,
            self.scale * rhs.scale,
        )
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Pod, Zeroable)]
pub struct Bounds {
    pub min: Vec3f,
    pub max: Vec3f,
}

impl Bounds {
    pub fn new(min: Vec3f, max: Vec3f) -> Self {
        Self { min, max }
    }

    pub fn center(&self) -> Vec3f {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3f {
        self.max - self.min
    }

    pub fn contains(&self, point: Vec3f) -> bool {
        point.x >= self.min.x && point.x <= self.max.x &&
        point.y >= self.min.y && point.y <= self.max.y &&
        point.z >= self.min.z && point.z <= self.max.z
    }

    pub fn intersects(&self, other: &Bounds) -> bool {
        self.min.x <= other.max.x && self.max.x >= other.min.x &&
        self.min.y <= other.max.y && self.max.y >= other.min.y &&
        self.min.z <= other.max.z && self.max.z >= other.min.z
    }

    pub fn expand(&self, amount: f32) -> Self {
        Self::new(
            self.min - Vec3f::new(amount, amount, amount),
            self.max + Vec3f::new(amount, amount, amount),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::Vec3f;

    /// The bug: `length()` squared the components, which overflows to infinity
    /// above about 1.8e19, so `normalize()` divided by infinity and returned the
    /// zero vector — a direction destroyed rather than an error raised.
    #[test]
    fn normalizing_a_huge_vector_keeps_its_direction() {
        let v = Vec3f::new(3e20, 0.0, 0.0);
        let n = v.normalize();
        assert!(
            (n.x - 1.0).abs() < 1e-6,
            "expected (1,0,0), got {n:?}: a huge vector must still have a direction"
        );
        assert!(n.y.abs() < 1e-6 && n.z.abs() < 1e-6);

        // Not just the axis-aligned case.
        let diagonal = Vec3f::new(3e20, 4e20, 0.0);
        let d = diagonal.normalize();
        assert!(
            (d.x - 0.6).abs() < 1e-5 && (d.y - 0.8).abs() < 1e-5,
            "expected (0.6,0.8,0), got {d:?}"
        );

        // Smallest and largest magnitudes that are still finite.
        let tiny = Vec3f::new(1e-30, 0.0, 0.0).normalize();
        assert!((tiny.x - 1.0).abs() < 1e-5, "got {tiny:?}");
        let big = Vec3f::new(f32::MAX, 0.0, 0.0).normalize();
        assert!((big.x - 1.0).abs() < 1e-6, "got {big:?}");
    }

    #[test]
    fn the_result_of_normalizing_is_a_unit_vector() {
        for v in [
            Vec3f::new(1.0, 2.0, 3.0),
            Vec3f::new(-1e18, 5.0, 2.0),
            Vec3f::new(1e-25, -1e-25, 1e-25),
            Vec3f::new(3e20, 4e20, 5e20),
        ] {
            let n = v.normalize();
            let len = n.x * n.x + n.y * n.y + n.z * n.z;
            assert!(
                (len - 1.0).abs() < 1e-5,
                "normalize({v:?}) gave {n:?} with length squared {len}"
            );
        }
    }

    #[test]
    fn a_zero_or_non_finite_vector_has_no_direction() {
        assert_eq!(Vec3f::ZERO.normalize(), Vec3f::ZERO);
        assert_eq!(Vec3f::new(f32::INFINITY, 0.0, 0.0).normalize(), Vec3f::ZERO);
        assert_eq!(Vec3f::new(f32::NAN, 1.0, 0.0).normalize(), Vec3f::ZERO);
    }

    #[test]
    fn length_survives_the_magnitudes_normalize_survives() {
        // 3e20 fits in an f32, so its length must too. It used to be infinity.
        let v = Vec3f::new(3e20, 4e20, 0.0);
        let len = v.length();
        assert!(
            len.is_finite() && (len - 5e20).abs() / 5e20 < 1e-5,
            "expected about 5e20, got {len}"
        );
        assert_eq!(Vec3f::ZERO.length(), 0.0);
    }

    #[test]
    fn normalizing_preserves_direction_for_ordinary_magnitudes() {
        // The pre-existing behaviour must be untouched, or the physics callers
        // that rely on it change meaning.
        let v = Vec3f::new(3.0, 4.0, 0.0);
        let n = v.normalize();
        assert!((n.x - 0.6).abs() < 1e-6 && (n.y - 0.8).abs() < 1e-6, "got {n:?}");
    }
}