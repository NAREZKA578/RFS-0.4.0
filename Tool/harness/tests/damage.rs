// Moved out of crates/damage/src/{damage,graph,propagation}.rs (side-only testing rule).
use rfs_core::entity::EntityId;
use rfs_core::math::{Bounds, Vec3f};
use rfs_core::time::FIXED_DT;
use rfs_damage::damage::{CompartmentHit, DamageSystem, DamageTarget};
use rfs_damage::graph::{BulkheadEdge, CompartmentNode, DamageGraph};
use rfs_damage::propagation::{
    PropagationConfig, PropagationEventType, PropagationSystem,
};
use std::cell::RefCell;

struct TestTarget {
    entity_id: EntityId,
    hits: Vec<CompartmentHit>,
    health: RefCell<f32>,
    damaged: RefCell<Vec<(EntityId, f32)>>,
}

impl TestTarget {
    fn new() -> Self {
        Self {
            entity_id: EntityId::new(1),
            hits: vec![CompartmentHit {
                entity_id: EntityId::new(1001),
                name: "fore".to_string(),
                bounds: Bounds::new(Vec3f::new(-5.0, -5.0, -5.0), Vec3f::new(5.0, 5.0, 5.0)),
                max_water_level: 100.0,
                pump_capacity: 10.0,
            }],
            health: RefCell::new(1000.0),
            damaged: RefCell::new(Vec::new()),
        }
    }
}

impl DamageTarget for TestTarget {
    fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    fn compartment_hits(&self) -> Vec<CompartmentHit> {
        self.hits.clone()
    }

    fn apply_health_damage(&self, amount: f32) {
        let mut health = self.health.borrow_mut();
        *health = (*health - amount).max(0.0);
    }

    fn apply_compartment_damage(&self, compartment_id: EntityId, damage: f32, _position: Vec3f) {
        self.damaged.borrow_mut().push((compartment_id, damage));
    }
}

#[test]
fn test_apply_damage() {
    let system = DamageSystem::new();
    let target = TestTarget::new();
    system.apply_damage_to_ship(&target, Vec3f::ZERO, 100.0);
    assert_eq!(*target.health.borrow(), 900.0);
    assert_eq!(target.damaged.borrow().len(), 1);
}

#[test]
fn test_damage_falloff_outside_radius() {
    let system = DamageSystem::new();
    let target = TestTarget::new();
    system.apply_damage_to_ship(&target, Vec3f::new(100.0, 100.0, 100.0), 100.0);
    assert_eq!(*target.health.borrow(), 900.0);
    assert_eq!(target.damaged.borrow().len(), 0);
}

#[test]
fn test_graph_creation() {
    let mut graph = DamageGraph::new();

    let comp1 = CompartmentNode::new(EntityId::new(1), "fore".to_string(), 100.0, 10.0);
    let comp2 = CompartmentNode::new(EntityId::new(2), "aft".to_string(), 100.0, 10.0);

    graph.add_compartment(comp1);
    graph.add_compartment(comp2);

    let bulkhead = BulkheadEdge::new(EntityId::new(100), EntityId::new(1), EntityId::new(2), 500.0);
    graph.add_bulkhead(bulkhead);

    assert_eq!(graph.compartment_count(), 2);
    assert_eq!(graph.bulkhead_count(), 1);
}

#[test]
fn test_water_flow() {
    let mut graph = DamageGraph::new();

    let mut comp1 = CompartmentNode::new(EntityId::new(1), "fore".to_string(), 100.0, 10.0);
    comp1.current_level = 50.0;
    comp1.is_sealed = false;

    let comp2 = CompartmentNode::new(EntityId::new(2), "aft".to_string(), 100.0, 10.0);

    graph.add_compartment(comp1);
    graph.add_compartment(comp2);

    let bulkhead = BulkheadEdge::new(EntityId::new(100), EntityId::new(1), EntityId::new(2), 500.0);
    graph.add_bulkhead(bulkhead);

    graph.unseal_bulkhead(EntityId::new(100));

    assert!(graph.get_bulkhead(EntityId::new(100)).unwrap().is_passable());
}

#[test]
fn test_water_propagation() {
    let mut graph = DamageGraph::new();

    let mut comp1 = CompartmentNode::new(EntityId::new(1), "fore".to_string(), 100.0, 10.0);
    comp1.current_level = 50.0;
    comp1.is_sealed = false;
    comp1.is_breached = true;

    let comp2 = CompartmentNode::new(EntityId::new(2), "aft".to_string(), 100.0, 10.0);

    graph.add_compartment(comp1);
    graph.add_compartment(comp2);

    let bulkhead = BulkheadEdge::new(EntityId::new(100), EntityId::new(1), EntityId::new(2), 500.0);
    graph.add_bulkhead(bulkhead);
    graph.unseal_bulkhead(EntityId::new(100));

    let config = PropagationConfig::default();
    let mut prop = PropagationSystem::new(config);

    prop.update(&mut graph, FIXED_DT);

    let comp2_after = graph.get_compartment(EntityId::new(2)).unwrap();

    assert!(comp2_after.current_level > 0.0);
}

#[test]
fn test_fire_spread() {
    let mut graph = DamageGraph::new();

    let mut comp1 = CompartmentNode::new(EntityId::new(1), "fore".to_string(), 100.0, 10.0);
    comp1.fire_intensity = 0.8;
    comp1.is_sealed = false;

    let comp2 = CompartmentNode::new(EntityId::new(2), "aft".to_string(), 100.0, 10.0);

    graph.add_compartment(comp1);
    graph.add_compartment(comp2);

    let bulkhead = BulkheadEdge::new(EntityId::new(100), EntityId::new(1), EntityId::new(2), 500.0);
    graph.add_bulkhead(bulkhead);
    graph.unseal_bulkhead(EntityId::new(100));

    let config = PropagationConfig {
        fire_spread_rate: 100.0,
        ..PropagationConfig::default()
    };
    let mut prop = PropagationSystem::new(config);

    let events = prop.update(&mut graph, FIXED_DT);

    let comp2_after = graph.get_compartment(EntityId::new(2)).unwrap();
    assert!(
        comp2_after.fire_intensity > 0.0
            || events
                .iter()
                .any(|e| e.event_type == PropagationEventType::FireSpread)
    );
}
