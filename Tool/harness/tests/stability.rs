// Side-only tests for the stability batch (bugs №44, №49, №50, №51, №55).
use rfs_core::entity::{EntityId, StationType};
use rfs_core::math::{Quatf, Vec3f};
use rfs_core::packet::ProjectileType;
use rfs_core::time::FIXED_DT;
use rfs_damage::damage::{DamageSystem, DamageTarget};
use rfs_ship::loader::create_default_frigate;
use rfs_ship::ship::{Ship, ShipConfig};
use rfs_sim::ballistics::{BallisticsCalculator, BallisticsConfig};
use std::sync::Arc;

fn make_ship(id: u64) -> Ship {
    let class = create_default_frigate();
    let config: ShipConfig = class.into();
    Ship::new(config, EntityId::new(id), Arc::new(DamageSystem::new()))
}

#[test]
fn quat_default_is_identity_and_zero_normalizes_cleanly() {
    // №50: a zero quaternion normalizes to NaN and collapses transforms.
    assert_eq!(Quatf::default(), Quatf::IDENTITY);
    let n = Quatf::new(0.0, 0.0, 0.0, 0.0).normalize();
    assert_eq!(n, Quatf::IDENTITY);
    assert!(n.x.is_finite() && n.y.is_finite() && n.z.is_finite() && n.w.is_finite());
    let v = Quatf::default().mul_vec3(Vec3f::FORWARD);
    assert!((v - Vec3f::FORWARD).length() < 1e-6);
}

#[test]
fn negative_turn_rate_does_not_panic() {
    // №49: turn_rate comes from JSON unvalidated; clamp(min>max) panics.
    let class = create_default_frigate();
    let mut config: ShipConfig = class.into();
    config.turn_rate = -1.5;
    let ship = Ship::new(config, EntityId::new(10), Arc::new(DamageSystem::new()));

    ship.set_rudder(5.0);
    let mut state = ship.get_state();
    state.rudder_angle = 5.0;
    ship.update(FIXED_DT, &mut state);
    ship.apply_state(state);
}

#[test]
fn heading_rotates_around_y_not_pitch() {
    // №51: heading spun angular_velocity.y but was fed as pitch.
    let ship = make_ship(10);
    let mut state = ship.get_state();
    state.angular_velocity.y = 1.0;
    ship.update(FIXED_DT, &mut state);
    ship.apply_state(state);

    let state = ship.get_state();
    let forward = state.transform.rotation.mul_vec3(Vec3f::FORWARD);
    assert!(
        forward.y.abs() < 1e-5,
        "turning must not pitch the bow, forward={forward:?}"
    );
    assert!((forward.length() - 1.0).abs() < 1e-5);
}

#[test]
fn unknown_projectile_type_does_not_panic() {
    // №44: GrapeShot has no config entry; the old unwrap blew up.
    let calc = BallisticsCalculator::new(BallisticsConfig::default());
    let traj = calc.calculate_trajectory(
        Vec3f::ZERO,
        Vec3f::FORWARD,
        ProjectileType::GrapeShot,
        5.0,
        0.1,
    );
    assert!(traj.is_empty());
    assert!(calc
        .solve_ballistic_arc(Vec3f::ZERO, Vec3f::new(100.0, 0.0, 0.0), ProjectileType::GrapeShot)
        .is_none());
}

#[test]
fn negative_damage_does_not_heal() {
    // №55: negative damage overhealed above max.
    let ship = make_ship(10);

    let full = ship.health();
    ship.apply_health_damage(-500.0);
    assert!(
        (ship.health() - full).abs() < 1e-4,
        "hull must not overheal, health={}",
        ship.health()
    );

    let gun = ship
        .get_state()
        .station_states
        .iter()
        .find(|(_, s)| s.station_type == StationType::Gun)
        .map(|(id, _)| *id)
        .expect("a gun station");
    let station = ship.get_station(gun).expect("gun holder");
    let mut s = ship.get_state().station_states[&gun].clone();
    s.health = 100.0;
    station.apply_damage(&mut s, -50.0);
    assert!(s.health <= s.max_health, "station must not overheal");
    station.repair(&mut s, -1000.0);
    assert!(s.health >= 0.0, "negative repair must not damage");

    let cid = *ship.get_state().compartment_states.keys().next().expect("compartment");
    let compartment = ship.get_compartment(cid).expect("holder");
    let mut c = ship.get_state().compartment_states[&cid].clone();
    let before: Vec<f32> = c.bulkhead_states.iter().map(|b| b.health).collect();
    compartment.apply_damage(&mut c, -100.0, Vec3f::ZERO);
    let after: Vec<f32> = c.bulkhead_states.iter().map(|b| b.health).collect();
    assert_eq!(before, after, "negative damage must not inflate bulkheads");
}
