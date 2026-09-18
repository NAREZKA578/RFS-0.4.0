use rfs_core::math::{Vec3f, Bounds};
use rfs_core::spatial::*;
use rfs_core::entity::{SpatialEntity, EntityType};

#[derive(Clone, Debug)]
struct MockEntity {
    id: EntityId,
    pos: Vec3f,
    half: Vec3f,
    etype: EntityType,
}

impl SpatialEntity for MockEntity {
    fn entity_id(&self) -> EntityId { self.id }
    fn position(&self) -> Vec3f { self.pos }
    fn bounds(&self) -> Bounds {
        Bounds::new(self.pos - self.half, self.pos + self.half)
    }
    fn entity_type(&self) -> EntityType { self.etype }
}

fn mock(id: u64, pos: Vec3f) -> MockEntity {
    MockEntity {
        id: EntityId::new(id),
        pos,
        half: Vec3f::new(5.0, 5.0, 5.0),
        etype: EntityType::Ship,
    }
}

// ---------- CellCoord::from_position ----------

#[test]
fn cell_coord_positive_origin() {
    let c = CellCoord::from_position(Vec3f::new(500.0, 1500.0, 2500.0));
    assert_eq!(c, CellCoord { x: 0, y: 1, z: 2 });
}

#[test]
fn cell_coord_exact_multiple() {
    let c = CellCoord::from_position(Vec3f::new(1000.0, 2000.0, 3000.0));
    assert_eq!(c, CellCoord { x: 1, y: 2, z: 3 });
}

#[test]
fn cell_coord_negative() {
    let c = CellCoord::from_position(Vec3f::new(-500.0, -1500.0, -2500.0));
    assert_eq!(c, CellCoord { x: -1, y: -2, z: -3 });
}

#[test]
fn cell_coord_negative_exact() {
    let c = CellCoord::from_position(Vec3f::new(-1000.0, -2000.0, -3000.0));
    assert_eq!(c, CellCoord { x: -1, y: -2, z: -3 });
}

#[test]
fn cell_coord_zero() {
    let c = CellCoord::from_position(Vec3f::ZERO);
    assert_eq!(c, CellCoord { x: 0, y: 0, z: 0 });
}

#[test]
fn cell_coord_just_below_boundary() {
    let c = CellCoord::from_position(Vec3f::new(999.9, -0.1, -1000.1));
    assert_eq!(c, CellCoord { x: 0, y: -1, z: -2 });
}

// ---------- CellCoord::neighbors / neighbors_2d ----------

#[test]
fn neighbors_returns_27() {
    let c = CellCoord { x: 5, y: 5, z: 5 };
    let n = c.neighbors();
    assert_eq!(n.len(), 27);
}

#[test]
fn neighbors_2d_returns_9() {
    let c = CellCoord { x: 5, y: 5, z: 5 };
    let n = c.neighbors_2d();
    assert_eq!(n.len(), 9);
}

#[test]
fn neighbors_2d_y_unchanged() {
    let c = CellCoord { x: 0, y: 10, z: 0 };
    for n in c.neighbors_2d() {
        assert_eq!(n.y, 10);
    }
}

#[test]
fn neighbors_includes_self() {
    let c = CellCoord { x: 3, y: 7, z: -2 };
    let n = c.neighbors();
    assert!(n.contains(&c));
}

// ---------- EntityId ----------

#[test]
fn entity_id_new() {
    let id = EntityId::new(42);
    assert_eq!(id.0, 42);
}

#[test]
fn entity_id_nil() {
    let id = EntityId::nil();
    assert!(id.is_nil());
    assert_eq!(id.0, 0);
}

#[test]
fn entity_id_non_nil() {
    let id = EntityId::new(1);
    assert!(!id.is_nil());
}

// ---------- SpatialGrid insert / entity_count / cell_count ----------

#[test]
fn grid_insert_and_count() {
    let mut grid = SpatialGrid::new();
    assert_eq!(grid.entity_count(), 0);
    assert_eq!(grid.cell_count(), 0);

    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));
    assert_eq!(grid.entity_count(), 1);
    assert_eq!(grid.cell_count(), 1);

    grid.insert(mock(2, Vec3f::new(5000.0, 5000.0, 5000.0)));
    assert_eq!(grid.entity_count(), 2);
    assert_eq!(grid.cell_count(), 2);
}

