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

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn normalize(self) -> Self {
        let len = self.length();
        if len > 0.0 {
            self / len
        } else {
            Self::ZERO
        }
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