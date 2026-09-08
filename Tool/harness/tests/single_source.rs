// Side-only tests for single-source station/compartment state (bug №53),
// populated bulkhead links (bug №54) and the hull-scaled id scheme (bug №52).
use rfs_core::entity::{EntityId, StationType};
use rfs_core::time::FIXED_DT;
use rfs_damage::damage::DamageSystem;
use rfs_ship::loader::create_default_frigate;
use rfs_ship::ship::{Ship, ShipConfig};
use std::collections::HashSet;
use std::sync::Arc;

const SHIP_ID_STRIDE: u64 = 10_000;

fn make_ship(id: u64) -> Ship {
    let class = create_default_frigate();
    let config: ShipConfig = class.into();
    Ship::new(config, EntityId::new(id), Arc::new(DamageSystem::new()))
}

fn ship_station_ids(ship: &Ship, station_type: StationType) -> Vec<EntityId> {
    ship.get_state()
        .station_states
        .iter()
        .filter(|(_, s)| s.station_type == station_type)
        .map(|(id, _)| *id)
        .collect()
}

#[test]
fn occupancy_is_single_sourced_no_double_occupation() {
    let ship = make_ship(10);
    let gun = ship_station_ids(&ship, StationType::Gun)[0];

    assert!(ship.occupy_station(gun, EntityId::new(777)), "first occupy");
    assert!(
        !ship.occupy_station(gun, EntityId::new(778)),
        "second player must be rejected"
    );
    assert!(ship.vacate_station(gun).is_some(), "vacate returns occupant");
    assert!(
        ship.occupy_station(gun, EntityId::new(779)),
        "re-occupy after vacate"
    );
}

#[test]
fn gun_fires_then_waits_out_cooldown_and_fires_again() {
    let ship = make_ship(10);
    let gun = ship_station_ids(&ship, StationType::Gun)[0];

    assert!(
        ship.try_fire_station(gun, None).is_some(),
        "first shot leaves the barrel"
    );
    assert!(
        ship.try_fire_station(gun, None).is_none(),
        "cooldown blocks instant re-fire (no permanent clamp)"
    );

    // 600 ticks @ 30 Hz = 20 s > gun cooldown (10 s).
    for _ in 0..600 {
        let mut state = ship.get_state();
        ship.update(FIXED_DT, &mut state);
        ship.apply_state(state);
    }
    assert!(
        ship.try_fire_station(gun, None).is_some(),
        "ready again after the cooldown window"
    );
}

#[test]
fn bulkheads_and_connections_resolve_to_real_ship_compartments() {
    let ship = make_ship(10);
    let state = ship.get_state();

    let comp_ids: HashSet<EntityId> = state.compartment_states.keys().copied().collect();
    assert!(!comp_ids.is_empty());

    for (_, cs) in &state.compartment_states {
        assert!(
            !cs.connected_compartments.is_empty(),
            "compartment must be wired to neighbors"
        );
        for connected in &cs.connected_compartments {
            assert!(
                comp_ids.contains(connected),
                "connected_compartments must point at this ship's compartments"
            );
        }
        for bh in &cs.bulkhead_states {
            assert_ne!(bh.entity_id, EntityId::nil(), "bulkhead id must be assigned");
            assert_ne!(bh.connects_to, EntityId::nil(), "bulkhead must be linked");
            assert!(comp_ids.contains(&bh.connects_to));
        }
    }

    for (sid, _) in &state.station_states {
        let compartment_id = ship.get_station(*sid).map(|s| s.compartment_id());
        assert!(
            compartment_id.is_some_and(|cid| comp_ids.contains(&cid) || cid == EntityId::nil()),
            "station must be linked to one of the ship's compartments"
        );
    }
}

#[test]
fn entity_ids_do_not_collide_across_ships() {
    let a = make_ship(10);
    let b = make_ship(10 + SHIP_ID_STRIDE);

    let mut ids: HashSet<u64> = HashSet::new();
    for ship in [&a, &b] {
        let state = ship.get_state();
        assert!(ids.insert(ship.entity_id().0), "duplicate ship id");
        for cid in state.compartment_states.keys() {
            assert!(ids.insert(cid.0), "duplicate compartment id {}", cid.0);
        }
        for sid in state.station_states.keys() {
            assert!(ids.insert(sid.0), "duplicate station id {}", sid.0);
        }
        for cs in state.compartment_states.values() {
            for bh in &cs.bulkhead_states {
                assert!(
                    ids.insert(bh.entity_id.0),
                    "duplicate bulkhead id {}",
                    bh.entity_id.0
                );
            }
        }
    }
}