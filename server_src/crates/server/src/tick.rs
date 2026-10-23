//! The per-input and per-event glue between the network and the simulation:
//! applying an input packet to a ship, turning a station shot into a live
//! projectile, and translating simulation events into wire events.
//!
//! Split out of the binary (plan H5) so tools can replay inputs the same way
//! the loop does.

use rfs_core::entity::EntityId;
use rfs_core::math::Vec3f;
use rfs_core::packet::{GameEvent as CoreGameEvent, InputPacket, ProjectileType};
use rfs_ship::ship::Ship;
use rfs_ship::station::FireResult;
use rfs_sim::ballistics::{BallisticsCalculator, ProjectileConfig, TrajectoryPoint};
use rfs_sim::tick::{GameEvent as SimGameEvent, Projectile, TickSystem};
use tracing::warn;

use crate::match_::{current_helm_occupant, new_projectile_id};
use crate::net::handle_station_interaction;

/// Bug №150: NaN-safe clamp for axis inputs — f32::clamp passes NaN through,
/// which would poison physics. Non-finite inputs are treated as 0.
pub fn clamp_axis(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

pub fn apply_input(sim: &TickSystem, ship: &Ship, player_id: EntityId, input: &InputPacket) {
    // Bug №59: steering is arbitrated — whoever ended up at a Helm seat, and
    // only that player, may drive. With nobody at the helm, anyone may (the
    // ship is un-manned). This replaces "last packet of the tick wins".
    let helm = current_helm_occupant(ship);
    if helm.is_none() || helm == Some(player_id) {
        ship.set_throttle(clamp_axis(input.move_forward));
        let rudder = (clamp_axis(input.move_right) + clamp_axis(input.yaw) * 0.5).clamp(-1.0, 1.0);
        ship.set_rudder(rudder);
    }

    if let Some(interaction) = &input.station_interaction {
        // Bug №196: a refused shot returns a reason instead of vanishing. The
        // input path has no command ack to carry it, so it is logged here — the
        // point is that the drop is observable at all.
        if let Some(reason) = handle_station_interaction(sim, ship, player_id, interaction) {
            warn!("player {:?} station action refused: {reason}", player_id.0);
        }
    }
}

pub fn spawn_projectile(
    sim: &TickSystem,
    ship: &Ship,
    weapon: EntityId,
    result: FireResult,
) -> Result<(), rfs_sim::tick::ProjectileRejected> {
    let world_pos = ship.local_to_world(result.position);
    let world_vel = ship.transform().rotation.mul_vec3(result.velocity);
    // Bug №193: damage, penetration and blast radius now come from the
    // projectile type's own config instead of a hardcoded 100/50 in the station
    // and a hardcoded 5.0 here. Those two answers disagreed with the config,
    // and with each other, and the config was the one nobody read.
    let config = ProjectileConfig::for_type(result.projectile_type);
    sim.fire_projectile(Projectile {
        entity_id: new_projectile_id(),
        projectile_type: result.projectile_type,
        position: world_pos,
        prev_position: world_pos,
        velocity: world_vel,
        spawn_tick: sim.current_tick(),
        lifetime: 0.0,
        max_lifetime: 12.0,
        distance_traveled: 0.0,
        owner: ship.entity_id(),
        weapon,
        damage: config.damage,
        penetration: config.penetration,
        explosion_radius: config.explosion_radius,
        ricochet_count: 0,
    })
}

/// Bug №664 (remainder): how much predicted parabolic drop a target must
/// promise before the fire-control solution pitches off the straight line.
/// 2 m: a 400 m/s cannonball promises 0.31 m at 100 m and 191 m at 2500 m, so
/// close engagements (and the FireWeapon tests' 100 m target) keep №154's
/// straight aim untouched, and the arc owns only the ranges where "aims at"
/// and "lands on" visibly part company.
pub const ICS_DROP_TOLERANCE: f32 = 2.0;

/// Bug №664 (remainder): ICS ballistic elevation for the FireWeapon aim.
///
/// The straight-line aim (№154) points the barrel at the target; on ranges
/// where gravity has time to matter that lands short. The ideal parabola is
/// priced by `solve_ballistic_arc` for THIS muzzle and THIS target; because
/// the paper solution ignores drag, a small deterministic ladder of higher
/// pitches searches for the pitch whose REAL integration
/// (`calculate_trajectory` — the same integrator the shell later flies) lands
/// closest on the target line, and a step is taken only while it measurably
/// improves on the previous one. The first non-improving step ends the search.
///
/// Gates:
/// - underwater ammunition (torpedoes, depth charges) keeps the straight aim;
/// - a target whose predicted parabolic drop is under [`ICS_DROP_TOLERANCE`]
///   keeps №154's straight aim entirely;
/// - `solve_ballistic_arc` refusing (unreachable range, degenerate muzzle)
///   falls back to the straight aim.
///
/// Deterministic by construction: no RNG, no wall clock. The same muzzle, the
/// same target, and the same round type give the same pitch every call, which
/// is what makes the deferral tests (`defer_shot`) and the logs comparable
/// across ticks.
///
/// Returns `(yaw, pitch)` in ship-local radians for `set_station_target_angles`.
pub fn ballistic_aim(
    ship: &Ship,
    muzzle_local: Vec3f,
    target: Vec3f,
    projectile_type: ProjectileType,
    ballistics: &BallisticsCalculator,
) -> (f32, f32) {
    let muzzle_world = ship.local_to_world(muzzle_local);
    let dir_local = ship.world_to_local(target) - muzzle_local;
    let yaw_local = dir_local.x.atan2(dir_local.z);
    let straight_pitch = dir_local.y.clamp(-1.0, 1.0).asin();
    let fallback = (yaw_local, straight_pitch);

    let config = ProjectileConfig::for_type(projectile_type);

    // Underwater weapons are not barrel arcs; the straight aim is theirs.
    if matches!(
        projectile_type,
        ProjectileType::Torpedo | ProjectileType::DepthCharge
    ) || !config.initial_velocity.is_finite()
        || config.initial_velocity <= 0.0
    {
        return fallback;
    }

    // Gate: a near target keeps the straight aim. The drop below is the ideal
    // formula's number — drag can only steepen it — so the gate is
    // conservative: it may keep a target on the straight aim one step longer
    // than strictly needed, never switch it to the arc too early.
    let to_target = target - muzzle_world;
    let distance = to_target.length();
    let flight = distance / config.initial_velocity;
    if !flight.is_finite() || 0.5 * 9.81 * flight * flight <= ICS_DROP_TOLERANCE {
        return fallback;
    }

    let Some(solution) = ballistics.solve_ballistic_arc(muzzle_world, target, projectile_type)
    else {
        return fallback;
    };

    // The paper solution tunes itself to the ideal formula, so it is already
    // aimed at the target on paper; the question is only how far short the
    // real drag lands it. Ladder up while the measured answer improves.
    let yaw_world = solution.yaw;
    let target_horizontal = (to_target.x * to_target.x + to_target.z * to_target.z).sqrt();

    let landing_at = |pitch: f32| -> f32 {
        let dir = Vec3f::new(
            pitch.cos() * yaw_world.sin(),
            pitch.sin(),
            pitch.cos() * yaw_world.cos(),
        );
        let points =
            ballistics.calculate_trajectory(muzzle_world, dir, projectile_type, 30.0, 0.05);
        let horizontal = |p: &TrajectoryPoint| {
            let dx = p.position.x - muzzle_world.x;
            let dz = p.position.z - muzzle_world.z;
            (dx * dx + dz * dz).sqrt()
        };
        for pair in points.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if a.position.y > 0.0 && b.position.y <= 0.0 {
                let span = a.position.y - b.position.y;
                let t = if span.abs() < 1e-6 {
                    0.5
                } else {
                    a.position.y / span
                };
                let (ha, hb) = (horizontal(a), horizontal(b));
                return ha + (hb - ha) * t;
            }
        }
        points.last().map(horizontal).unwrap_or(target_horizontal)
    };

    let mut best_pitch = solution.pitch;
    let mut best_error = (landing_at(solution.pitch) - target_horizontal).abs();
    // Drag steals range, so the search direction is strictly up; 1.55 rad
    // (~89°) is where a barrel stops being a barrel.
    for scale in [1.08_f32, 1.16, 1.24] {
        let candidate = solution.pitch * scale;
        if candidate >= 1.55 {
            break;
        }
        let error = (landing_at(candidate) - target_horizontal).abs();
        if error < best_error {
            best_pitch = candidate;
            best_error = error;
        } else {
            break;
        }
    }

    // Back to ship-local angles for `set_station_target_angles`. The formal
    // conversion — not the assumption that pitch is frame-invariant — keeps
    // it honest for heeled decks: the world-space launch direction is rotated
    // back through the ship's inverse orientation, and the resulting local
    // elevation is what the turret is told to reach.
    let dir_world = Vec3f::new(
        best_pitch.cos() * yaw_world.sin(),
        best_pitch.sin(),
        best_pitch.cos() * yaw_world.cos(),
    );
    let dir_body = ship.transform().rotation.inverse().mul_vec3(dir_world);
    let pitch_local = dir_body.y.clamp(-1.0, 1.0).asin();

    (yaw_local, pitch_local)
}

