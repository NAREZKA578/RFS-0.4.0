use crate::entity::SpatialEntity;
use crate::math::{Bounds, Vec3f};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
#[cfg(test)]
use std::cell::Cell;
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
                    // Bug №237: `self.x + dx` on an i32 near the bounds
                    // overflows — a panic in debug, a silent wrap in release
                    // that puts the "neighbour" in a cell on the far side of
                    // the grid. Saturation keeps the coordinate extreme rather
                    // than wrapping it, so a saturated cell still returns its
                    // own (extreme) neighbours instead of unrelated ones.
                    result.push(CellCoord {
                        x: self.x.saturating_add(dx),
                        y: self.y.saturating_add(dy),
                        z: self.z.saturating_add(dz),
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
                // Bug №237: saturating, as in `neighbors`.
                result.push(CellCoord {
                    x: self.x.saturating_add(dx),
                    y: self.y,
                    z: self.z.saturating_add(dz),
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
    /// Entities whose own cell was already full.
    ///
    /// Bug №201: these used to be parked in a *neighbouring* cell to keep the
    /// per-cell `SmallVec` on the stack. A query only walks the cells its own
    /// range covers, so an entity parked one cell over is invisible to every
    /// query whose range does not happen to include that neighbour — including
    /// a query centred exactly on the entity. The claim that the spill was
    /// "invisible to queries" because they filter by true position was wrong:
    /// that filter runs only for entities the cell walk has already found.
    ///
    /// Parking them here instead keeps the memory bound that motivated the cap
    /// (a cell never grows past `MAX_ENTITIES_PER_CELL` on the stack) without
    /// moving an entity away from the cell its position maps to. Every query
    /// scans this list in addition to its cells.
    spilled: Vec<T>,
    entity_cells: HashMap<EntityId, CellCoord>,
    /// Bug №238: test instrumentation — how many cells the most recent query
    /// walked. The walk is exactly what must be avoided for a huge request
    /// over a sparse grid, so tests need a hard number, not a feeling.
    #[cfg(test)]
    walked_query_cells: Cell<u64>,
}

impl<T: SpatialEntity + Clone> SpatialGrid<T> {
    pub fn new() -> Self {
        Self {
            cells: HashMap::new(),
            spilled: Vec::new(),
            entity_cells: HashMap::new(),
            #[cfg(test)]
            walked_query_cells: Cell::new(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn walked_query_cells(&self) -> u64 {
        self.walked_query_cells.get()
    }

    pub fn insert(&mut self, entity: T) {
        let id = entity.entity_id();
        let position = entity.position();
        // Bug №161: a non-finite position used to cast into a garbage cell
        // and linger in the grid forever (nothing could ever find or remove it).
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            return;
        }
        let cell = CellCoord::from_position(position);

        if let Some(old_cell) = self.entity_cells.get(&id) {
            if *old_cell == cell {
                if let Some(cell_entities) = self.cells.get_mut(&cell) {
                    if let Some(idx) = cell_entities.iter().position(|e| e.entity_id() == id) {
                        cell_entities[idx] = entity;
                        return;
                    }
                }
                // The id is tracked against this cell but is not in it, so it
                // must be in the spill list. Replacing it in place used to be
                // missing: the lookup above missed, the code fell through, and a
                // fresh copy was pushed onto `spilled` on every single update.
                // `update_player`/`update_station` run per tick, so with more
                // than `MAX_ENTITIES_PER_CELL` entities sharing a cell (all
                // players on one ship, say) the spill list grew without bound
                // and every `query_radius` scanned all of it — quadratic, and
                // it silently dominated the tick budget under load.
                if let Some(idx) = self.spilled.iter().position(|e| e.entity_id() == id) {
                    self.spilled[idx] = entity;
                    return;
                }
            } else {
                self.remove(id);
            }
        }

        // Bug №62: MAX_ENTITIES_PER_CELL was never enforced — the SmallVec
        // grew on the heap without limit. Bug №201: overflow is parked in a
        // spill list rather than a neighbouring cell, because a neighbouring
        // cell falls outside the range a query walks. `spill_cell` holds that
        // rule, and it is the ONLY placement path: a relocation is an insert
        // of the entity at its new position, so the two cannot drift.
        let cell = CellCoord::from_position(position);
        match self.spill_cell(cell) {
            // `entry`, not `get_mut`: a cell that does not exist yet has to be
            // created here, or the first insert of every cell misses the map and
            // lands in the spill list instead.
            Some(target) => self.cells.entry(target).or_default().push(entity),
            None => self.spilled.push(entity),
        }
        self.entity_cells.insert(id, cell);
    }

    /// Where an entity belonging to `cell` should be placed, or `None` when
    /// `cell` is full and the entity has to go to the spill list.
    ///
    /// Bug №632. This had two latent defects, both waiting for the first caller:
    ///
    /// - It walked the 27 neighbours, which is exactly the placement `insert`
    ///   rejected in №201 — a neighbour can sit outside the range a query walks,
    ///   so the entity becomes invisible to the query that centred on it. `insert`
    ///   parks overflow in the spill list for that reason.
    /// - When every neighbour was full it returned `cell` unchanged, so the
    ///   caller pushed into a cell already at `MAX_ENTITIES_PER_CELL` and the cap
    ///   evaporated for that cell.
    ///
    /// One rule now, shared by `insert` and by the relocation path: room in
    /// `cell` ⇒ `cell`, otherwise ⇒ spill list.
    fn spill_cell(&self, cell: CellCoord) -> Option<CellCoord> {
        if self.cells.get(&cell).map_or(0, |v| v.len()) < MAX_ENTITIES_PER_CELL {
            Some(cell)
        } else {
            None
        }
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
            // Bug №201: an entity parked in the spill list shares its cell
            // coordinate with the cell it overflowed, so the lookup above finds
            // nothing and the entity would survive its own removal — and keep
            // being returned by every query.
            if let Some(idx) = self.spilled.iter().position(|e| e.entity_id() == id) {
                return Some(self.spilled.swap_remove(idx));
            }
        }
        None
    }

    /// Every entity held in the spill list, for a query to consider.
    fn spilled_matching<'a>(
        &'a self,
        result: &mut SmallVec<[&'a T; 32]>,
        keep: impl Fn(&T) -> bool,
    ) {
        for entity in &self.spilled {
            if keep(entity) {
                result.push(entity);
            }
        }
    }

    pub fn query_radius(&self, center: Vec3f, radius: f32) -> SmallVec<[&T; 32]> {
        let mut result = SmallVec::new();
        // Bug №62: a non-finite center used to collapse into cell (0,0,0)
        // and walk nonsense ranges.
        if !radius.is_finite()
            || radius < 0.0
            || !center.x.is_finite()
            || !center.y.is_finite()
            || !center.z.is_finite()
        {
            return result;
        }
        #[cfg(test)]
        {
            self.walked_query_cells.set(0);
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
            self.spilled_matching(&mut result, |e| e.position().distance(center) <= radius);
            return result;
        }

        for x in min_cell.x..=max_cell.x {
            for y in min_cell.y..=max_cell.y {
                for z in min_cell.z..=max_cell.z {
                    let cell = CellCoord { x, y, z };
                    #[cfg(test)]
                    {
                        self.walked_query_cells
                            .set(self.walked_query_cells.get() + 1);
                    }
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
        // Bug №201: same as in `query_bounds` — the entity that overflowed a
        // full cell is not in any cell, so the walk above cannot reach it.
        self.spilled_matching(&mut result, |e| e.position().distance(center) <= radius);
        result
    }

    pub fn query_bounds(&self, bounds: &Bounds) -> SmallVec<[&T; 32]> {
        let mut result = SmallVec::new();
        // Bug №62: NaN bounds collapse to cell (0,0,0) and scan everything.
        if !bounds.min.x.is_finite()
            || !bounds.min.y.is_finite()
            || !bounds.min.z.is_finite()
            || !bounds.max.x.is_finite()
            || !bounds.max.y.is_finite()
            || !bounds.max.z.is_finite()
        {
            return result;
        }
        #[cfg(test)]
        {
            self.walked_query_cells.set(0);
        }
        let min_cell = CellCoord::from_position(bounds.min);
        let max_cell = CellCoord::from_position(bounds.max);

        // Bug №238: the same sparse-grid budget `query_radius` already runs.
        // A request whose AABB covers thousands of cells over a mostly-empty
        // grid pays one hash lookup per covered cell on the walking path, and
        // the walking path has no ceiling — an entity pushed outside the grid
        // (or a genuinely huge query) turns a routine per-tick cost into an
        // unbounded one. Once walking is more expensive than scanning every
        // live cell once, scan: the same neighbours + spill list, the same
        // intersects filter, linear in contents rather than in the volumes.
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
                    if entity.bounds().intersects(bounds) {
                        result.push(entity);
                    }
                }
            }
            self.spilled_matching(&mut result, |e| e.bounds().intersects(bounds));
            return result;
        }

        for x in min_cell.x..=max_cell.x {
            for y in min_cell.y..=max_cell.y {
                for z in min_cell.z..=max_cell.z {
                    let cell = CellCoord { x, y, z };
                    #[cfg(test)]
                    {
                        self.walked_query_cells
                            .set(self.walked_query_cells.get() + 1);
                    }
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
        // Bug №201: entities whose cell was full live in the spill list, not in
        // a neighbouring cell, so they have to be considered here explicitly.
        self.spilled_matching(&mut result, |e| e.bounds().intersects(bounds));
        result
    }

    pub fn get_cell_entities(
        &self,
        cell: CellCoord,
    ) -> Option<&SmallVec<[T; MAX_ENTITIES_PER_CELL]>> {
        self.cells.get(&cell)
    }

    pub fn entity_count(&self) -> usize {
        self.entity_cells.len()
    }

    pub fn all_entities(&self) -> Vec<T> {
        // Bug №603: the spill list was scanned by every spatial query but not
        // by this one, so an entity parked in it (its cell was full) never
        // reached `InterestManager::get_*` and vanished from client-bound
        // snapshots while still alive. `entity_cells` counts it, so the
        // capacity below already assumes it comes back.
        let mut result = Vec::with_capacity(self.entity_cells.len());
        for entities in self.cells.values() {
            result.extend(entities.iter().cloned());
        }
        result.extend(self.spilled.iter().cloned());
        result
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.entity_cells.clear();
        // Bug №632 (found while fixing it): `spilled` was left behind. With
        // `entity_cells` emptied, `remove` keys off nothing and can never find
        // those entities again, yet `all_entities` and every query still return
        // them — a cleared grid that keeps serving the previous one's overflow
        // forever. `clear` and `spilled` had been added separately and the new
        // field was never wired into the existing reset.
        self.spilled.clear();
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
        InterestLayer::SHIP => distance <= config.ship_radius,
        InterestLayer::COMPARTMENTS => {
            viewer_ship == target_ship && distance <= config.compartments_radius
        }
        InterestLayer::PLAYER => viewer_ship == target_ship && distance <= config.player_radius,
        InterestLayer::PROJECTILES => distance <= config.projectiles_radius,
        InterestLayer::ALL => true,
        _ => false,
    }
}
#[cfg(test)]
mod neighbour_tests {
    use super::*;

    /// Bug №237: `self.x + dx` on a saturated i32 wrapped in release (and
    /// panicked in debug), so a cell at the extreme produced "neighbours" on
    /// the far side of the grid.
    #[test]
    fn neighbours_do_not_overflow_at_the_coordinate_bounds() {
        for coord in [
            CellCoord {
                x: i32::MAX,
                y: i32::MAX,
                z: i32::MAX,
            },
            CellCoord {
                x: i32::MIN,
                y: i32::MIN,
                z: i32::MIN,
            },
            CellCoord {
                x: i32::MAX,
                y: 0,
                z: i32::MIN,
            },
        ] {
            let n = coord.neighbors();
            assert_eq!(n.len(), 27, "neighbour count must stay 27 for {coord:?}");
            for c in &n {
                // Saturation keeps the value at the bound instead of wrapping.
                assert!(
                    c.x == i32::MAX
                        || c.x == i32::MIN
                        || c.x == coord.x
                        || c.x == coord.x.saturating_add(1)
                        || c.x == coord.x.saturating_sub(1),
                    "x wrapped for {coord:?}: {c:?}"
                );
            }
            let n2 = coord.neighbors_2d();
            assert_eq!(n2.len(), 9);
            for c in &n2 {
                assert_eq!(c.y, coord.y, "2d neighbours must not move y");
            }
        }
    }

    #[test]
    fn ordinary_neighbours_are_unchanged() {
        let c = CellCoord { x: 5, y: 6, z: 7 };
        let n = c.neighbors();
        assert_eq!(n.len(), 27);
        assert!(n.contains(&c), "a cell is its own neighbour");
        assert!(n.contains(&CellCoord { x: 6, y: 6, z: 7 }));
        assert!(n.contains(&CellCoord { x: 4, y: 5, z: 6 }));
    }
}
#[cfg(test)]
mod spill_tests {
    use super::{Bounds, SpatialEntity, SpatialGrid, MAX_ENTITIES_PER_CELL, SPATIAL_CELL_SIZE};
    use crate::math::Vec3f;
    use crate::spatial::EntityId;

    /// Minimal entity: the grid only needs an id, a position and bounds.
    #[derive(Debug, Clone, PartialEq)]
    struct TestEntity {
        id: u64,
        pos: Vec3f,
        half: f32,
    }

    impl SpatialEntity for TestEntity {
        fn entity_id(&self) -> EntityId {
            EntityId::new(self.id)
        }
        fn entity_type(&self) -> crate::packet::EntityType {
            crate::packet::EntityType::Ship
        }
        fn position(&self) -> Vec3f {
            self.pos
        }
        fn bounds(&self) -> Bounds {
            Bounds::new(
                self.pos - Vec3f::new(self.half, self.half, self.half),
                self.pos + Vec3f::new(self.half, self.half, self.half),
            )
        }
    }

    fn entity(id: u64, x: f32, y: f32, z: f32) -> TestEntity {
        TestEntity {
            id,
            pos: Vec3f::new(x, y, z),
            half: 1.0,
        }
    }

    /// Bug №201: an entity that overflowed a full cell was parked in a
    /// neighbouring cell, so a query whose cell range covered only the entity's
    /// own cell never saw it. The query was centred exactly on the entity, so
    /// "filtering by true position" could not help — the entity was never
    /// reached to be filtered.
    #[test]
    fn an_entity_pushed_out_of_a_full_cell_is_still_found() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();

        // Fill one cell to the cap, all at the same position so they share a
        // cell and cannot spill away from each other.
        let home = Vec3f::new(100.0, 100.0, 100.0);
        for i in 0..MAX_ENTITIES_PER_CELL as u64 {
            grid.insert(entity(i, home.x, home.y, home.z));
        }
        // This one has to go somewhere else.
        let lost = MAX_ENTITIES_PER_CELL as u64;
        grid.insert(entity(lost, home.x, home.y, home.z));

        // A query centred exactly on the entity, with a radius far smaller than
        // the cell size, so its cell range is a single cell.
        let found = grid.query_radius(home, 1.0);
        assert!(
            found.iter().any(|e| e.entity_id() == EntityId::new(lost)),
            "the overflowing entity is invisible to a query centred on it: \
             {} of {} entities found",
            found.len(),
            MAX_ENTITIES_PER_CELL + 1
        );
        // And the ones that fitted are all there too.
        assert_eq!(
            found.len(),
            MAX_ENTITIES_PER_CELL + 1,
            "every entity is at the same position, so all must be returned"
        );
    }

    /// The cap still has to bound a cell, or the memory bound it was introduced
    /// for is gone.
    #[test]
    fn a_cell_never_grows_past_the_cap() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        for i in 0..(MAX_ENTITIES_PER_CELL as u64) * 3 {
            grid.insert(entity(i, 0.0, 0.0, 0.0));
        }
        let biggest = grid.cells.values().map(|c| c.len()).max().unwrap_or(0);
        assert!(
            biggest <= MAX_ENTITIES_PER_CELL,
            "a cell holds {biggest}, past the cap of {MAX_ENTITIES_PER_CELL}"
        );
    }

    /// A bounds query has to consider the spill list too.
    #[test]
    fn a_bounds_query_finds_an_overflowing_entity() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        for i in 0..(MAX_ENTITIES_PER_CELL as u64) + 1 {
            grid.insert(entity(i, 50.0, 50.0, 50.0));
        }
        let bounds = Bounds::new(Vec3f::new(49.0, 49.0, 49.0), Vec3f::new(51.0, 51.0, 51.0));
        let found = grid.query_bounds(&bounds);
        assert_eq!(
            found.len(),
            MAX_ENTITIES_PER_CELL + 1,
            "the overflowing entity is missing from the bounds query"
        );
    }

    /// `all_entities` is what the interest getters hand to the snapshot
    /// builder, so it must include the spill list — Bug №603: entities in an
    /// overflowing cell were alive, queryable, and simply absent from every
    /// client-bound snapshot.
    #[test]
    fn all_entities_includes_the_spill_list() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        let total = (MAX_ENTITIES_PER_CELL as u64) * 3;
        for i in 0..total {
            grid.insert(entity(i, 10.0, 10.0, 10.0));
        }
        assert!(
            !grid.spilled.is_empty(),
            "the fixture must actually overflow, or this proves nothing"
        );
        let all = grid.all_entities();
        assert_eq!(
            all.len() as u64,
            total,
            "all_entities returned {} of {} entities; the {} in the spill list \
             never reach a snapshot",
            all.len(),
            total,
            grid.spilled.len()
        );
        let unique: std::collections::HashSet<_> = all.iter().map(|e| e.entity_id()).collect();
        assert_eq!(
            unique.len() as u64,
            total,
            "all_entities duplicated an entity"
        );
    }

    /// An entity removed from the spill list must actually disappear, or it
    /// keeps being returned forever.
    #[test]
    fn removing_an_overflowing_entity_really_removes_it() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        for i in 0..(MAX_ENTITIES_PER_CELL as u64) + 1 {
            grid.insert(entity(i, 10.0, 10.0, 10.0));
        }
        let victim = EntityId::new(MAX_ENTITIES_PER_CELL as u64);
        assert!(
            grid.remove(victim).is_some(),
            "the overflowing entity must be removable"
        );
        let found = grid.query_radius(Vec3f::new(10.0, 10.0, 10.0), 1.0);
        assert!(
            !found.iter().any(|e| e.entity_id() == victim),
            "a removed entity is still being returned by queries"
        );
        assert_eq!(found.len(), MAX_ENTITIES_PER_CELL);
    }

    /// Re-inserting an entity that lives in the spill list must replace it, not
    /// append a second copy.
    ///
    /// Every server tick re-inserts each tracked entity, so an entity parked in
    /// the spill list is re-inserted 30 times a second. When the in-place
    /// replacement was missing, each of those updates appended another copy, so
    /// the spill list grew without bound and every radius query scanned all of
    /// it — quadratic cost that showed up only under load, as a server that
    /// missed its tick budget with 200 players on one ship.
    #[test]
    fn updating_a_spilled_entity_does_not_grow_the_spill_list() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        let overflow = MAX_ENTITIES_PER_CELL as u64;
        for i in 0..=overflow {
            grid.insert(entity(i, 10.0, 10.0, 10.0));
        }
        let before = grid.spilled.len();
        assert_eq!(before, 1, "exactly one entity should have overflowed");

        // Same cell, same id, moved a little: the replace-in-place path.
        for step in 1..=10u32 {
            grid.insert(entity(overflow, 10.0 + step as f32, 10.0, 10.0));
        }
        assert_eq!(
            grid.spilled.len(),
            before,
            "re-inserting a spilled entity grew the spill list"
        );

        let found = grid.query_radius(Vec3f::new(20.0, 10.0, 10.0), 1000.0);
        let copies = found.iter().filter(|e| e.entity_id().0 == overflow).count();
        assert_eq!(
            copies, 1,
            "the spilled entity was duplicated: {copies} copies"
        );
    }

    /// The same leak, driven through the real per-tick pattern that exposed it:
    /// many entities sharing one cell, re-inserted every tick.
    #[test]
    fn per_tick_reinsertion_of_a_crowded_cell_stays_bounded() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        let total = MAX_ENTITIES_PER_CELL as u64 * 3;
        for i in 0..total {
            grid.insert(entity(i, 10.0, 10.0, 10.0));
        }
        let expected_spilled = total - MAX_ENTITIES_PER_CELL as u64;
        assert_eq!(grid.spilled.len() as u64, expected_spilled);

        for _ in 0..50 {
            for i in 0..total {
                grid.insert(entity(i, 10.0, 10.0, 10.0));
            }
        }
        assert_eq!(
            grid.spilled.len() as u64,
            expected_spilled,
            "50 ticks of re-insertion grew the spill list from {expected_spilled} to {}",
            grid.spilled.len()
        );
        assert_eq!(
            grid.entity_cells.len() as u64,
            total,
            "re-insertion lost track of an entity"
        );
    }

    /// The centre of `cell`, so the tests below can address a cell without
    /// depending on the arithmetic `from_position` uses.
    fn cell_centre(x: i32, y: i32, z: i32) -> Vec3f {
        Vec3f::new(
            x as f32 * SPATIAL_CELL_SIZE + SPATIAL_CELL_SIZE * 0.5,
            y as f32 * SPATIAL_CELL_SIZE + SPATIAL_CELL_SIZE * 0.5,
            z as f32 * SPATIAL_CELL_SIZE + SPATIAL_CELL_SIZE * 0.5,
        )
    }

    /// Bug №632: a position change has to leave the entity findable at its new
    /// position and nowhere at the old one. The path it took to get there was
    /// the defect — the dead `update_position` used to move the *cell* while
    /// the stored snapshot kept reporting its old `position()`, so queries
    /// filtered the entity out of the cell it had just been placed in and it
    /// was findable neither there nor at home. The method is gone now: a
    /// relocation IS an insert of the entity at its new position, and that is
    /// what the live path (`InterestManager::update_player` and friends) has
    /// always done. This test pins the behaviour, not the removed method.
    #[test]
    fn moving_an_entity_away_from_its_cell_finds_it_at_the_new_position() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        grid.insert(entity(1, 10.0, 10.0, 10.0));

        let new_pos = cell_centre(5, 0, 0);
        grid.insert(entity(1, new_pos.x, new_pos.y, new_pos.z));

        assert_eq!(
            grid.query_radius(Vec3f::new(10.0, 10.0, 10.0), 1.0).len(),
            0,
            "the entity is still found at its old position"
        );
        let found = grid.query_radius(new_pos, 1.0);
        assert_eq!(
            found.len(),
            1,
            "the entity is not found at its new position"
        );
        assert_eq!(found[0].entity_id(), EntityId::new(1));
    }

    /// An entity parked in the spill list has to relocate like any other. The
    /// path that used to exist here looked only *inside* the recorded cell —
    /// where a spilled entity by definition is not — found nothing, and refused
    /// the move. So the entity that most needed moving (its cell was full) was
    /// precisely the one guaranteed not to move. The two halves are worth
    /// separate tests: reaching the entity at all, and leaving no ghost behind.
    #[test]
    fn an_overflowed_entity_can_still_be_moved() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        let home = cell_centre(0, 0, 0);
        // `MAX + 1` entities in one cell, so exactly the last lands in the spill.
        for i in 0..=MAX_ENTITIES_PER_CELL as u64 {
            grid.insert(entity(i, home.x, home.y, home.z));
        }
        assert_eq!(
            grid.spilled.len(),
            1,
            "the fixture must overflow or this proves nothing"
        );

        let spilled_id = EntityId::new(MAX_ENTITIES_PER_CELL as u64);
        let new_pos = cell_centre(5, 0, 0);
        grid.insert(entity(spilled_id.0, new_pos.x, new_pos.y, new_pos.z));
        assert_eq!(
            grid.spilled.len(),
            0,
            "the entity is still parked after moving"
        );

        let at_new = grid.query_radius(new_pos, 1.0);
        assert_eq!(
            at_new.len(),
            1,
            "the moved entity is not at its new position"
        );
        assert_eq!(at_new[0].entity_id(), spilled_id);

        let at_old = grid.query_radius(home, 1.0);
        assert_eq!(
            at_old.len(),
            MAX_ENTITIES_PER_CELL,
            "the cell it left still holds {} of {} entities",
            at_old.len(),
            MAX_ENTITIES_PER_CELL
        );
        assert!(
            !at_old.iter().any(|e| e.entity_id() == spilled_id),
            "the moved entity still answers to its old cell"
        );
    }

    /// The second half of №632: `spill_cell` walked the neighbours and, when
    /// they were *all* full, returned the original cell — so the caller pushed
    /// an entity into a cell already at the cap. Parking the entity in the
    /// spill list is what `insert` has always done (№201), and it is what a
    /// query centred on the entity can still reach. The neighbours no longer
    /// enter into it at all — `spill_cell` never looks at them — so the fixture
    /// below fills them only to pin down the scenario the old code got wrong.
    #[test]
    fn moving_into_a_cell_with_no_room_anywhere_does_not_break_the_cap() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        let target = cell_centre(9, 0, 0);

        // Fill the destination cell and all 26 of its neighbours, so no empty
        // cell is left to divert into.
        let mut next_id: u64 = 0;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let pos = cell_centre(9 + dx, dy, dz);
                    for _ in 0..MAX_ENTITIES_PER_CELL as u64 {
                        grid.insert(entity(next_id, pos.x, pos.y, pos.z));
                        next_id += 1;
                    }
                }
            }
        }
        assert_eq!(
            grid.cells.len(),
            27,
            "the fixture should have filled the destination and its neighbours"
        );

        // The mover comes from a cell that does have room.
        let home = cell_centre(0, 0, 0);
        let mover_id = EntityId::new(next_id);
        grid.insert(entity(mover_id.0, home.x, home.y, home.z));

        grid.insert(entity(mover_id.0, target.x, target.y, target.z));

        let biggest = grid.cells.values().map(|c| c.len()).max().unwrap_or(0);
        assert!(
            biggest <= MAX_ENTITIES_PER_CELL,
            "a cell holds {biggest}, past the cap of {MAX_ENTITIES_PER_CELL}"
        );
        assert_eq!(
            grid.spilled.len(),
            1,
            "the mover should be parked in the spill list"
        );

        let found = grid.query_radius(target, 1.0);
        assert!(
            found.iter().any(|e| e.entity_id() == mover_id),
            "the mover is unreachable from a query centred on it: {} entities",
            found.len()
        );
    }

    /// `clear` emptied `cells` and `entity_cells` but not `spilled`. Since
    /// `remove` keys off `entity_cells`, those entities could never be removed
    /// again — yet `all_entities` and every query kept returning them, so a
    /// cleared grid went on serving the previous one's overflow forever.
    #[test]
    fn clearing_a_grid_forgets_its_spill_list() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        let home = Vec3f::new(10.0, 10.0, 10.0);
        for i in 0..=MAX_ENTITIES_PER_CELL as u64 {
            grid.insert(entity(i, home.x, home.y, home.z));
        }
        assert_eq!(grid.spilled.len(), 1, "the fixture must overflow");

        grid.clear();

        assert_eq!(
            grid.entity_count(),
            0,
            "the grid still counts cleared entities"
        );
        assert_eq!(
            grid.spilled.len(),
            0,
            "clear() left {} spilled entities behind",
            grid.spilled.len()
        );
        assert_eq!(
            grid.query_radius(home, 10.0).len(),
            0,
            "a cleared grid still answers queries"
        );
        assert_eq!(
            grid.all_entities().len(),
            0,
            "a cleared grid still hands out entities"
        );
    }
}

