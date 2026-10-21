use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

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
        Self {
            position,
            rotation,
            scale,
        }
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

/// Returns the lower of two corners on one axis, repairing an inverted pair.
///
/// A non-finite value is returned unchanged on purpose: `f32::min` and
/// `f32::max` return the *other* operand when one is NaN, so sorting with them
/// would fabricate a finite box out of a NaN one and hide the bad data from the
/// `query_bounds` guard that is supposed to reject it.
fn low(lo: f32, hi: f32) -> f32 {
    if lo.is_finite() && hi.is_finite() && lo > hi {
        hi
    } else {
        lo
    }
}

/// The upper counterpart of [`low`]. Same NaN rule, mirrored.
fn high(lo: f32, hi: f32) -> f32 {
    if lo.is_finite() && hi.is_finite() && lo > hi {
        lo
    } else {
        hi
    }
}

impl Bounds {
    /// Bug №199: `min`/`max` used to be stored exactly as given, so one
    /// inverted axis (`min > max`) produced a box that was not a box. Every
    /// method below then misreported:
    ///
    /// - `size()` returned a negative component,
    /// - `contains()` was never satisfied on that axis,
    /// - `intersects()` gave arbitrary answers, and
    /// - `distance_to_bounds` in the damage crate clamped to nothing on that
    ///   axis, so the axis contributed a distance of **zero**. One inverted axis
    ///   in a compartment template therefore under-reported the blast distance,
    ///   which `falloff(0.0)` scores as 1.0 — full damage — so a single bad
    ///   number in the ship data made explosions hit at full strength
    ///   everywhere.
    ///
    /// The extent is now ordered per axis on the way in, so the invariant holds
    /// by construction instead of at each call site. Computed callers
    /// (`position ± half`) were already ordered, so this is a no-op for them;
    /// the literal ship-class data in `ship/src/loader.rs` is what it repairs.
    /// `Bounds::new` is the only construction site in the workspace — every
    /// other builder goes through it — so ordering here covers all callers.
    ///
    /// A non-finite corner is deliberately **left alone**. Sorting with `f32::min`
    /// and `f32::max` would be shorter, but both return the finite operand when
    /// the other is NaN, which would silently turn a NaN box into a valid
    /// degenerate one: `min(NaN, 1.0)` is `1.0`, so `Bounds::new` with a NaN
    /// corner would fabricate a box at x == 1. That breaks
    /// `SpatialGrid::query_bounds`, whose whole contract is that a non-finite
    /// query matches nothing (`query_bounds_nan_returns_empty`), and it would
    /// hide bad data instead of surfacing it. Keep NaN visible.
    pub fn new(min: Vec3f, max: Vec3f) -> Self {
        Self {
            min: Vec3f::new(low(min.x, max.x), low(min.y, max.y), low(min.z, max.z)),
            max: Vec3f::new(high(min.x, max.x), high(min.y, max.y), high(min.z, max.z)),
        }
    }

    /// Re-orders the axes of a box that is already in hand.
    ///
    /// `Bounds` is `Serialize`/`Deserialize` and its fields are `pub`, so values
    /// can arrive from the wire or be written directly, bypassing `new`. Use
    /// this at the point of consumption for those.
    pub fn normalized(&self) -> Self {
        Self::new(self.min, self.max)
    }