// Bug №630: a shot assigned for deferral while the turret is still slewing to
// the angles the fire command installed. The command path used to aim and
// fire in the same tick, so the shell always left along the orientation the
// turret had BEFORE slewing — the first shot of every engagement went where
// the gun was pointing, not where the gunner asked. The owner picked deferral
// with a deadline: the shot waits until the turret is where it was told to
// be, and is refused loudly when that never happens.

/// Simulation ticks a deferred shot may wait for its turret before the shot
/// is explicitly refused. Counted in simulation ticks on purpose: the
/// countdown runs only in ticks where the turret actually moves, so a skipped
/// cadence deadline under load (#614) cannot shorten the window — the answer
/// to "how long has the turret had to aim" is measured in the same currency
/// as the slew itself. The slowest station (1.0 rad/s) crosses the worst
/// in-range slew of π rad in ~3.2 s, so 120 ticks (4 s at `TICK_RATE = 30`)
/// leaves a ~25% margin above the slowest legitimate arrival and still kills
/// a turret that will never get there (broken/off-line station, zero turn
/// speed).
pub const AIM_TIMEOUT_TICKS: u32 = 120;

/// A shot waiting for its turret. One per station at most: a newer aim for
/// the same station replaces the older entry (see `defer_shot`), so what
/// fires is the shooter's freshest intent, not a stale one.
pub struct DeferredShot {
    pub ship: EntityId,
    pub station: EntityId,
    /// Simulation ticks remaining before the shot is refused.
    pub ttl: u32,
}

