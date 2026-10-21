use rfs_core::entity::EntityId;
use rfs_damage::graph::{DamageGraph, CompartmentNode, BulkheadEdge};
use rfs_damage::propagation::*;

fn comp_id(n: u64) -> EntityId { EntityId::new(n) }

fn build_graph_two_compartments() -> (DamageGraph, EntityId, EntityId, EntityId) {
    let id_a = comp_id(1);
    let id_b = comp_id(2);
    let bh_id = comp_id(100);

    let mut graph = DamageGraph::new();
    let a = CompartmentNode::new(id_a, "A".into(), 100.0, 10.0);
    let b = CompartmentNode::new(id_b, "B".into(), 100.0, 10.0);
    graph.add_compartment(a);
    graph.add_compartment(b);

    let bh = BulkheadEdge::new(bh_id, id_a, id_b, 100.0);
    graph.add_bulkhead(bh);

    (graph, id_a, id_b, bh_id)
}

// ---------- PropagationSystem::new + update ----------

#[test]
fn propagation_system_update_on_empty_graph() {
    let mut graph = DamageGraph::new();
    let mut sys = PropagationSystem::new(PropagationConfig::default());
    let events = sys.update(&mut graph, 1.0 / 30.0);
    assert!(events.is_empty());
}

#[test]
fn propagation_system_update_with_compartments() {
    let (mut graph, _id_a, _id_b, _bh_id) = build_graph_two_compartments();
    let mut sys = PropagationSystem::new(PropagationConfig::default());
    let events = sys.update(&mut graph, 1.0 / 30.0);
    assert!(events.is_empty());
}

// ---------- breach_compartment + water intake ----------

