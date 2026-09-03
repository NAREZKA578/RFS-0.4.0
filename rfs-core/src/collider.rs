//! Collision detection module for AABB-based collision resolution.
//! Provides structures and functions for capsule-vs-AABB collision detection.

use glam::Vec3;

use crate::spatial::SpatialGrid;

/// Cell size for collider spatial grid (matches player radius scale).
const COLLIDER_GRID_CELL: f32 = 8.0;

/// Axis-aligned bounding box for collision detection.
#[derive(Debug, Clone)]
pub struct Aabb {
    /// Minimum corner of the box.
    pub min: Vec3,
    /// Maximum corner of the box.
    pub max: Vec3,
}

impl Aabb {
    /// Creates an AABB from explicit min/max corners.
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    /// Creates an AABB centered at `pos` with the given size.
    pub fn from_pos_size(pos: Vec3, size: Vec3) -> Self {
        let half = size * 0.5;
        Self {
            min: pos - half,
            max: pos + half,
        }
    }

    /// Tests whether a circle (xz-plane) with the given radius overlaps this AABB.
    pub fn contains_xz(&self, x: f32, z: f32, radius: f32) -> bool {
        x + radius > self.min.x
            && x - radius < self.max.x
            && z + radius > self.min.z
            && z - radius < self.max.z
    }

    /// Returns the nearest X position that pushes a circle out of this AABB, or None if no overlap.
    pub fn pushout_x(&self, x: f32, _z: f32, radius: f32) -> Option<f32> {
        if !self.contains_xz(x, _z, radius) {
            return None;
        }
        let left = x - (self.min.x - radius);
        let right = (self.max.x + radius) - x;
        if left < right {
            Some(self.min.x - radius)
        } else {
            Some(self.max.x + radius)
        }
    }

    /// Returns the nearest Z position that pushes a circle out of this AABB, or None if no overlap.
    pub fn pushout_z(&self, x: f32, z: f32, radius: f32) -> Option<f32> {
        if !self.contains_xz(x, z, radius) {
            return None;
        }
        let back = z - (self.min.z - radius);
        let front = (self.max.z + radius) - z;
        if back < front {
            Some(self.min.z - radius)
        } else {
            Some(self.max.z + radius)
        }
    }

    /// Center X coordinate.
    pub fn center_x(&self) -> f32 {
        (self.min.x + self.max.x) * 0.5
    }

    /// Center Z coordinate.
    pub fn center_z(&self) -> f32 {
        (self.min.z + self.max.z) * 0.5
    }
}

/// Resolves capsule-vs-AABB collisions by sliding along X then Z axes.
/// Returns the corrected position after resolving all collider overlaps.
pub fn slide_move(pos: Vec3, _prev: Vec3, colliders: &[Aabb], radius: f32) -> Vec3 {
    let mut resolved = Vec3::new(pos.x, pos.y, pos.z);

    // First pass: slide along X
    for c in colliders {
        if let Some(new_x) = c.pushout_x(resolved.x, resolved.z, radius) {
            resolved.x = new_x;
        }
    }

    // Second pass: slide along Z
    for c in colliders {
        if let Some(new_z) = c.pushout_z(resolved.x, resolved.z, radius) {
            resolved.z = new_z;
        }
    }

    resolved
}

/// Uses spatial partitioning to reduce collision checks from O(n) to O(1) average.
/// Builds a temporary grid from the collider slice, then queries only nearby colliders.
pub fn slide_move_optimized(pos: Vec3, _prev: Vec3, colliders: &[Aabb], radius: f32) -> Vec3 {
    if colliders.len() < 16 {
        return slide_move(pos, Vec3::ZERO, colliders, radius);
    }

    // Build spatial grid
    let mut grid: SpatialGrid<Aabb> = SpatialGrid::new(COLLIDER_GRID_CELL);
    for c in colliders {
        grid.insert(c.clone(), c.center_x(), c.center_z());
    }

    let mut resolved = Vec3::new(pos.x, pos.y, pos.z);

    // Check colliders in cells overlapping the player's sweep area
    let sweep_radius = radius + COLLIDER_GRID_CELL;
    let relevant: Vec<_> = grid
        .query_radius(resolved.x, resolved.z, sweep_radius)
        .into_iter()
        .map(|(aabb, _, _)| aabb)
        .collect();

    // Slide along X
    for c in &relevant {
        if let Some(new_x) = c.pushout_x(resolved.x, resolved.z, radius) {
            resolved.x = new_x;
        }
    }

    // Slide along Z
    for c in &relevant {
        if let Some(new_z) = c.pushout_z(resolved.x, resolved.z, radius) {
            resolved.z = new_z;
        }
    }

    resolved
}
