//! Culling
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use glam::{Mat4, Vec3};

/// Axis-Aligned Bounding Box
#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) / 2.0
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn transform(&self, matrix: Mat4) -> Self {
        // Transform all 8 corners and find new min/max
        let corners = [
            Vec3::new(self.min.x, self.min.y, self.min.z),
            Vec3::new(self.max.x, self.min.y, self.min.z),
            Vec3::new(self.min.x, self.max.y, self.min.z),
            Vec3::new(self.max.x, self.max.y, self.min.z),
            Vec3::new(self.min.x, self.min.y, self.max.z),
            Vec3::new(self.max.x, self.min.y, self.max.z),
            Vec3::new(self.min.x, self.max.y, self.max.z),
            Vec3::new(self.max.x, self.max.y, self.max.z),
        ];

        let mut new_min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut new_max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

        for corner in &corners {
            let transformed = matrix.transform_point3(*corner);
            new_min = new_min.min(transformed);
            new_max = new_max.max(transformed);
        }

        Self::new(new_min, new_max)
    }

    pub fn contains(&self, point: Vec3) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    pub fn intersects(&self, other: &Aabb) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }
}

/// Frustum plane
#[derive(Debug, Clone, Copy)]
pub struct FrustumPlane {
    pub normal: Vec3,
    pub distance: f32,
}

impl FrustumPlane {
    pub fn new(normal: Vec3, distance: f32) -> Self {
        Self {
            normal: normal.normalize(),
            distance,
        }
    }

    pub fn distance_to_point(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.distance
    }
}

/// Frustum for culling
#[derive(Debug, Clone)]
pub struct Frustum {
    pub planes: [FrustumPlane; 6], // Near, Far, Left, Right, Top, Bottom
}

impl Frustum {
    pub fn new() -> Self {
        Self {
            planes: [
                FrustumPlane::new(Vec3::Z, 0.0),     // Near (placeholder)
                FrustumPlane::new(Vec3::NEG_Z, 0.0), // Far (placeholder)
                FrustumPlane::new(Vec3::X, 0.0),     // Left (placeholder)
                FrustumPlane::new(Vec3::NEG_X, 0.0), // Right (placeholder)
                FrustumPlane::new(Vec3::Y, 0.0),     // Top (placeholder)
                FrustumPlane::new(Vec3::NEG_Y, 0.0), // Bottom (placeholder)
            ],
        }
    }

    pub fn from_matrix(matrix: Mat4) -> Self {
        // Extract frustum planes from view-projection matrix
        // This is a simplified version - actual implementation would extract
        // the planes from the matrix columns

        let mut frustum = Self::new();

        // Extract planes from matrix
        // Row 3 of the matrix contains the plane equations
        let m = matrix.to_cols_array();

        // Near plane (column 3 + column 2)
        let p3 = Vec3::new(m[3], m[7], m[11]);
        let p2 = Vec3::new(m[2], m[6], m[10]);
        frustum.planes[0] = FrustumPlane::new(p3 + p2, m[15] + m[14]);

        // Far plane (column 3 - column 2)
        frustum.planes[1] = FrustumPlane::new(p3 - p2, m[15] - m[14]);

        // Left plane (column 3 + column 0)
        let p0 = Vec3::new(m[0], m[4], m[8]);
        frustum.planes[2] = FrustumPlane::new(p3 + p0, m[15] + m[12]);

        // Right plane (column 3 - column 0)
        frustum.planes[3] = FrustumPlane::new(p3 - p0, m[15] - m[12]);

        // Top plane (column 3 - column 1)
        let p1 = Vec3::new(m[1], m[5], m[9]);
        frustum.planes[4] = FrustumPlane::new(p3 - p1, m[15] - m[13]);

        // Bottom plane (column 3 + column 1)
        frustum.planes[5] = FrustumPlane::new(p3 + p1, m[15] + m[13]);

        frustum
    }

    pub fn is_visible(&self, aabb: &Aabb) -> bool {
        for plane in &self.planes {
            // Calculate the distance from the plane to the AABB
            let mut distance = plane.distance_to_point(aabb.min);

            // Find the maximum distance (the point farthest in the plane's normal direction)
            if plane.normal.x >= 0.0 {
                distance = distance
                    .max(plane.distance_to_point(Vec3::new(aabb.max.x, aabb.min.y, aabb.min.z)));
            } else {
                distance = distance
                    .max(plane.distance_to_point(Vec3::new(aabb.min.x, aabb.min.y, aabb.min.z)));
            }

            if plane.normal.y >= 0.0 {
                distance = distance
                    .max(plane.distance_to_point(Vec3::new(aabb.min.x, aabb.max.y, aabb.min.z)));
            } else {
                distance = distance
                    .max(plane.distance_to_point(Vec3::new(aabb.min.x, aabb.min.y, aabb.min.z)));
            }

            if plane.normal.z >= 0.0 {
                distance = distance
                    .max(plane.distance_to_point(Vec3::new(aabb.min.x, aabb.min.y, aabb.max.z)));
            } else {
                distance = distance
                    .max(plane.distance_to_point(Vec3::new(aabb.min.x, aabb.min.y, aabb.min.z)));
            }

            // If the entire AABB is on the outside of the plane, it's not visible
            if distance < 0.0 {
                return false;
            }
        }

        true
    }
}

/// Frustum culler
pub struct FrustumCuller {
    frustum: Frustum,
}

impl FrustumCuller {
    pub fn new() -> Self {
        Self {
            frustum: Frustum::new(),
        }
    }

    pub fn update(&mut self, view_proj_matrix: Mat4) {
        self.frustum = Frustum::from_matrix(view_proj_matrix);
    }

    pub fn is_visible(&self, aabb: &Aabb) -> bool {
        self.frustum.is_visible(aabb)
    }

    pub fn is_visible_point(&self, point: Vec3) -> bool {
        for plane in &self.frustum.planes {
            if plane.distance_to_point(point) < 0.0 {
                return false;
            }
        }
        true
    }
}

impl Default for FrustumCuller {
    fn default() -> Self {
        Self::new()
    }
}

/// Culling result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CullingResult {
    Visible,
    PartiallyVisible,
    NotVisible,
}

/// Occlusion culler (placeholder - would use GPU queries in actual implementation)
pub struct OcclusionCuller {
    // In actual implementation, this would use:
    // - GPU occlusion queries
    // - Depth buffer from previous frame
    // - etc.
}

impl OcclusionCuller {
    pub fn new() -> Self {
        Self {}
    }

    pub fn is_visible(&self, _aabb: &Aabb) -> CullingResult {
        // In actual implementation, this would check if the AABB
        // is occluded by other objects
        CullingResult::Visible
    }
}

impl Default for OcclusionCuller {
    fn default() -> Self {
        Self::new()
    }
}