#[test]
fn breach_and_water_intake() {
    let (mut graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    let mut sys = PropagationSystem::new(PropagationConfig::default());

    let breached = sys.breach_compartment(&mut graph, id_a);
    assert!(breached);

    let events = sys.update(&mut graph, 1.0);
    let intake: Vec<_> = events.iter()
        .filter(|e| e.event_type == PropagationEventType::WaterIntake)
        .collect();
    assert!(!intake.is_empty());
    assert!(intake[0].amount > 0.0);
}

#[test]
fn water_level_rises_over_time() {
    let (mut graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    let mut sys = PropagationSystem::new(PropagationConfig::default());
    sys.breach_compartment(&mut graph, id_a);

    sys.update(&mut graph, 1.0);
    let level1 = graph.get_compartment(id_a).unwrap().current_level;
    assert!(level1 > 0.0);

    sys.update(&mut graph, 1.0);
    let level2 = graph.get_compartment(id_a).unwrap().current_level;
    assert!(level2 > level1);
}

// ---------- ignite_fire + fire grows ----------

#[test]
fn ignite_fire() {
    let (mut graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    let mut sys = PropagationSystem::new(PropagationConfig::default());

    let ignited = sys.ignite_fire(&mut graph, id_a, 0.5);
    assert!(ignited);
    assert!((graph.get_compartment(id_a).unwrap().fire_intensity - 0.5).abs() < 0.001);
}

#[test]
fn fire_grows_over_time() {
    let (mut graph, id_a, id_b, _bh_id) = build_graph_two_compartments();
    {
        let bh = graph.get_bulkhead_mut(comp_id(100)).unwrap();
        bh.is_sealed = false;
    }

    let mut sys = PropagationSystem::new(PropagationConfig::default());
    sys.ignite_fire(&mut graph, id_a, 0.5);

    sys.update(&mut graph, 1.0);
    let first = graph.get_compartment(id_b).unwrap().fire_intensity;
    assert!(first > 0.0);

    sys.update(&mut graph, 1.0);
    let second = graph.get_compartment(id_b).unwrap().fire_intensity;
    assert!(second > first);
}

// ---------- extinguish_fire ----------

#[test]
fn extinguish_fire() {
    let (mut graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    let mut sys = PropagationSystem::new(PropagationConfig::default());
    sys.ignite_fire(&mut graph, id_a, 0.8);

    let ext = sys.extinguish_fire(&mut graph, id_a);
    assert!(ext);
    assert!((graph.get_compartment(id_a).unwrap().fire_intensity).abs() < 0.001);
}

// ---------- CompartmentFlooded edge-triggered ----------

#[test]
fn flooded_event_fires_once() {
    let (mut graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    graph.breach_compartment(id_a);

    // Fill past the flood threshold manually
    graph.add_water(id_a, 95.0);

    let mut sys = PropagationSystem::new(PropagationConfig::default());
    let events1 = sys.update(&mut graph, 1.0 / 30.0);
    let flooded1: Vec<_> = events1.iter()
        .filter(|e| e.event_type == PropagationEventType::CompartmentFlooded)
        .collect();
    assert_eq!(flooded1.len(), 1);

    let events2 = sys.update(&mut graph, 1.0 / 30.0);
    let flooded2: Vec<_> = events2.iter()
        .filter(|e| e.event_type == PropagationEventType::CompartmentFlooded)
        .collect();
    assert!(flooded2.is_empty());
}

// ---------- is_ship_lost ----------

#[test]
fn is_ship_lost_empty_graph() {
    let graph = DamageGraph::new();
    let sim = FloodingSimulator::new(graph, PropagationConfig::default());
    assert!(!sim.is_ship_lost());
}

#[test]
fn is_ship_lost_above_threshold() {
    let (mut graph, id_a, id_b, _bh_id) = build_graph_two_compartments();
    graph.add_water(id_a, 100.0);
    graph.add_water(id_b, 45.0);
    let sim = FloodingSimulator::new(graph, PropagationConfig::default());
    assert!(sim.is_ship_lost());
}

#[test]
fn is_ship_lost_exactly_70_percent_not_lost() {
    let (mut graph, id_a, id_b, _bh_id) = build_graph_two_compartments();
    graph.add_water(id_a, 100.0);
    graph.add_water(id_b, 40.0);
    let sim = FloodingSimulator::new(graph, PropagationConfig::default());
    assert!(!sim.is_ship_lost());
}

#[test]
fn is_ship_lost_below_threshold() {
    let (mut graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    graph.add_water(id_a, 50.0);
    let sim = FloodingSimulator::new(graph, PropagationConfig::default());
    assert!(!sim.is_ship_lost());
}

// ---------- FloodingSimulator ----------

#[test]
fn flooding_simulator_water_source() {
    let (graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    let mut sim = FloodingSimulator::new(graph, PropagationConfig::default());
    sim.add_water_source(id_a, 50.0);

    sim.step(1.0);
    let level = sim.graph().get_compartment(id_a).unwrap().current_level;
    assert!(level > 0.0);
}

#[test]
fn flooding_simulator_water_source_removed() {
    let (graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    let mut sim = FloodingSimulator::new(graph, PropagationConfig::default());
    sim.add_water_source(id_a, 50.0);

    sim.step(1.0);
    let level_after = sim.graph().get_compartment(id_a).unwrap().current_level;
    assert!(level_after > 0.0);

    sim.remove_water_source(id_a);
    sim.step(1.0);
    let level_later = sim.graph().get_compartment(id_a).unwrap().current_level;
    assert!((level_later - level_after).abs() < 0.001);
}

// ---------- pump_processing ----------

#[test]
fn pump_reduces_water() {
    let (mut graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    graph.add_water(id_a, 50.0);

    let mut sys = PropagationSystem::new(PropagationConfig::default());
    if let Some(node) = graph.get_compartment_mut(id_a) {
        node.pump_active = true;
    }

    let level_before = graph.get_compartment(id_a).unwrap().current_level;
    let events = sys.update(&mut graph, 1.0);
    let pumped: Vec<_> = events.iter()
        .filter(|e| e.event_type == PropagationEventType::WaterPumped)
        .collect();
    assert!(!pumped.is_empty());

    let level_after = graph.get_compartment(id_a).unwrap().current_level;
    assert!(level_after < level_before);
}

// ---------- water flow between compartments ----------

#[test]
fn water_flows_through_open_bulkhead() {
    let (mut graph, id_a, _id_b, _bh_id) = build_graph_two_compartments();
    graph.add_water(id_a, 80.0);

    {
        let bh = graph.get_bulkhead_mut(comp_id(100)).unwrap();
        bh.is_sealed = false;
    }

    let mut sys = PropagationSystem::new(PropagationConfig::default());
    let events = sys.update(&mut graph, 1.0);

    let flow_events: Vec<_> = events.iter()
        .filter(|e| e.event_type == PropagationEventType::WaterFlow)
        .collect();
    assert!(!flow_events.is_empty());
}

#[test]
fn water_blocked_by_sealed_bulkhead() {
    let (mut graph, id_a, id_b, _bh_id) = build_graph_two_compartments();
    graph.add_water(id_a, 80.0);

    let mut sys = PropagationSystem::new(PropagationConfig::default());
    let events = sys.update(&mut graph, 1.0);

    let flow_events: Vec<_> = events.iter()
        .filter(|e| e.event_type == PropagationEventType::WaterFlow)
        .collect();
    assert!(flow_events.is_empty());

    let level_b = graph.get_compartment(id_b).unwrap().current_level;
    assert!((level_b).abs() < 0.001);
    let level_a = graph.get_compartment(id_a).unwrap().current_level;
    assert!((level_a - 80.0).abs() < 0.001);
}

// ---------- fire spread through open bulkhead ----------

#[test]
fn fire_spreads_through_open_bulkhead() {
    let (mut graph, id_a, id_b, _bh_id) = build_graph_two_compartments();
    let mut sys = PropagationSystem::new(PropagationConfig::default());
    sys.ignite_fire(&mut graph, id_a, 0.8);

    {
        let bh = graph.get_bulkhead_mut(comp_id(100)).unwrap();
        bh.is_sealed = false;
    }

    sys.update(&mut graph, 1.0);
    let fire_b = graph.get_compartment(id_b).unwrap().fire_intensity;
    assert!(fire_b > 0.0);
}

#[test]
fn fire_blocked_by_sealed_bulkhead() {
    let (mut graph, id_a, id_b, _bh_id) = build_graph_two_compartments();
    let mut sys = PropagationSystem::new(PropagationConfig::default());
    sys.ignite_fire(&mut graph, id_a, 0.8);

    sys.update(&mut graph, 1.0);
    let fire_b = graph.get_compartment(id_b).unwrap().fire_intensity;
    assert!((fire_b).abs() < 0.001);
}