    /// Whether any axis is inverted, i.e. the box is malformed.
    pub fn is_inverted(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn center(&self) -> Vec3f {
        (self.min + self.max) * 0.5
    }

    /// Bug №199: for an inverted axis this used to yield a negative component.
    /// A non-negative size is now guaranteed by the ordering `new` maintains.
    pub fn size(&self) -> Vec3f {
        self.max - self.min
    }

    pub fn contains(&self, point: Vec3f) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    pub fn intersects(&self, other: &Bounds) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
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
    use super::{Bounds, Vec3f};

    /// Bug №199: a box whose corners arrive swapped used to stay that way, and
    /// `size()` then reported a negative extent.
    #[test]
    fn an_inverted_axis_is_ordered_on_construction() {
        let b = Bounds::new(Vec3f::new(5.0, 5.0, 5.0), Vec3f::new(-5.0, -5.0, -5.0));
        assert_eq!(b.min, Vec3f::new(-5.0, -5.0, -5.0));
        assert_eq!(b.max, Vec3f::new(5.0, 5.0, 5.0));
        assert!(!b.is_inverted());
    }

    /// One bad axis must not disturb the others: here y is a genuinely flat axis
    /// (min == max) that must survive as flat, while the inverted z axis is
    /// repaired independently.
    #[test]
    fn one_inverted_axis_does_not_disturb_the_good_axes() {
        let b = Bounds::new(Vec3f::new(0.0, 2.0, 4.0), Vec3f::new(4.0, 2.0, 0.0));
        assert!(!b.is_inverted());
        assert_eq!(b.min, Vec3f::new(0.0, 2.0, 0.0));
        assert_eq!(b.max, Vec3f::new(4.0, 2.0, 4.0));
        let s = b.size();
        assert_eq!(s.y, 0.0, "a genuinely flat axis was altered");
        assert_eq!(s.x, 4.0);
        assert_eq!(s.z, 4.0, "the inverted axis was not repaired");
    }

    /// An inverted axis and a degenerate one are different: min == max is valid
    /// data and must be left exactly as it is.
    #[test]
    fn a_degenerate_axis_is_left_alone() {
        let b = Bounds::new(Vec3f::new(-1.0, -1.0, 7.0), Vec3f::new(1.0, 1.0, 7.0));
        assert!(!b.is_inverted(), "min == max is not inverted");
        assert_eq!(b.size().z, 0.0);
        assert_eq!(b.min.z, 7.0);
        assert_eq!(b.max.z, 7.0);
    }

    /// The `size()` regression: it returned `max - min`, so an inverted axis
    /// produced a negative extent that callers would propagate.
    #[test]
    fn size_is_never_negative() {
        let b = Bounds::new(Vec3f::new(10.0, -1.0, 0.0), Vec3f::new(-10.0, 1.0, 7.0));
        let s = b.size();
        assert!(
            s.x >= 0.0 && s.y >= 0.0 && s.z >= 0.0,
            "negative size {s:?}"
        );
        assert_eq!(s, Vec3f::new(20.0, 2.0, 7.0));
    }

    /// Swapping the arguments must give the identical box, otherwise "which
    /// corner is min" would still matter to callers.
    #[test]
    fn corner_order_does_not_change_the_box() {
        let a = Bounds::new(Vec3f::new(-2.0, -3.0, -4.0), Vec3f::new(6.0, 7.0, 8.0));
        let b = Bounds::new(Vec3f::new(6.0, 7.0, 8.0), Vec3f::new(-2.0, -3.0, -4.0));
        assert_eq!(a, b);
    }

    /// A well-formed box must be completely untouched, or the normalization is
    /// not a no-op and would be changing behaviour everywhere.
    #[test]
    fn a_well_formed_box_is_unchanged() {
        let min = Vec3f::new(-1.0, -2.0, -3.0);
        let max = Vec3f::new(4.0, 5.0, 6.0);
        let b = Bounds::new(min, max);
        assert_eq!(b.min, min);
        assert_eq!(b.max, max);
        assert!(!b.is_inverted());
    }

    /// `center()` was already correct for a swapped pair — the midpoint of two
    /// corners does not care which is called min. Pinned down so the
    /// normalization is not wrongly credited with fixing it.
    #[test]
    fn center_is_unaffected_by_corner_order() {
        let min = Vec3f::new(-1.0, -2.0, -3.0);
        let max = Vec3f::new(4.0, 5.0, 6.0);
        assert_eq!(
            Bounds::new(min, max).center(),
            Bounds::new(max, min).center()
        );
    }

    /// `contains` was unsatisfiable on an inverted axis, so it always said
    /// "outside". An ordered box must contain its own center.
    #[test]
    fn an_ordered_box_contains_its_own_center() {
        let b = Bounds::new(Vec3f::new(5.0, 5.0, 5.0), Vec3f::new(-5.0, -5.0, -5.0));
        assert!(b.contains(b.center()), "the box rejected its own center");
    }

    /// A box cannot fail to intersect itself under either corner order.
    #[test]
    fn a_box_intersects_itself_regardless_of_corner_order() {
        let min = Vec3f::new(-1.0, -1.0, -1.0);
        let max = Vec3f::new(1.0, 1.0, 1.0);
        assert!(Bounds::new(min, max).intersects(&Bounds::new(min, max)));
        assert!(Bounds::new(max, min).intersects(&Bounds::new(max, min)));
    }

    /// Deserialization and direct field writes bypass `new`, so `normalized` is
    /// the repair for values that arrive already malformed.
    #[test]
    fn normalized_repairs_a_box_built_by_hand() {
        let malformed = Bounds {
            min: Vec3f::new(3.0, 0.0, 0.0),
            max: Vec3f::new(-3.0, 1.0, 1.0),
        };
        assert!(malformed.is_inverted());
        assert!(!malformed.normalized().is_inverted());
        assert_eq!(malformed.normalized().size(), Vec3f::new(6.0, 1.0, 1.0));
    }

    /// Regression, and the reason `order` exists at all: sorting the corners
    /// with `f32::min`/`f32::max` looks equivalent but is not. Both return the
    /// finite operand when the other is NaN, so a NaN corner was silently
    /// replaced by the opposite corner — fabricating a valid degenerate box at
    /// x == 1 and defeating `query_bounds`' contract that a non-finite query
    /// matches nothing.
    #[test]
    fn a_nan_corner_is_not_silently_replaced() {
        let nan = f32::NAN;
        let b = Bounds::new(Vec3f::new(nan, 0.0, 0.0), Vec3f::new(1.0, 1.0, 1.0));
        assert!(
            b.min.x.is_nan(),
            "the NaN corner became {}, which fabricates a valid box",
            b.min.x
        );
        assert_eq!(b.max.x, 1.0, "the finite corner should be untouched");
    }

    /// The same for the other two axes, and for an infinity, so the guard is
    /// about non-finiteness rather than about NaN specifically.
    #[test]
    fn every_axis_keeps_its_non_finite_corner() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let b = Bounds::new(Vec3f::new(bad, bad, bad), Vec3f::new(5.0, 5.0, 5.0));
            assert!(!b.min.x.is_finite(), "{bad} was replaced on x");
            assert!(!b.min.y.is_finite(), "{bad} was replaced on y");
            assert!(!b.min.z.is_finite(), "{bad} was replaced on z");
            assert_eq!(b.max.x, 5.0, "{bad} disturbed the finite corner");
        }
    }

    /// A box with a NaN corner stays detectable as unusable, which is what the
    /// downstream `is_finite` guards rely on.
    #[test]
    fn a_nan_box_reports_itself_as_inverted() {
        // min.x is NaN so `NaN > 1.0` is false; the box is not "inverted", but
        // it is also not finite, and consumers must not treat it as valid.
        let b = Bounds::new(Vec3f::new(f32::NAN, 0.0, 0.0), Vec3f::new(1.0, 1.0, 1.0));
        assert!(!b.min.is_finite() || !b.max.is_finite());
    }

    /// Ordering still happens for the ordinary finite case — the NaN guard must
    /// not have disabled the repair.
    #[test]
    fn ordering_still_works_for_finite_corners() {
        let b = Bounds::new(Vec3f::new(5.0, 5.0, 5.0), Vec3f::new(-5.0, -5.0, -5.0));
        assert_eq!(b.min, Vec3f::new(-5.0, -5.0, -5.0));
        assert_eq!(b.max, Vec3f::new(5.0, 5.0, 5.0));
    }

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
        assert!(
            (n.x - 0.6).abs() < 1e-6 && (n.y - 0.8).abs() < 1e-6,
            "got {n:?}"
        );
    }
}
