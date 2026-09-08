use crate::entity::SpatialEntity;
use crate::math::{Vec3f, Bounds};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::HashMap;

pub const SPATIAL_CELL_SIZE: f32 = 1000.0;
pub const MAX_ENTITIES_PER_CELL: usize = 64;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CellCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl CellCoord {
    pub fn from_position(pos: Vec3f) -> Self {
        Self {
            x: (pos.x / SPATIAL_CELL_SIZE).floor() as i32,
            y: (pos.y / SPATIAL_CELL_SIZE).floor() as i32,
            z: (pos.z / SPATIAL_CELL_SIZE).floor() as i32,
        }
    }

    pub fn neighbors(&self) -> SmallVec<[CellCoord; 27]> {
        let mut result = SmallVec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    result.push(CellCoord {
                        x: self.x + dx,
                        y: self.y + dy,
                        z: self.z + dz,
                    });
                }
            }
        }
        result
    }

    pub fn neighbors_2d(&self) -> SmallVec<[CellCoord; 9]> {
        let mut result = SmallVec::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                result.push(CellCoord {
                    x: self.x + dx,
                    y: self.y,
                    z: self.z + dz,
                });
            }
        }
        result
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(pub u64);

impl EntityId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn nil() -> Self {
        Self(0)
    }

    pub fn is_nil(&self) -> bool {
        self.0 == 0
    }
}

impl Default for EntityId {
    fn default() -> Self {
        Self::nil()
    }
}

#[derive(Debug, Default)]
pub struct SpatialGrid<T: SpatialEntity> {
    cells: HashMap<CellCoord, SmallVec<[T; MAX_ENTITIES_PER_CELL]>>,
    entity_cells: HashMap<EntityId, CellCoord>,
}

impl<T: SpatialEntity + Clone> SpatialGrid<T> {
    pub fn new() -> Self {
        Self {
            cells: HashMap::new(),
            entity_cells: HashMap::new(),
        }
    }

    pub fn insert(&mut self, entity: T) {
        let id = entity.entity_id();
        let cell = CellCoord::from_position(entity.position());
        
        if let Some(old_cell) = self.entity_cells.get(&id) {
            if *old_cell == cell {
                if let Some(cell_entities) = self.cells.get_mut(&cell) {
                    if let Some(idx) = cell_entities.iter().position(|e| e.entity_id() == id) {
                        cell_entities[idx] = entity;
                        return;
                    }
                }
            } else {
                self.remove(id);
            }
        }

        self.entity_cells.insert(id, cell);
        self.cells.entry(cell).or_default().push(entity);
    }

    pub fn remove(&mut self, id: EntityId) -> Option<T> {
        if let Some(cell) = self.entity_cells.remove(&id) {
            if let Some(cell_entities) = self.cells.get_mut(&cell) {
                if let Some(idx) = cell_entities.iter().position(|e| e.entity_id() == id) {
                    let entity = cell_entities.swap_remove(idx);
                    if cell_entities.is_empty() {
                        self.cells.remove(&cell);
                    }
                    return Some(entity);
                }
            }
        }
        None
    }

    pub fn update_position(&mut self, id: EntityId, _new_pos: Vec3f) -> bool {
        if let Some(entity) = self.remove(id) {
            self.insert(entity);
            true
        } else {
            false
        }
    }

    pub fn query_radius(&self, center: Vec3f, radius: f32) -> SmallVec<[&T; 32]> {
        let mut result = SmallVec::new();
        if !radius.is_finite() || radius < 0.0 {
            return result;
        }
        let min_cell = CellCoord::from_position(center - Vec3f::new(radius, radius, radius));
        let max_cell = CellCoord::from_position(center + Vec3f::new(radius, radius, radius));

        // Sparse grid shortcut: visiting mostly-empty cells costs a hash lookup
        // each, while a linear scan costs one distance check per entity.
        // (e.g. 16 ships with radius 5000/cell 1000 = 1331 lookups vs 16 checks.)
        let nx = (max_cell.x as i64 - min_cell.x as i64 + 1).max(0) as u64;
        let ny = (max_cell.y as i64 - min_cell.y as i64 + 1).max(0) as u64;
        let nz = (max_cell.z as i64 - min_cell.z as i64 + 1).max(0) as u64;
        let cell_visits = nx.saturating_mul(ny).saturating_mul(nz);
        let linear_threshold = (self.entity_cells.len() as u64)
            .saturating_mul(8)
            .saturating_add(64);
        if cell_visits > linear_threshold {
            for entities in self.cells.values() {
                for entity in entities {
                    if entity.position().distance(center) <= radius {
                        result.push(entity);
                    }
                }
            }
            return result;
        }

        for x in min_cell.x..=max_cell.x {
            for y in min_cell.y..=max_cell.y {
                for z in min_cell.z..=max_cell.z {
                    let cell = CellCoord { x, y, z };
                    if let Some(entities) = self.cells.get(&cell) {
                        for entity in entities {
                            if entity.position().distance(center) <= radius {
                                result.push(entity);
                            }
                        }
                    }
                }
            }
        }
        result
    }