#[cfg(test)]
mod bounds_scan_tests {
    use super::{Bounds, SpatialEntity, SpatialGrid};
    use crate::math::Vec3f;
    use crate::spatial::EntityId;

    /// Minimal entity: the grid only needs an id, a position and bounds.
    #[derive(Debug, Clone, PartialEq)]
    struct TestEntity {
        id: u64,
        pos: Vec3f,
        half: f32,
    }

    impl SpatialEntity for TestEntity {
        fn entity_id(&self) -> EntityId {
            EntityId::new(self.id)
        }
        fn entity_type(&self) -> crate::packet::EntityType {
            crate::packet::EntityType::Ship
        }
        fn position(&self) -> Vec3f {
            self.pos
        }
        fn bounds(&self) -> Bounds {
            Bounds::new(
                self.pos - Vec3f::new(self.half, self.half, self.half),
                self.pos + Vec3f::new(self.half, self.half, self.half),
            )
        }
    }

    fn entity(id: u64, x: f32, y: f32, z: f32) -> TestEntity {
        TestEntity {
            id,
            pos: Vec3f::new(x, y, z),
            half: 1.0,
        }
    }

    /// Bug №238: a huge AABB over a sparse grid must take the same budget
    /// `query_radius` already runs — once walking the covered cells costs
    /// more than one scan of the live cells, scan instead. The result is
    /// identical either way; the point is that the walk stays bounded.
    #[test]
    fn a_huge_bounds_request_scans_instead_of_walking() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        grid.insert(entity(1, 100.0, 0.0, 0.0)); // cell (0,0,0)
        grid.insert(entity(2, 1100.0, 0.0, 0.0)); // cell (1,0,0)
        grid.insert(entity(3, 3500.0, 2100.0, 200.0)); // cell (3,2,0)