/// Books, or re-books, a deferred shot for one station of one ship. An
/// existing entry for the same station is dropped first so a superseded aim
/// cannot fire after the shooter corrected the aim.
pub fn defer_shot(pending: &mut Vec<DeferredShot>, ship: EntityId, station: EntityId) {
    pending.retain(|shot| !(shot.ship == ship && shot.station == station));
    pending.push(DeferredShot {
        ship,
        station,
        ttl: AIM_TIMEOUT_TICKS,
    });
}

/// Resolves pending shots after a completed simulation tick, i.e. after
/// `Station::update` has done this tick's capped slew step. Three endings:
/// the turret arrived → the shot fires (and is consumed, whatever verdict
/// `spawn_projectile` yields — the deferral has no ack channel, so a refused
/// spawn is reported in the log, the shape the №196 input path already uses);
/// the turret will not make it in time → the shot is refused in the log;
/// the ship is gone → the deferral dies with it.
pub fn resolve_deferred_shots(sim: &TickSystem, pending: &mut Vec<DeferredShot>) {
    pending.retain_mut(|shot| {
        let Some(ship) = sim.get_ship(shot.ship) else {
            return false;
        };
        if !ship.station_aim_reached(shot.station) {
            let left = shot.ttl.saturating_sub(1);
            if left == 0 {
                warn!(
                    "deferred shot on station {:?}: turret did not reach the aim within {} ticks; shot refused",
                    shot.station.0, AIM_TIMEOUT_TICKS
                );
                return false;
            }
            shot.ttl = left;
            return true;
        }
        match ship.try_fire_station(shot.station, None) {
            Some(result) => {
                if let Err(rejected) = spawn_projectile(sim, &ship, shot.station, result) {
                    warn!(
                        "deferred shot on station {:?}: shot refused: {}",
                        shot.station.0,
                        rejected.as_str()
                    );
                }
                false
            }
            None => {
                warn!(
                    "deferred shot on station {:?}: weapon not ready when the aim completed; shot refused",
                    shot.station.0
                );
                false
            }
        }
    });
}

