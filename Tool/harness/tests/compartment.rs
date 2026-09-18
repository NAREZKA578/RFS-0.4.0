use rfs_core::entity::{EntityId, CompartmentTemplate, Bounds, Vec3f};
use rfs_ship::compartment::*;
use std::collections::HashMap;

fn comp_id(n: u64) -> EntityId { EntityId::new(n) }

fn make_template(max_water: f32) -> CompartmentTemplate {
    CompartmentTemplate {
        name: "Test".into(),
        local_bounds: Bounds::new(Vec3f::ZERO, Vec3f::new(10.0, 5.0, 20.0)),
        max_water_level: max_water,
        pump_capacity: 10.0,
        connected_compartments: vec![],
        bulkheads: vec![],
        stations: vec![],
    }
}

fn make_compartment(id: u64, max_water: f32) -> Compartment {
    Compartment::new(
        comp_id(id),
        comp_id(0),
        make_template(max_water),
        vec![],
        vec![],
    )
}

fn make_state(max_water: f32) -> CompartmentState {
    CompartmentState {
        water_level: 0.0,
        max_water_level: max_water,
        is_sealed: true,
        is_breached: false,
        fire_intensity: 0.0,
        pump_active: false,
        pump_capacity: 10.0,
        bulkhead_states: vec![],
        connected_compartments: vec![],
    }
}

fn state_with_water(water: f32, max_water: f32) -> CompartmentState {
    let mut s = make_state(max_water);
    s.water_level = water;
    s
}

// ---------- CompartmentState::is_flooded ----------

#[test]
fn not_flooded_at_0_9() {
    let s = state_with_water(90.0, 100.0);
    assert!(!s.is_flooded());
}

#[test]
fn flooded_at_0_91() {
    let s = state_with_water(91.0, 100.0);
    assert!(s.is_flooded());
}

// ---------- CompartmentState::is_critical ----------

#[test]
fn not_critical_at_0_5() {
    let s = state_with_water(50.0, 100.0);
    assert!(!s.is_critical());
}

#[test]
fn critical_at_0_51() {
    let s = state_with_water(51.0, 100.0);
    assert!(s.is_critical());
}

// ---------- CompartmentState::fill_ratio ----------

#[test]
fn fill_ratio_normal() {
    let s = state_with_water(75.0, 100.0);
    assert!((s.fill_ratio() - 0.75).abs() < 0.001);
}

#[test]
fn fill_ratio_zero_max() {
    let mut s = make_state(0.0);
    s.water_level = 10.0;
    assert!((s.fill_ratio()).abs() < 0.001);
}

// ---------- Compartment::update ----------

#[test]
fn update_breach_intake() {
    let c = make_compartment(1, 100.0);
    let mut s = make_state(100.0);
    s.is_breached = true;
    s.is_sealed = false;
    s.water_level = 0.0;

    c.update(1.0, &mut s);
    assert!(s.water_level > 0.0);
    assert!(s.water_level <= 100.0);
}

#[test]
fn update_breach_capped_at_max() {
    let c = make_compartment(1, 10.0);
    let mut s = make_state(10.0);
    s.is_breached = true;
    s.is_sealed = false;
    s.water_level = 9.0;

    c.update(1.0, &mut s);
    assert!(s.water_level <= 10.0);
}

#[test]
fn update_pump_reduces_water() {
    let c = make_compartment(1, 100.0);
    let mut s = state_with_water(50.0, 100.0);
    s.pump_active = true;

    c.update(1.0, &mut s);
    assert!(s.water_level < 50.0);
}

#[test]
fn update_breach_plus_pump_net() {
    let c = make_compartment(1, 100.0);
    let mut s = state_with_water(50.0, 100.0);
    s.is_breached = true;
    s.is_sealed = false;
    s.pump_active = true;
    s.pump_capacity = 20.0;

    c.update(1.0, &mut s);
    let intake_rate: f32 = 10.0;
    let pump_out: f32 = 20.0;
    let expected = (50.0 + intake_rate - pump_out).max(0.0);
    assert!((s.water_level - expected).abs() < 0.001);
}