        // 10×10 positions span 11 cells along each axis (floor(y/1000)
        // picks up an extra cell at the 9000 boundary): 11×11×11 = 1331
        // covered cells over three live cells. Threshold: 3×8+64 = 88 —
        // the walking branch would visit 1331 cells for one hash lookup
        // each, so the request has no business taking it.
        let bounds = Bounds::new(
            Vec3f::new(-100.0, -100.0, -100.0),
            Vec3f::new(9900.0, 9900.0, 9900.0),
        );
        let found = grid.query_bounds(&bounds);
        assert_eq!(found.len(), 3, "all three live entities are inside");
        assert_eq!(
            grid.walked_query_cells(),
            0,
            "the huge request must scan, not walk the 1331 covered cells"
        );
    }

    /// The walking branch is still the live path for small requests — the
    /// budget must not switch a routine query to the scan.
    #[test]
    fn a_small_bounds_request_walks_exactly_its_cells() {
        let mut grid: SpatialGrid<TestEntity> = SpatialGrid::new();
        grid.insert(entity(1, 100.0, 100.0, 100.0));

        // Cells floor(-1000/1000) = -1 .. floor(1999/1000) = 1 →
        // (-1..=1)³ = 27 covered cells for one live cell; threshold 8+64 =
        // 72 keeps the walk in charge.
        let bounds = Bounds::new(
            Vec3f::new(-1000.0, -1000.0, -1000.0),
            Vec3f::new(1999.0, 1999.0, 1999.0),
        );
        let found = grid.query_bounds(&bounds);
        assert_eq!(found.len(), 1);
        assert_eq!(
            grid.walked_query_cells(),
            27,
            "a 27-cell request walks 27 cells, no more and no fewer"
        );
    }
}
