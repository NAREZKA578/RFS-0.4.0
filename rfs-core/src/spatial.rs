use std::collections::HashMap;

/// A uniform spatial grid for fast proximity and overlap queries.
///
/// Divides 2D space (XZ plane) into cells of configurable size.
/// Objects are inserted into cells based on their position, and
/// queries only check nearby cells instead of all objects.
pub struct SpatialGrid<T> {
    #[allow(dead_code)]
    cell_size: f32,
    inv_cell_size: f32,
    cells: HashMap<(i32, i32), Vec<SpatialEntry<T>>>,
}

struct SpatialEntry<T> {
    item: T,
    x: f32,
    z: f32,
}

impl<T> SpatialGrid<T> {
    /// Create a new spatial grid with the given cell size.
    ///
    /// # Panics
    /// Panics in debug mode if `cell_size` is not positive and finite.
    /// In release mode, a non-positive `cell_size` is clamped to `1.0`.
    pub fn new(cell_size: f32) -> Self {
        debug_assert!(
            cell_size.is_finite() && cell_size > 0.0,
            "SpatialGrid cell_size must be positive and finite, got {}",
            cell_size
        );
        let cell_size = if cell_size.is_finite() && cell_size > 0.0 {
            cell_size
        } else {
            1.0
        };
        Self {
            cell_size,
            inv_cell_size: 1.0 / cell_size,
            cells: HashMap::new(),
        }
    }

    /// Clear all entries (reuse allocation).
    pub fn clear(&mut self) {
        self.cells.clear();
    }

    /// Insert an item at the given world position.
    pub fn insert(&mut self, item: T, x: f32, z: f32) {
        let cx = (x * self.inv_cell_size).floor() as i32;
        let cz = (z * self.inv_cell_size).floor() as i32;
        self.cells
            .entry((cx, cz))
            .or_default()
            .push(SpatialEntry { item, x, z });
    }

    /// Query all items within a circular radius of (qx, qz).
    /// Returns references to items and their positions.
    pub fn query_radius(&self, qx: f32, qz: f32, radius: f32) -> Vec<(&T, f32, f32)> {
        if !qx.is_finite() || !qz.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return Vec::new();
        }
        let radius = radius.min(1000.0);
        let mut results = Vec::new();
        let r2 = radius * radius;

        // Clamp query position to prevent integer overflow in cell coordinate calculation
        // This protects against very large (but finite) values that could cause i32 overflow
        let qx = qx.clamp(-1_000_000.0, 1_000_000.0);
        let qz = qz.clamp(-1_000_000.0, 1_000_000.0);

        let min_cx = ((qx - radius) * self.inv_cell_size).floor() as i32;
        let max_cx = ((qx + radius) * self.inv_cell_size).floor() as i32;
        let min_cz = ((qz - radius) * self.inv_cell_size).floor() as i32;
        let max_cz = ((qz + radius) * self.inv_cell_size).floor() as i32;

        for cx in min_cx..=max_cx {
            for cz in min_cz..=max_cz {
                if let Some(cell) = self.cells.get(&(cx, cz)) {
                    for entry in cell {
                        let dx = entry.x - qx;
                        let dz = entry.z - qz;
                        if dx * dx + dz * dz <= r2 {
                            results.push((&entry.item, entry.x, entry.z));
                        }
                    }
                }
            }
        }
        results
    }

    /// Query all items whose AABB overlaps the given rectangle.
    pub fn query_rect(
        &self,
        min_x: f32,
        min_z: f32,
        max_x: f32,
        max_z: f32,
    ) -> Vec<(&T, f32, f32)> {
        let mut results = Vec::new();

        // Clamp coordinates to prevent overflow from huge values
        let min_cx = ((min_x * self.inv_cell_size).floor() as i32).clamp(-1_000_000, 1_000_000);
        let max_cx = ((max_x * self.inv_cell_size).floor() as i32).clamp(-1_000_000, 1_000_000);
        let min_cz = ((min_z * self.inv_cell_size).floor() as i32).clamp(-1_000_000, 1_000_000);
        let max_cz = ((max_z * self.inv_cell_size).floor() as i32).clamp(-1_000_000, 1_000_000);

        for cx in min_cx..=max_cx {
            for cz in min_cz..=max_cz {
                if let Some(cell) = self.cells.get(&(cx, cz)) {
                    for entry in cell {
                        // Item is a point; check if it falls within the query rect
                        if entry.x >= min_x
                            && entry.x <= max_x
                            && entry.z >= min_z
                            && entry.z <= max_z
                        {
                            results.push((&entry.item, entry.x, entry.z));
                        }
                    }
                }
            }
        }
        results
    }

    /// Find the nearest item to (qx, qz) within a maximum radius.
    pub fn query_nearest(&self, qx: f32, qz: f32, max_radius: f32) -> Option<(&T, f32, f32, f32)> {
        let candidates = self.query_radius(qx, qz, max_radius);
        let mut best: Option<(&T, f32, f32, f32)> = None;
        let mut best_r2 = max_radius * max_radius;

        for (item, x, z) in candidates {
            let dx = x - qx;
            let dz = z - qz;
            let r2 = dx * dx + dz * dz;
            if r2 < best_r2 {
                best_r2 = r2;
                best = Some((item, x, z, r2.sqrt()));
            }
        }
        best
    }

    /// Total number of inserted items.
    pub fn len(&self) -> usize {
        self.cells.values().map(|c| c.len()).sum()
    }

    /// Whether the grid is empty.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}