#[test]
fn update_fire_decreases() {
    let c = make_compartment(1, 100.0);
    let mut s = make_state(100.0);
    s.fire_intensity = 0.5;

    c.update(1.0, &mut s);
    assert!(s.fire_intensity < 0.5);
}

// ---------- compute_water_spread ----------

#[test]
fn compute_water_spread_sealed_blocks() {
    let c = make_compartment(1, 100.0);
    let mut state = state_with_water(80.0, 100.0);
    state.connected_compartments = vec![comp_id(2)];
    state.bulkhead_states = vec![BulkheadState {
        entity_id: comp_id(10),
        connects_to: comp_id(2),
        is_sealed: true,
        is_destroyed: false,
        seal_strength: 1000.0,
        ..Default::default()
    }];

    let mut all = HashMap::new();
    all.insert(comp_id(1), state.clone());
    all.insert(comp_id(2), state_with_water(10.0, 100.0));

    let transfers = c.compute_water_spread(&state, &all, 1.0);
    assert!(transfers.is_empty());
}

#[test]
fn compute_water_spread_open_allows() {
    let c = make_compartment(1, 100.0);
    let mut state = state_with_water(80.0, 100.0);
    state.connected_compartments = vec![comp_id(2)];
    state.bulkhead_states = vec![BulkheadState {
        entity_id: comp_id(10),
        connects_to: comp_id(2),
        is_sealed: false,
        is_destroyed: false,
        seal_strength: 1000.0,
        ..Default::default()
    }];

    let mut all = HashMap::new();
    all.insert(comp_id(1), state.clone());
    all.insert(comp_id(2), state_with_water(10.0, 100.0));

    let transfers = c.compute_water_spread(&state, &all, 1.0);
    assert!(!transfers.is_empty());
    assert!(transfers[0].amount > 0.0);
}

#[test]
fn compute_water_spread_room_capped() {
    let c = make_compartment(1, 100.0);
    let mut state = state_with_water(100.0, 100.0);
    state.connected_compartments = vec![comp_id(2)];
    state.bulkhead_states = vec![BulkheadState {
        entity_id: comp_id(10),
        connects_to: comp_id(2),
        is_sealed: false,
        is_destroyed: false,
        seal_strength: 1000.0,
        ..Default::default()
    }];

    let mut all = HashMap::new();
    all.insert(comp_id(1), state.clone());
    all.insert(comp_id(2), state_with_water(99.9, 100.0));

    let transfers = c.compute_water_spread(&state, &all, 1.0);
    assert!(!transfers.is_empty());
    for t in &transfers {
        assert!(t.amount <= 0.1 + 0.001);
    }
}

// ---------- fire_spread ----------

#[test]
fn fire_spread_through_open_bulkhead() {
    let c = make_compartment(1, 100.0);
    let mut state = make_state(100.0);
    state.fire_intensity = 0.8;
    state.connected_compartments = vec![comp_id(2)];
    state.bulkhead_states = vec![BulkheadState {
        entity_id: comp_id(10),
        connects_to: comp_id(2),
        is_sealed: false,
        is_destroyed: false,
        seal_strength: 1000.0,
        ..Default::default()
    }];

    let mut all = HashMap::new();
    all.insert(comp_id(1), state.clone());
    all.insert(comp_id(2), make_state(100.0));

    let spreads = c.fire_spread(&state, &all, 1.0);
    assert!(!spreads.is_empty());
    assert!(spreads[0].1 > 0.0);
}

#[test]
fn fire_spread_blocked_by_sealed_bulkhead() {
    let c = make_compartment(1, 100.0);
    let mut state = make_state(100.0);
    state.fire_intensity = 0.8;
    state.connected_compartments = vec![comp_id(2)];
    state.bulkhead_states = vec![BulkheadState {
        entity_id: comp_id(10),
        connects_to: comp_id(2),
        is_sealed: true,
        is_destroyed: false,
        seal_strength: 1000.0,
        ..Default::default()
    }];

    let mut all = HashMap::new();
    all.insert(comp_id(1), state.clone());
    all.insert(comp_id(2), make_state(100.0));

    let spreads = c.fire_spread(&state, &all, 1.0);
    assert!(spreads.is_empty());
}