pub fn core_game_event(event: &SimGameEvent) -> CoreGameEvent {
    match event {
        SimGameEvent::ShipHit {
            target,
            projectile,
            position,
            normal,
            damage,
            penetration,
            hit_compartment,
        } => CoreGameEvent::ShipHit {
            target: *target,
            projectile: *projectile,
            position: *position,
            normal: *normal,
            damage: *damage,
            penetration: *penetration,
            hit_compartment: *hit_compartment,
        },
        SimGameEvent::ShipSunk { ship, position } => CoreGameEvent::ShipSunk {
            ship: *ship,
            position: *position,
        },
        SimGameEvent::CompartmentFlooded {
            compartment,
            water_level,
        } => CoreGameEvent::CompartmentFlooded {
            compartment: *compartment,
            water_level: *water_level,
        },
        SimGameEvent::StationOccupied { station, player } => CoreGameEvent::StationOccupied {
            station: *station,
            player: *player,
        },
        SimGameEvent::StationVacated { station, player } => CoreGameEvent::StationVacated {
            station: *station,
            player: *player,
        },
        SimGameEvent::FireStarted {
            compartment,
            intensity,
        } => CoreGameEvent::FireStarted {
            compartment: *compartment,
            intensity: *intensity,
        },
        SimGameEvent::FireExtinguished { compartment } => CoreGameEvent::FireExtinguished {
            compartment: *compartment,
        },
        SimGameEvent::ShipRam {
            rammer,
            target,
            position,
            normal,
            damage,
        } => CoreGameEvent::ShipRam {
            rammer: *rammer,
            target: *target,
            position: *position,
            normal: *normal,
            damage: *damage,
        },
        // Bug №307: the client has to see that a shot ended on a structure —
        // otherwise the shell vanishes at the rock.
        SimGameEvent::ProjectileImpact {
            projectile,
            position,
            normal,
        } => CoreGameEvent::ProjectileImpact {
            projectile: *projectile,
            position: *position,
            normal: *normal,
        },
        // Bug №701: the crossing of the point of no return is world-visible —
        // every viewer of the hull learns it is going down. The crew's own
        // deaths do NOT ride this arm: they are fanned out per player in
        // `main.rs`, which reads the players map that the pure translator
        // never sees.
        SimGameEvent::ShipSinkingStarted { ship } => {
            CoreGameEvent::ShipSinkingStarted { ship: *ship }
        }
    }
}

#[cfg(test)]
mod deferred_shot_tests {
    use super::*;
    use rfs_core::entity::StationType;
    use rfs_damage::damage::DamageSystem;
    use rfs_ship::loader::create_default_frigate;
    use rfs_ship::ship::ShipConfig;
    use std::sync::Arc;

    use crate::match_::SHIP_ID;