#[test]
fn grid_insert_same_id_updates() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));
    assert_eq!(grid.entity_count(), 1);

    grid.insert(mock(1, Vec3f::new(200.0, 200.0, 200.0)));
    assert_eq!(grid.entity_count(), 1);
    assert_eq!(grid.cell_count(), 1);
}

// ---------- SpatialGrid remove ----------

#[test]
fn grid_remove() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));
    grid.insert(mock(2, Vec3f::new(100.0, 100.0, 100.0)));
    assert_eq!(grid.entity_count(), 2);

    let removed = grid.remove(EntityId::new(1));
    assert!(removed.is_some());
    assert_eq!(grid.entity_count(), 1);
}

#[test]
fn grid_remove_cleans_empty_cell() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));
    assert_eq!(grid.cell_count(), 1);

    grid.remove(EntityId::new(1));
    assert_eq!(grid.cell_count(), 0);
    assert_eq!(grid.entity_count(), 0);
}

#[test]
fn grid_remove_nonexistent() {
    let mut grid: SpatialGrid<MockEntity> = SpatialGrid::new();
    let removed = grid.remove(EntityId::new(999));
    assert!(removed.is_none());
}

// ---------- SpatialGrid update_position ----------

#[test]
fn grid_update_position_moves_entity() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));
    let cell_before = CellCoord::from_position(Vec3f::new(100.0, 100.0, 100.0));

    let ok = grid.update_position(EntityId::new(1), Vec3f::new(5000.0, 5000.0, 5000.0));
    assert!(ok);

    let cell_after = CellCoord::from_position(Vec3f::new(5000.0, 5000.0, 5000.0));
    assert_ne!(cell_before, cell_after);
    assert_eq!(grid.entity_count(), 1);

    let cell_info = grid.get_cell_entities(cell_after);
    assert!(cell_info.is_some());
    assert_eq!(cell_info.unwrap().len(), 1);
}

#[test]
fn grid_update_position_nan_returns_false() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));

    let ok = grid.update_position(EntityId::new(1), Vec3f::new(f32::NAN, 0.0, 0.0));
    assert!(!ok);
}

#[test]
fn grid_update_position_same_cell() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));
    let ok = grid.update_position(EntityId::new(1), Vec3f::new(101.0, 102.0, 103.0));
    assert!(ok);
    assert_eq!(grid.entity_count(), 1);
    assert_eq!(grid.cell_count(), 1);
}

#[test]
fn grid_update_position_nonexistent() {
    let mut grid: SpatialGrid<MockEntity> = SpatialGrid::new();
    let ok = grid.update_position(EntityId::new(999), Vec3f::ZERO);
    assert!(!ok);
}

// ---------- SpatialGrid query_radius ----------

#[test]
fn query_radius_hit() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));
    grid.insert(mock(2, Vec3f::new(5000.0, 5000.0, 5000.0)));

    let results = grid.query_radius(Vec3f::new(100.0, 100.0, 100.0), 200.0);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entity_id(), EntityId::new(1));
}

#[test]
fn query_radius_filters_far_entities() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(0.0, 0.0, 0.0)));
    grid.insert(mock(2, Vec3f::new(5000.0, 5000.0, 5000.0)));

    let results = grid.query_radius(Vec3f::new(0.0, 0.0, 0.0), 1.0);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entity_id(), EntityId::new(1));
}

#[test]
fn query_radius_negative_returns_empty() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::ZERO));
    let results = grid.query_radius(Vec3f::ZERO, -1.0);
    assert_eq!(results.len(), 0);
}

#[test]
fn query_radius_nan_center_returns_empty() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::ZERO));
    let results = grid.query_radius(Vec3f::new(f32::NAN, 0.0, 0.0), 100.0);
    assert_eq!(results.len(), 0);
}

#[test]
fn query_radius_inf_radius_returns_empty() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::ZERO));
    let results = grid.query_radius(Vec3f::ZERO, f32::INFINITY);
    assert_eq!(results.len(), 0);
}

// ---------- SpatialGrid query_bounds ----------

#[test]
fn query_bounds_overlap() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));

    let q = Bounds::new(Vec3f::new(90.0, 90.0, 90.0), Vec3f::new(110.0, 110.0, 110.0));
    let results = grid.query_bounds(&q);
    assert_eq!(results.len(), 1);
}