    pub fn query_bounds(&self, bounds: &Bounds) -> SmallVec<[&T; 32]> {
        let mut result = SmallVec::new();
        let min_cell = CellCoord::from_position(bounds.min);
        let max_cell = CellCoord::from_position(bounds.max);
        
        for x in min_cell.x..=max_cell.x {
            for y in min_cell.y..=max_cell.y {
                for z in min_cell.z..=max_cell.z {
                    let cell = CellCoord { x, y, z };
                    if let Some(entities) = self.cells.get(&cell) {
                        for entity in entities {
                            if entity.bounds().intersects(bounds) {
                                result.push(entity);
                            }
                        }
                    }
                }
            }
        }
        result
    }

    pub fn get_cell_entities(&self, cell: CellCoord) -> Option<&SmallVec<[T; MAX_ENTITIES_PER_CELL]>> {
        self.cells.get(&cell)
    }

    pub fn entity_count(&self) -> usize {
        self.entity_cells.len()
    }

    pub fn all_entities(&self) -> Vec<T> {
        let mut result = Vec::with_capacity(self.entity_cells.len());
        for entities in self.cells.values() {
            result.extend(entities.iter().cloned());
        }
        result
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.entity_cells.clear();
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct InterestLayer(pub u8);

impl InterestLayer {
    pub const SHIP: Self = Self(0);
    pub const COMPARTMENTS: Self = Self(1);
    pub const PLAYER: Self = Self(2);
    pub const PROJECTILES: Self = Self(3);
    pub const ALL: Self = Self(255);
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct InterestMask: u32 {
        const SHIP = 1 << 0;
        const COMPARTMENTS = 1 << 1;
        const PLAYER = 1 << 2;
        const PROJECTILES = 1 << 3;
        const ALL = Self::SHIP.bits() | Self::COMPARTMENTS.bits() | Self::PLAYER.bits() | Self::PROJECTILES.bits();
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InterestConfig {
    pub ship_radius: f32,
    pub ship_update_rate: f32,
    pub compartments_radius: f32,
    pub compartments_update_rate: f32,
    pub player_radius: f32,
    pub player_update_rate: f32,
    pub projectiles_radius: f32,
    pub projectiles_update_rate: f32,
}

impl Default for InterestConfig {
    fn default() -> Self {
        Self {
            ship_radius: 5000.0,
            ship_update_rate: 10.0,
            compartments_radius: 200.0,
            compartments_update_rate: 5.0,
            player_radius: 100.0,
            player_update_rate: 30.0,
            projectiles_radius: 3000.0,
            projectiles_update_rate: 30.0,
        }
    }
}

pub fn calculate_interest_mask(
    viewer_pos: Vec3f,
    viewer_ship: EntityId,
    target_pos: Vec3f,
    target_ship: EntityId,
    target_layer: InterestLayer,
    config: &InterestConfig,
) -> bool {
    let distance = viewer_pos.distance(target_pos);
    
    match target_layer {
        InterestLayer::SHIP => {
            distance <= config.ship_radius
        }
        InterestLayer::COMPARTMENTS => {
            viewer_ship == target_ship && distance <= config.compartments_radius
        }
        InterestLayer::PLAYER => {
            viewer_ship == target_ship && distance <= config.player_radius
        }
        InterestLayer::PROJECTILES => {
            distance <= config.projectiles_radius
        }
        InterestLayer::ALL => true,
        _ => false,
    }
}