    fn frigate() -> Ship {
        let ship_config: ShipConfig = create_default_frigate().into();
        Ship::new(
            ship_config,
            EntityId::new(SHIP_ID),
            Arc::new(DamageSystem::new()),
        )
    }

    fn the_gun(ship: &Ship) -> EntityId {
        ship.get_state()
            .station_states
            .iter()
            .find(|(_, s)| s.station_type == StationType::Gun)
            .map(|(id, _)| *id)
            .expect("the default frigate has a gun station")
    }

    /// Bug №630: a deferred shot must not fire on the tick it was booked —
    /// the turret is, by construction, still somewhere else — and must fire
    /// exactly when the slew brings the turret onto the commanded angles,
    /// spending exactly one round.
    #[test]
    fn a_deferred_shot_fires_only_when_the_turret_arrives() {
        let mut sim = TickSystem::new();
        let ship = frigate();
        let ship_id = ship.entity_id();
        let station = the_gun(&ship);
        ship.set_station_target_angles(station, std::f32::consts::FRAC_PI_2, 0.0);
        sim.add_ship(ship);
        let ship = sim.get_ship(ship_id).expect("ship registered");
        let ammo_full = ship.get_state().station_states[&station].ammo_count;

        let mut pending = Vec::new();
        defer_shot(&mut pending, ship_id, station);
        sim.tick();
        resolve_deferred_shots(&sim, &mut pending);

        assert_eq!(
            pending.len(),
            1,
            "one slew step (a fraction of a radian) cannot cover the aim; the shot must stay pending"
        );
        assert!(
            sim.get_active_projectiles().is_empty(),
            "a deferred shot must not fire while the turret is on its way"
        );
        assert_eq!(
            sim.get_ship(ship_id)
                .expect("ship registered")
                .get_state()
                .station_states[&station]
                .ammo_count,
            ammo_full,
            "no round is consumed until the turret arrives"
        );

        for _ in 0..60 {
            sim.tick();
            resolve_deferred_shots(&sim, &mut pending);
            if pending.is_empty() {
                break;
            }
        }
        let ship = sim.get_ship(ship_id).expect("ship registered");
        assert!(
            ship.station_aim_reached(station),
            "the frigate's slowest turret crosses the aim well inside 60 ticks"
        );
        assert!(pending.is_empty(), "arrival must consume the deferred shot");
        assert_eq!(
            sim.get_active_projectiles().len(),
            1,
            "the arrival must spawn exactly one shell"
        );
        assert_eq!(
            ship.get_state().station_states[&station].ammo_count,
            ammo_full - 1,
            "the arrival spent exactly one round"
        );
    }

    /// Bug №630: a turret that can no longer slew — the station is destroyed,
    /// so `Station::update` returns before its slew step — must not hold its
    /// shot forever. The deadline refuses it, visibly, instead of queueing a
    /// shot that can never find its angles.
    #[test]
    fn a_deferred_shot_is_refused_at_its_deadline() {
        let mut sim = TickSystem::new();
        let ship = frigate();
        let ship_id = ship.entity_id();
        let station = the_gun(&ship);
        ship.set_station_target_angles(station, std::f32::consts::FRAC_PI_2, 0.0);
        let mut state = ship.get_state();
        state
            .station_states
            .get_mut(&station)
            .expect("seat exists")
            .health = 0.0;
        ship.apply_state(state);
        sim.add_ship(ship);

        let mut pending = Vec::new();
        defer_shot(&mut pending, ship_id, station);
        // A compressed deadline: the logic under test is the expiry, not the
        // length of the production window.
        pending[0].ttl = 3;

        for _ in 0..4 {
            sim.tick();
            resolve_deferred_shots(&sim, &mut pending);
        }
        assert!(
            pending.is_empty(),
            "the deadline must refuse the shot instead of queueing it forever"
        );
        assert!(
            sim.get_active_projectiles().is_empty(),
            "a refused shot must not have fired"
        );
    }
}