// ---------- apply_damage ----------

#[test]
fn apply_damage_within_range() {
    let c = make_compartment(1, 100.0);
    let mut s = make_state(100.0);
    s.bulkhead_states = vec![BulkheadState {
        entity_id: comp_id(10),
        connects_to: comp_id(2),
        local_position: Vec3f::ZERO,
        health: 1000.0,
        max_health: 1000.0,
        seal_strength: 1000.0,
        is_sealed: true,
        is_destroyed: false,
    }];

    c.apply_damage(&mut s, 50.0, Vec3f::new(1.0, 0.0, 0.0));
    assert!(s.bulkhead_states[0].health < 1000.0);
}

#[test]
fn apply_damage_zero_returns_early() {
    let c = make_compartment(1, 100.0);
    let mut s = make_state(100.0);
    s.bulkhead_states = vec![BulkheadState {
        entity_id: comp_id(10),
        connects_to: comp_id(2),
        local_position: Vec3f::ZERO,
        health: 1000.0,
        max_health: 1000.0,
        seal_strength: 1000.0,
        is_sealed: true,
        is_destroyed: false,
    }];

    c.apply_damage(&mut s, 0.0, Vec3f::ZERO);
    assert!((s.bulkhead_states[0].health - 1000.0).abs() < 0.001);
}

#[test]
fn apply_damage_destroys_bulkhead() {
    let c = make_compartment(1, 100.0);
    let mut s = make_state(100.0);
    s.bulkhead_states = vec![BulkheadState {
        entity_id: comp_id(10),
        connects_to: comp_id(2),
        local_position: Vec3f::ZERO,
        health: 10.0,
        max_health: 10.0,
        seal_strength: 1000.0,
        is_sealed: true,
        is_destroyed: false,
    }];

    c.apply_damage(&mut s, 50.0, Vec3f::ZERO);
    assert!(s.bulkhead_states[0].is_destroyed);
    assert!(!s.bulkhead_states[0].is_sealed);
}

// ---------- toggle_seal ----------

#[test]
fn toggle_seal() {
    let c = make_compartment(1, 100.0);
    let mut s = make_state(100.0);
    s.is_sealed = true;

    let result = c.toggle_seal(&mut s);
    assert!(!result);
    assert!(!s.is_sealed);

    let result2 = c.toggle_seal(&mut s);
    assert!(result2);
    assert!(s.is_sealed);
}

// ---------- set_fire ----------

#[test]
fn set_fire_clamping() {
    let c = make_compartment(1, 100.0);
    let mut s = make_state(100.0);

    c.set_fire(&mut s, 2.0);
    assert!((s.fire_intensity - 1.0).abs() < 0.001);

    c.set_fire(&mut s, -1.0);
    assert!((s.fire_intensity).abs() < 0.001);
}

// ---------- activate/deactivate pump ----------

#[test]
fn activate_deactivate_pump() {
    let c = make_compartment(1, 100.0);
    let mut s = make_state(100.0);

    c.activate_pump(&mut s);
    assert!(s.pump_active);

    c.deactivate_pump(&mut s);
    assert!(!s.pump_active);
}

// ---------- Compartment accessors ----------

#[test]
fn compartment_entity_id() {
    let c = make_compartment(42, 100.0);
    assert_eq!(c.entity_id(), comp_id(42));
}

#[test]
fn compartment_name() {
    let c = make_compartment(1, 100.0);
    assert_eq!(c.name(), "Test");
}

#[test]
fn compartment_bounds() {
    let c = make_compartment(1, 100.0);
    let b = c.bounds();
    assert!((b.max.x - 10.0).abs() < 0.001);
}

#[test]
fn compartment_pump_capacity() {
    let c = make_compartment(1, 100.0);
    assert!((c.pump_capacity() - 10.0).abs() < 0.001);
}

#[test]
fn extinguish_fire() {
    let c = make_compartment(1, 100.0);
    let mut s = make_state(100.0);
    s.fire_intensity = 0.9;
    c.extinguish_fire(&mut s);
    assert!((s.fire_intensity).abs() < 0.001);
}