#[test]
fn query_bounds_no_overlap() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));

    let q = Bounds::new(Vec3f::new(5000.0, 5000.0, 5000.0), Vec3f::new(5010.0, 5010.0, 5010.0));
    let results = grid.query_bounds(&q);
    assert_eq!(results.len(), 0);
}

#[test]
fn query_bounds_nan_returns_empty() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::ZERO));
    let q = Bounds::new(Vec3f::new(f32::NAN, 0.0, 0.0), Vec3f::new(1.0, 1.0, 1.0));
    let results = grid.query_bounds(&q);
    assert_eq!(results.len(), 0);
}

// ---------- SpatialGrid all_entities ----------

#[test]
fn all_entities() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::new(100.0, 100.0, 100.0)));
    grid.insert(mock(2, Vec3f::new(5000.0, 5000.0, 5000.0)));
    grid.insert(mock(3, Vec3f::new(-100.0, -100.0, -100.0)));

    let all = grid.all_entities();
    assert_eq!(all.len(), 3);
}

// ---------- SpatialGrid clear ----------

#[test]
fn grid_clear() {
    let mut grid = SpatialGrid::new();
    grid.insert(mock(1, Vec3f::ZERO));
    grid.insert(mock(2, Vec3f::new(5000.0, 0.0, 0.0)));
    grid.clear();
    assert_eq!(grid.entity_count(), 0);
    assert_eq!(grid.cell_count(), 0);
}

// ---------- calculate_interest_mask ----------

#[test]
fn interest_ship_within_radius() {
    let cfg = InterestConfig::default();
    let same = EntityId::new(1);
    let other = EntityId::new(2);
    assert!(calculate_interest_mask(
        Vec3f::ZERO, same,
        Vec3f::new(100.0, 0.0, 0.0), other,
        InterestLayer::SHIP, &cfg,
    ));
}

#[test]
fn interest_ship_outside_radius() {
    let cfg = InterestConfig::default();
    let same = EntityId::new(1);
    let other = EntityId::new(2);
    assert!(!calculate_interest_mask(
        Vec3f::ZERO, same,
        Vec3f::new(10000.0, 0.0, 0.0), other,
        InterestLayer::SHIP, &cfg,
    ));
}

#[test]
fn interest_compartments_same_ship_within_radius() {
    let cfg = InterestConfig::default();
    let ship = EntityId::new(1);
    assert!(calculate_interest_mask(
        Vec3f::ZERO, ship,
        Vec3f::new(50.0, 0.0, 0.0), ship,
        InterestLayer::COMPARTMENTS, &cfg,
    ));
}

#[test]
fn interest_compartments_different_ship() {
    let cfg = InterestConfig::default();
    let ship_a = EntityId::new(1);
    let ship_b = EntityId::new(2);
    assert!(!calculate_interest_mask(
        Vec3f::ZERO, ship_a,
        Vec3f::new(50.0, 0.0, 0.0), ship_b,
        InterestLayer::COMPARTMENTS, &cfg,
    ));
}

#[test]
fn interest_player_same_ship_within_radius() {
    let cfg = InterestConfig::default();
    let ship = EntityId::new(1);
    assert!(calculate_interest_mask(
        Vec3f::ZERO, ship,
        Vec3f::new(50.0, 0.0, 0.0), ship,
        InterestLayer::PLAYER, &cfg,
    ));
}

#[test]
fn interest_player_different_ship() {
    let cfg = InterestConfig::default();
    let ship_a = EntityId::new(1);
    let ship_b = EntityId::new(2);
    assert!(!calculate_interest_mask(
        Vec3f::ZERO, ship_a,
        Vec3f::new(50.0, 0.0, 0.0), ship_b,
        InterestLayer::PLAYER, &cfg,
    ));
}

#[test]
fn interest_all_always_true() {
    let cfg = InterestConfig::default();
    let ship = EntityId::new(1);
    let other = EntityId::new(99);
    assert!(calculate_interest_mask(
        Vec3f::ZERO, ship,
        Vec3f::new(999999.0, 0.0, 0.0), other,
        InterestLayer::ALL, &cfg,
    ));
}
