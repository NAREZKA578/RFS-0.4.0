use rfs_core::entity::EntityId;
use rfs_core::math::{Vec3f, Transform};
use rfs_core::packet::ProjectileType;
use rfs_core::time::{Tick, FIXED_DT};
use rfs_ship::ship::{Ship, ShipState};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::ballistics::{BallisticsCalculator, BallisticsConfig};

pub struct TickSystem {
    ships: RwLock<HashMap<EntityId, Arc<Ship>>>,
    current_tick: Tick,
    projectiles: RwLock<Vec<Projectile>>,
    collisions: RwLock<Vec<CollisionEvent>>,
    /// Damage found by last tick's collision pass, applied at this tick's
    /// boundary (plan §5.1: stage-2 results never touch half-computed state).
    pending_damage: RwLock<Vec<PendingDamage>>,
    ballistics: BallisticsCalculator,
    /// Bug №196: projectiles refused by `fire_projectile`. The old comment
    /// claimed a counter existed; it did not, so a dropped shot was invisible.
    dropped_projectiles: std::sync::atomic::AtomicU64,
    /// Projectiles spent on a world obstacle rather than on a ship.
    ///
    /// Obstacle health is not modelled, so this is the only observable trace of a
    /// shell that hit a rock. Without it, a map full of asteroids would silently
    /// swallow every shot aimed at one.
    obstacle_hits: std::sync::atomic::AtomicU64,
    /// The world, if one is loaded. `None` means "no map": projectiles pass
    /// through empty space and ships are held nowhere. That is the state every
    /// test starts in, so a missing map cannot break one.
    world: RwLock<Option<crate::map::WorldMap>>,
}

/// Why a projectile was not accepted.
///
/// Bug №196: `fire_projectile` returned `()` and dropped silently, so a client
/// whose shot hit the cap got no reply and the command looked successful. The
/// caller now has something to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectileRejected {
    /// Position, velocity or lifetime was not finite, which would create a shell
    /// that can never be culled.
    NonFiniteBallistics,
    /// The server already holds `MAX_PROJECTILES`.
    AtCapacity,
}

impl ProjectileRejected {
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectileRejected::NonFiniteBallistics => "non-finite ballistics",
            ProjectileRejected::AtCapacity => "projectile limit reached",
        }
    }
}

/// Bug №607: how hard an explosive hit sets the struck compartment alight.
///
/// The live fire model — spread, decay, and the `FireStarted` /
/// `FireExtinguished` events — all existed and ticked, but nothing on the live
/// path ever raised `fire_intensity` above zero: every ignition call lived in a
/// test. An explosive shell now lights the compartment it lands in. 0.5 is one
/// half of full intensity: below it a hit does not instantly disable a station
/// (`ship/src/station.rs` reads `fire_intensity > 0.5` as "out of action"), and
/// it is inside `set_fire`'s `[0, 1]` clamp.
pub const FIRE_INTENSITY_ON_EXPLOSIVE_HIT: f32 = 0.5;

#[derive(Debug, Clone)]
pub struct PendingDamage {
    pub target: EntityId,
    pub projectile: EntityId,
    pub damage: f32,
    pub penetration: f32,
    pub position: Vec3f,
    pub normal: Vec3f,
    /// Bug №607: whether the shell that produced this entry was explosive, and
    /// so should set the struck compartment on fire when the damage lands.
    pub incendiary: bool,
}

#[derive(Debug, Clone)]
pub struct Projectile {
    pub entity_id: EntityId,
    pub projectile_type: ProjectileType,
    pub position: Vec3f,
    /// Position at the end of the previous tick. Together with `position`
    /// it forms the swept segment used by the hit test (plan §4: a fast
    /// shell must not tunnel through a thin target between two ticks).
    pub prev_position: Vec3f,
    pub velocity: Vec3f,
    pub spawn_tick: Tick,
    pub lifetime: f32,
    pub max_lifetime: f32,
    /// Accumulated flight path length, for max_range expiry (plan §4).
    pub distance_traveled: f32,
    pub owner: EntityId,
    pub weapon: EntityId,
    pub damage: f32,
    pub penetration: f32,
    pub explosion_radius: f32,
}

/// How far inside the map face a ship's centre is kept.
///
/// Not zero: a ship clamped exactly onto the boundary is outside on the next
/// floating-point step, so it would be clamped again every tick and the velocity
/// damping would fire forever.
pub const SHIP_BOUNDS_MARGIN: f32 = 1.0;

/// What a projectile ran into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionTarget {
    /// Another ship. Existing behaviour.
    Ship,
    /// A solid volume in the world map.
    ///
    /// Nothing in the world takes damage yet — obstacle health is not modelled —
    /// so the kind travels with the event only so a caller does not have to look
    /// it up, and so adding damage later has somewhere to read it from.
    Obstacle(crate::map::ObstacleKind),
}

#[derive(Debug, Clone)]
pub struct CollisionEvent {
    pub tick: Tick,
    pub projectile: EntityId,
    pub target: EntityId,
    /// What `target` is, so a caller does not have to look it up to decide
    /// whether to apply damage.
    pub target_kind: CollisionTarget,
    pub position: Vec3f,
    pub normal: Vec3f,
    pub velocity: Vec3f,
    /// How far into the target the shell ended up, 0..=1. A graze is shallower
    /// than a square hit, and only meaningful for obstacles.
    pub penetration: f32,
}

impl Default for TickSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl TickSystem {
    pub fn new() -> Self {
        Self {
            ships: RwLock::new(HashMap::new()),
            current_tick: Tick(0),
            projectiles: RwLock::new(Vec::new()),
            collisions: RwLock::new(Vec::new()),
            pending_damage: RwLock::new(Vec::new()),
            ballistics: BallisticsCalculator::new(BallisticsConfig::default()),
            dropped_projectiles: std::sync::atomic::AtomicU64::new(0),
            obstacle_hits: std::sync::atomic::AtomicU64::new(0),
            world: RwLock::new(None),
        }
    }

    pub fn tick(&mut self) -> TickResult {
        self.current_tick = self.current_tick.next();
        
        let mut result = TickResult {
            tick: self.current_tick,
            ship_updates: Vec::new(),
            projectile_updates: Vec::new(),
            collisions: Vec::new(),
            events: Vec::new(),
        };

// Stage 2 fallout of the PREVIOUS tick lands first, on a settled state.
        self.apply_pending_damage(&mut result);
        self.update_ships_parallel(&mut result);
        self.hold_ships_in_bounds();
        self.update_projectiles(&mut result);
        self.check_collisions(&mut result);
        self.collect_damage();

        result
    }

    /// Keeps every ship inside the loaded map.
    ///
    /// Two things are corrected: the position, and the velocity component that
    /// was carrying it out. Moving the position without stopping the velocity is
    /// the version that leaves a ship pinned against the boundary at full speed,
    /// which reads as a collision bug rather than a wall.
    ///
    /// Clamps by a small margin rather than exactly to the face: a ship parked
    /// exactly on the boundary is outside on the next floating-point step, so it
    /// would oscillate.
    ///
    /// Only ships whose *centre* has left the volume are touched. Pushing the
    /// hull back in as well would make a wide ship unable to fly along a wall,
    /// and there is no contact model here that could tell "resting against" from
    /// "embedded in".
    fn hold_ships_in_bounds(&self) {
        let world = self.world.read();
        let Some(map) = world.as_ref() else {
            return;
        };
        let margin = SHIP_BOUNDS_MARGIN;
        // Bug №611: this used to clone the whole map — obstacles included — and
        // then build an inset map with `obstacles: Vec::new()`, so the clone's
        // entire payload was discarded one line later. `clamp_point` only reads
        // `bounds`, which is two `Vec3f`; it now takes them directly.
        let inset = rfs_core::math::Bounds::new(
            map.bounds.min + Vec3f::new(margin, margin, margin),
            map.bounds.max - Vec3f::new(margin, margin, margin),
        );

        for ship in self.ships.read().values() {
            let mut state = ship.get_state();
            let (position, moved) = crate::map::clamp_point_into(&inset, state.transform.position);
            if !moved {
                continue;
            }
            // Remove only the component of the velocity pointing back out of the
            // volume. Done per axis from the raw components because `Vec3f` has no
            // element setter, and rebuilding the whole vector from a copied array
            // is clearer than three conditional assignments to a field.
            let outside = [state.transform.position.x - position.x,
                state.transform.position.y - position.y,
                state.transform.position.z - position.z];
            let v = [state.velocity.x, state.velocity.y, state.velocity.z];
            let mut damped = v;
            for axis in 0..3 {
                if (outside[axis] > 0.0 && v[axis] > 0.0) || (outside[axis] < 0.0 && v[axis] < 0.0) {
                    damped[axis] = 0.0;
                }
            }
            state.transform.position = position;
            state.velocity = Vec3f::new(damped[0], damped[1], damped[2]);
            ship.apply_state(state);
        }
    }

    /// Installs the world map. Replaces any previous one.
    ///
    /// The map is validated on the way in, so an invalid map is refused here
    /// rather than producing undefined collision behaviour for the rest of the
    /// process's life.
    pub fn set_world(&self, map: crate::map::WorldMap) -> Result<(), crate::map::MapError> {
        map.validated(&map.name)?;
        *self.world.write() = Some(map);
        Ok(())
    }

    /// The loaded map, if any.
    pub fn world(&self) -> Option<crate::map::WorldMap> {
        self.world.read().clone()
    }

    fn update_ships_parallel(&self, result: &mut TickResult) {
        let ships = self.ships.read();

        // Bug №153: the pre-tick state is captured so flooding/occupancy
        // transitions are detected edge-triggered — the old code never emitted
        // CompartmentFlooded/StationOccupied/StationVacated at all.
        let updates: Vec<_> = ships.par_iter()
            .map(|(id, ship)| {
                let prev = ship.get_state();
                let mut ship_state = prev.clone();
                ship.update(FIXED_DT, &mut ship_state);
                (*id, prev, ship_state)
            })
            .collect();

        drop(ships);

        let mut ships = self.ships.write();
        for (id, prev, state) in updates {
            // Bug №57: a ship that finished sinking leaves the simulation —
            // the old code kept it around forever with an empty sink-timer
            // body.
            if state.has_sunk {
                ships.remove(&id);
                result.events.push(GameEvent::ShipSunk {
                    ship: id,
                    position: state.transform.position,
                });
                continue;
            }
            if let Some(ship) = ships.get(&id) {
                Self::emit_state_transitions(&prev, &state, &mut result.events);
                ship.apply_state(state);
                result.ship_updates.push(ShipUpdate {
                    entity_id: id,
                    transform: ship.transform(),
                    velocity: ship.velocity(),
                    angular_velocity: ship.angular_velocity(),
                    health: ship.health(),
                    fuel: ship.fuel(),
                    speed: ship.speed(),
                    heading: ship.heading(),
                    rudder_angle: ship.rudder_angle(),
                    throttle: ship.throttle(),
                });
            }
        }
    }

    /// Bug №153: fire each CompartmentFlooded/StationOccupied/StationVacated
    /// exactly once, on the crossing edge, never on every tick.
    fn emit_state_transitions(prev: &ShipState, next: &ShipState, out: &mut Vec<GameEvent>) {
        // Bug №612: this built a fresh `HashMap<EntityId, bool>` per ship per
        // tick out of `prev.compartment_states` — which is already keyed by
        // entity id — and then read it back with `get`. One allocation per ship
        // per tick to look up what a map lookup answers directly. The station
        // loop below was already doing the direct lookup; this is the same thing.
        for (cid, c) in &next.compartment_states {
            let was_flooded = prev
                .compartment_states
                .get(cid)
                .map(|p| p.is_flooded())
                .unwrap_or(false);
            if !was_flooded && c.is_flooded() {
                out.push(GameEvent::CompartmentFlooded {
                    compartment: *cid,
                    water_level: c.water_level,
                });
            }
        }
        for (sid, s) in &next.station_states {
            let prev_occ = prev.station_states.get(sid).and_then(|x| x.occupant);
            match (prev_occ, s.occupant) {
                (None, Some(p)) => out.push(GameEvent::StationOccupied {
                    station: *sid,
                    player: p,
                }),
                (Some(p), None) => out.push(GameEvent::StationVacated {
                    station: *sid,
                    player: p,
                }),
                _ => {}
            }
        }
    }

    fn update_projectiles(&self, result: &mut TickResult) {
        let ballistics = &self.ballistics;
        let mut projectiles = self.projectiles.write();
        let dt = FIXED_DT;
        
        projectiles.retain_mut(|proj| {
            // Non-finite or non-positive lifetime config: expire immediately
            // instead of living forever (NaN comparisons are always false).
            if !proj.max_lifetime.is_finite() || proj.max_lifetime <= 0.0 {
                return false;
            }
            proj.lifetime += dt;
            if proj.lifetime >= proj.max_lifetime {
                return false;
            }

            // Bug №608: both integrators freeze a projectile below
            // RESTING_SPEED and hand it straight back, so nothing else ever
            // retired it — the shell stopped where it was and was still in the
            // world, replicated to every client, for the rest of its 12 s life.
            // `max_lifetime` is a ceiling, not a schedule: a shell *entered at
            // rest* is spent.
            //
            // The test is on entry, not on the integrated velocity, and the
            // difference is not cosmetic. A shell hitting water at speed stops
            // dead in a single step (`clamp_no_reverse`, water drag 2.37·v²), so
            // a check on `new_vel` retires it on the very tick it arrives — and
            // the stopped position is the thing the impact oracle exists to see
            // (`underwater_shell_stops_dead_no_teleport` asserts the shell is
            // still there, still where it was). It is the *following* tick,
            // entered already at rest with nothing left to add, that has no
            // reason to exist.
            if proj.velocity.length() < crate::ballistics::RESTING_SPEED {
                return false;
            }

            proj.prev_position = proj.position;
            let (new_pos, new_vel) = match ballistics.get_config(proj.projectile_type) {
                Some(config) => {
                    // Strictly below the surface: buoyancy + water drag.
                    // y == 0 is still air (sea-skimmers must not teleport).
                    let step = if proj.position.y < 0.0 {
                        ballistics.simulate_underwater(proj.position, proj.velocity, config, dt)
                    } else {
                        ballistics.integrate_step(proj.position, proj.velocity, config, dt)
                    };
                    proj.distance_traveled += (step.0 - proj.position).length();
                    if config.max_range > 0.0 && proj.distance_traveled >= config.max_range {
                        return false;
                    }
                    step
                }
                // Unknown type: legacy gravity Euler, lifetime-bounded.
                None => {
                    let gravity = Vec3f::new(0.0, -9.81, 0.0);
                    let new_vel = proj.velocity + gravity * dt;
                    let new_pos = proj.position + proj.velocity * dt;
                    (new_pos, new_vel)
                }
            };
            proj.velocity = new_vel;
            proj.position = new_pos;

            result.projectile_updates.push(ProjectileUpdate {
                entity_id: proj.entity_id,
                position: proj.position,
                velocity: proj.velocity,
                lifetime: proj.lifetime,
            });

            true
        });
    }

    fn check_collisions(&self, result: &mut TickResult) {
        // Guards, not copies (Bug №611): this pass only READS the projectiles and
        // the world, and it used to `clone()` both of them every tick — the whole
        // projectile list, and the whole map with every obstacle in it — purely to
        // drop them at the end of the function. The locks are released before the
        // results are published, so nothing is held across the write.
        let projectiles = self.projectiles.read();
        let ships: Vec<Arc<Ship>> = self.ships.read().values().cloned().collect();
        let world = self.world.read();
        let map = world.as_ref();

        let mut collisions = Vec::new();

        for proj in projectiles.iter() {
            // Obstacles first. A shell that reaches a rock before a ship behind it
            // should hit the rock, and testing the world before the fleet is what
            // makes that hold without comparing depths afterwards.
            let obstacle_hit = map.as_ref().and_then(|m| m.swept_obstacle(proj.prev_position, proj.position));
            if let Some((target, kind, position, penetration)) = obstacle_hit {
                collisions.push(CollisionEvent {
                    tick: self.current_tick,
                    projectile: proj.entity_id,
                    target,
                    target_kind: CollisionTarget::Obstacle(*kind),
                    position,
                    normal: Self::box_impact_normal(proj.prev_position, position),
                    velocity: proj.velocity,
                    penetration,
                });
                // The shell is spent either way; `collect_damage` despawns every
                // projectile that reported a collision.
                continue;
            }

            for ship in ships.iter() {
                if ship.entity_id() == proj.owner {
                    continue;
                }

                let bounds = ship.bounds();
                if segment_intersects_bounds(proj.prev_position, proj.position, &bounds) {
                    // Where the shell struck, not where the tick ended. The hull
                    // box a shell can hit is far larger than the compartments
                    // inside it, so the end of the segment sits outside every
                    // damage falloff radius and the hit would deal nothing.
                    let impact =
                        segment_entry_point(proj.prev_position, proj.position, &bounds)
                            .unwrap_or(proj.position);
                    let normal = Self::calculate_impact_normal(ship, impact);
                    collisions.push(CollisionEvent {
                        tick: self.current_tick,
                        projectile: proj.entity_id,
                        target: ship.entity_id(),
                        target_kind: CollisionTarget::Ship,
                        position: impact,
                        normal,
                        velocity: proj.velocity,
                        penetration: 1.0,
                    });
                    // One hit per projectile per tick is enough; the shell is
                    // despawned in collect_damage below.
                    break;
                }
            }
        }

        // Published only after both guards are gone: the tick keeps its history to
        // exactly one entry, so there is no unbounded growth to justify holding a
        // read lock across the write below.
        drop(projectiles);
        drop(world);

        result.collisions = collisions.clone();
        *self.collisions.write() = collisions;
    }

    /// Impact normal for a hit against an axis-aligned box.
    ///
    /// The dominant axis of the offset from the box centre, pushed back out.
    /// For a dead-centre hit every axis ties, and the result falls back to `+Y`,
    /// which is the least surprising direction for a top-down impact.
    fn box_impact_normal(prev: Vec3f, impact: Vec3f) -> Vec3f {
        let back = prev - impact;
        let a = [back.x.abs(), back.y.abs(), back.z.abs()];
        let mut normal = [0.0f32; 3];
        let mut best = 0usize;
        for axis in 1..3 {
            if a[axis] > a[best] {
                best = axis;
            }
        }
        if a[best] <= 1e-9 {
            normal[1] = 1.0;
            return Vec3f::new(normal[0], normal[1], normal[2]);
        }
        normal[best] = if back.x == 0.0 && back.y == 0.0 && back.z == 0.0 {
            1.0
        } else {
            [back.x, back.y, back.z][best] / a[best]
        };
        Vec3f::new(normal[0], normal[1], normal[2])
    }

    /// Impact normal in the WORLD frame: pick the dominant axis in the ship's
    /// local frame, then rotate back. Falls back to up on a dead-center hit.
    fn calculate_impact_normal(ship: &Ship, impact_pos: Vec3f) -> Vec3f {
        let local_pos = ship.world_to_local(impact_pos);
        let bounds = ship.bounds();
        // bounds() is a world AABB: bring its center into the local frame so
        // both sides of the comparison live in the same space.
        let center = ship.world_to_local(bounds.center());
        let diff = local_pos - center;

        let abs_diff = Vec3f::new(diff.x.abs(), diff.y.abs(), diff.z.abs());

        let local_normal = if abs_diff.x >= abs_diff.y && abs_diff.x >= abs_diff.z {
            Vec3f::new(diff.x.signum(), 0.0, 0.0)
        } else if abs_diff.y >= abs_diff.x && abs_diff.y >= abs_diff.z {
            Vec3f::new(0.0, diff.y.signum(), 0.0)
        } else {
            Vec3f::new(0.0, 0.0, diff.z.signum())
        };

        if local_normal == Vec3f::ZERO {
            return Vec3f::new(0.0, 1.0, 0.0);
        }
        ship.transform().rotation.mul_vec3(local_normal)
    }

    fn collect_damage(&self) {
        // Snapshot everything first, then mutate: short separate locks in a
        // fixed order (collisions -> projectiles), no nesting. Damage itself
        // is NOT applied here — it lands at the next tick boundary
        // (see apply_pending_damage).
        // Read guards, not copies (Bug №611): this pass reads the projectile list
        // and the collision history, and cloned both every tick. Both guards are
        // released before the projectile list is mutated below, so the write
        // still happens with nothing held.
        let collisions = self.collisions.read();
        if collisions.is_empty() {
            return;
        }

        let projectiles = self.projectiles.read();

        let mut pending: Vec<PendingDamage> = Vec::new();
        let mut obstacle_hits = 0u64;
        for collision in collisions.iter() {
            let Some(proj) = projectiles.iter().find(|p| p.entity_id == collision.projectile) else {
                continue;
            };
            match collision.target_kind {
                CollisionTarget::Ship => {
                    pending.push(PendingDamage {
                        target: collision.target,
                        projectile: collision.projectile,
                        damage: proj.damage,
                        penetration: proj.penetration,
                        position: collision.position,
                        normal: collision.normal,
                        // Bug №607: only an explosive shell starts a fire, and
                        // only on the ship it actually struck.
                        incendiary: proj.explosion_radius.is_finite()
                            && proj.explosion_radius > 0.0,
                    });
                }
                CollisionTarget::Obstacle(_) => {
                    // Not queued. `apply_pending_damage` looks the target up among
                    // ships and would silently drop it, so queueing it here would
                    // cost a lookup to achieve nothing - and the silence is the
                    // problem, not the cost. Nothing tracks asteroid health yet,
                    // so a rock cannot be hurt; what *is* real is the shell being
                    // spent on it, and that has to be countable rather than
                    // invisible. Same lesson as Bug #196, where a missing counter
                    // made dropped shots impossible to see.
                    obstacle_hits += 1;
                }
            }
        }
        if obstacle_hits > 0 {
            self.obstacle_hits
                .fetch_add(obstacle_hits, std::sync::atomic::Ordering::Relaxed);
        }

        // Bug №155: explosive shells splash into NEIGHBOURING ships. The hit
        // ship takes the full blast via the direct entry above; everyone else
        // inside explosion_radius takes falling damage from the burst point.
        let has_splash = collisions.iter().any(|c| {
            projectiles
                .iter()
                .any(|p| p.entity_id == c.projectile && p.explosion_radius.is_finite())
        });
        if has_splash {
            let ships: Vec<Arc<Ship>> = self.ships.read().values().cloned().collect();
            for collision in collisions.iter() {
                let Some(proj) = projectiles
                    .iter()
                    .find(|p| p.entity_id == collision.projectile)
                else {
                    continue;
                };
                if proj.explosion_radius.is_nan() || proj.explosion_radius <= 0.0 {
                    continue;
                }
                for ship in &ships {
                    if ship.entity_id() == collision.target || ship.entity_id() == proj.owner {
                        continue;
                    }
                    let dist = distance_point_to_bounds(collision.position, &ship.bounds());
                    if !dist.is_finite() || dist > proj.explosion_radius {
                        continue;
                    }
                    let factor = (1.0 - dist / proj.explosion_radius).clamp(0.0, 1.0);
                    let splash = proj.damage * factor * 0.5;
                    if splash > 0.0 && splash.is_finite() {
                        pending.push(PendingDamage {
                            target: ship.entity_id(),
                            projectile: collision.projectile,
                            damage: splash,
                            penetration: 0.0,
                            position: collision.position,
                            normal: collision.normal,
                            // Bug №607: splash reaches a neighbour's hull, but
                            // the recorded point is on the ship the shell
                            // struck, so it does not name a compartment here —
                            // a blast lights the ship it hit, not the ones
                            // nearby. (`hit_compartment` would be `None`.)
                            incendiary: false,
                        });
                    }
                }
            }
        }

        // A shell that hit *anything* is spent, and "hit" is decided by the
        // collision list - not by `pending`.
        //
        // Driving it from `pending` looks equivalent until a collision
        // produces no damage entry, which is exactly what an obstacle is: no
        // ship to damage, so nothing queued, so the shell survived the rock it
        // had just struck. The early return below made that reachable.
        let spent: Vec<EntityId> = collisions.iter().map(|c| c.projectile).collect();
        // Bug №611: the read guards are dropped before the projectile list is
        // mutated, so the write never happens with a read guard on the same
        // lock. `spent` is what the old snapshot was cloned for.
        drop(projectiles);
        drop(collisions);

        if !spent.is_empty() {
            self.projectiles
                .write()
                .retain(|p| !spent.contains(&p.entity_id));
        }

            if pending.is_empty() {
                return;
            }

            self.pending_damage.write().extend(pending);
        }

    /// Applies last tick's collision fallout to the settled state and emits
    /// the ShipHit events for THIS tick (plan §5.1).
    fn apply_pending_damage(&self, result: &mut TickResult) {
        let pending: Vec<PendingDamage> = std::mem::take(&mut *self.pending_damage.write());
        if pending.is_empty() {
            return;
        }

        let ships = self.ships.write();
        for hit in pending {
            if let Some(ship) = ships.get(&hit.target) {
                // Bug №607: the blast reports every compartment it reached, so
                // an explosive hit can light the one that took the most. The
                // point-in-box `hit_compartment` below cannot serve this — a
                // shell fired from outside stops on the hull box, which is far
                // larger than any compartment, so it names none.
                let inflicted = ship.apply_damage(hit.damage, hit.position);

                // Bug №161: hit_compartment was always None — the shell hit a
                // ship but the event never told the client which compartment.
                let hit_compartment = ship
                    .get_compartment_at(ship.world_to_local(hit.position))
                    .or_else(|| ship.get_compartment_at(hit.position));

                // Bug №272: the same information, but into the ship LAYER, so
                // it survives a lost event packet. Events went unreliable
                // because they carry no state — but this normal and this
                // compartment were the only parts of a hit that no layer
                // carried, so without this the impact effect and the
                // survivability feedback would depend on a packet that can be
                // dropped. Everything else the event reports is already in a
                // layer and needed no change.
                ship.note_hit(
                    self.current_tick.value(),
                    hit.position,
                    hit.normal,
                    hit_compartment,
                    hit.damage,
                );

                result.events.push(GameEvent::ShipHit {
                    target: hit.target,
                    projectile: hit.projectile,
                    position: hit.position,
                    normal: hit.normal,
                    damage: hit.damage,
                    penetration: hit.penetration,
                    hit_compartment,
                });

                // Bug №607: an explosive hit lights the compartment the blast
                // reached hardest. `ignite_compartment` returns true only for a
                // compartment that was not already burning, so a second shell
                // into a live fire does not spam `FireStarted`.
                if hit.incendiary {
                    if let Some(&(compartment, _)) =
                        inflicted.iter().max_by(|a, b| a.1.total_cmp(&b.1))
                    {
                        if ship.ignite_compartment(compartment, FIRE_INTENSITY_ON_EXPLOSIVE_HIT) {
                            result.events.push(GameEvent::FireStarted {
                                compartment,
                                intensity: FIRE_INTENSITY_ON_EXPLOSIVE_HIT,
                            });
                        }
                    }
                }
            }
        }
    }

    pub fn add_ship(&self, ship: Ship) {
        self.ships.write().insert(ship.entity_id(), Arc::new(ship));
    }

    pub fn remove_ship(&self, entity_id: EntityId) {
        self.ships.write().remove(&entity_id);
    }

    pub fn get_ship(&self, entity_id: EntityId) -> Option<Arc<Ship>> {
        self.ships.read().get(&entity_id).cloned()
    }

    /// Spawns a projectile, or reports why it was refused.
    pub fn fire_projectile(&self, projectile: Projectile) -> Result<(), ProjectileRejected> {
        // Unbounded spawn is a DoS/OOM (256 clients x 30Hz x 12s), so the count is
        // capped. Reject non-finite ballistics too: they would create immortal shells.
        if !projectile.position.is_finite()
            || !projectile.velocity.is_finite()
            || !projectile.lifetime.is_finite()
        {
            self.dropped_projectiles.fetch_add(1, Ordering::Relaxed);
            return Err(ProjectileRejected::NonFiniteBallistics);
        }
        const MAX_PROJECTILES: usize = 8192;
        let mut guard = self.projectiles.write();
        if guard.len() >= MAX_PROJECTILES {
            drop(guard);
            // Bug №196: this counter is what the old comment claimed existed. It is
            // now real, so a rejection is observable in a log or a stat instead of
            // looking like a successful shot.
            self.dropped_projectiles.fetch_add(1, Ordering::Relaxed);
            return Err(ProjectileRejected::AtCapacity);
        }
        guard.push(projectile);
        Ok(())
    }

/// How many projectiles have been refused since start-up.
///
/// Bug №196: the old comment described exactly this counter, but no such value
/// was kept — the drop was unobservable.
pub fn dropped_projectiles(&self) -> u64 {
        self.dropped_projectiles.load(Ordering::Relaxed)
    }

    /// Projectiles spent on a world obstacle.
    pub fn obstacle_hits(&self) -> u64 {
        self.obstacle_hits.load(Ordering::Relaxed)
    }

    pub fn current_tick(&self) -> Tick {
        self.current_tick
    }

    pub fn get_active_projectiles(&self) -> Vec<Projectile> {
        self.projectiles.read().clone()
    }

    pub fn get_collisions_since(&self, since_tick: Tick) -> Vec<CollisionEvent> {
        self.collisions.read().iter()
            .filter(|c| c.tick > since_tick)
            .cloned()
            .collect()
    }
}

/// Swept point-vs-AABB test over one tick segment (plan §4: the shell must
/// not tunnel through a thin target between two ticks). Slab method with
/// explicit parallel-axis handling, so no NaN on zero-length segments.
pub fn segment_intersects_bounds(prev: Vec3f, curr: Vec3f, bounds: &rfs_core::math::Bounds) -> bool {
    let dir = curr - prev;
    let mut tmin = 0.0f32;
    let mut tmax = 1.0f32;

    macro_rules! axis {
        ($p:expr, $d:expr, $mn:expr, $mx:expr) => {{
            if $d.abs() < 1e-9 {
                if $p < $mn || $p > $mx {
                    return false;
                }
            } else {
                let inv = 1.0 / $d;
                let (mut t0, mut t1) = (($mn - $p) * inv, ($mx - $p) * inv);
                if t0 > t1 {
                    std::mem::swap(&mut t0, &mut t1);
                }
                if t0 > tmin {
                    tmin = t0;
                }
                if t1 < tmax {
                    tmax = t1;
                }
                if tmin > tmax {
                    return false;
                }
            }
        }};
    }

    axis!(prev.x, dir.x, bounds.min.x, bounds.max.x);
    axis!(prev.y, dir.y, bounds.min.y, bounds.max.y);
    axis!(prev.z, dir.z, bounds.min.z, bounds.max.z);
    true
}

/// Where a swept segment first enters a box. The same slab test as
/// [`segment_intersects_bounds`], keeping the entry parameter instead of
/// throwing it away.
///
/// This is the point a shell actually strikes, not the end of the tick in which
/// it noticed. The difference is the whole damage model: the box a shell can
/// hit (the hull) is much larger than the boxes that can be hurt (the
/// compartments inside it), so measuring from the end of the segment puts the
/// impact outside every falloff radius and the shell deals nothing at all.
///
/// A segment that starts inside the box enters it at `prev`, which is the right
/// answer there too.
pub fn segment_entry_point(
    prev: Vec3f,
    curr: Vec3f,
    bounds: &rfs_core::math::Bounds,
) -> Option<Vec3f> {
    let dir = curr - prev;
    let mut tmin = 0.0f32;
    let mut tmax = 1.0f32;

    macro_rules! axis {
        ($p:expr, $d:expr, $mn:expr, $mx:expr) => {{
            if $d.abs() < 1e-9 {
                if $p < $mn || $p > $mx {
                    return None;
                }
            } else {
                let inv = 1.0 / $d;
                let (mut t0, mut t1) = (($mn - $p) * inv, ($mx - $p) * inv);
                if t0 > t1 {
                    std::mem::swap(&mut t0, &mut t1);
                }
                if t0 > tmin {
                    tmin = t0;
                }
                if t1 < tmax {
                    tmax = t1;
                }
                if tmin > tmax {
                    return None;
                }
            }
        }};
    }

    axis!(prev.x, dir.x, bounds.min.x, bounds.max.x);
    axis!(prev.y, dir.y, bounds.min.y, bounds.max.y);
    axis!(prev.z, dir.z, bounds.min.z, bounds.max.z);
    Some(prev + dir * tmin.clamp(0.0, 1.0))
}

/// Distance from a point to the nearest point of an axis-aligned box
/// (0 when inside). NaN-safe: a non-finite input returns f32::INFINITY.
fn distance_point_to_bounds(point: Vec3f, bounds: &rfs_core::math::Bounds) -> f32 {
    if !point.is_finite()
        || !bounds.min.x.is_finite()
        || !bounds.min.y.is_finite()
        || !bounds.min.z.is_finite()
        || !bounds.max.x.is_finite()
        || !bounds.max.y.is_finite()
        || !bounds.max.z.is_finite()
    {
        return f32::INFINITY;
    }
    let dx = point.x.clamp(bounds.min.x, bounds.max.x) - point.x;
    let dy = point.y.clamp(bounds.min.y, bounds.max.y) - point.y;
    let dz = point.z.clamp(bounds.min.z, bounds.max.z) - point.z;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[derive(Debug, Clone)]
pub struct TickResult {
    pub tick: Tick,
    pub ship_updates: Vec<ShipUpdate>,
    pub projectile_updates: Vec<ProjectileUpdate>,
    pub collisions: Vec<CollisionEvent>,
    pub events: Vec<GameEvent>,
}

#[derive(Debug, Clone)]
pub struct ShipUpdate {
    pub entity_id: EntityId,
    pub transform: Transform,
    pub velocity: Vec3f,
    pub angular_velocity: Vec3f,
    pub health: f32,
    pub fuel: f32,
    pub speed: f32,
    pub heading: f32,
    pub rudder_angle: f32,
    pub throttle: f32,
}

#[derive(Debug, Clone)]
pub struct ProjectileUpdate {
    pub entity_id: EntityId,
    pub position: Vec3f,
    pub velocity: Vec3f,
    pub lifetime: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    ShipHit {
        target: EntityId,
        projectile: EntityId,
        position: Vec3f,
        normal: Vec3f,
        damage: f32,
        penetration: f32,
        hit_compartment: Option<EntityId>,
    },
    ShipSunk {
        ship: EntityId,
        position: Vec3f,
    },
    CompartmentFlooded {
        compartment: EntityId,
        water_level: f32,
    },
    StationOccupied {
        station: EntityId,
        player: EntityId,
    },
    StationVacated {
        station: EntityId,
        player: EntityId,
    },
    /// Bug №607: an explosive hit set a compartment on fire.
    FireStarted {
        compartment: EntityId,
        intensity: f32,
    },
    /// Bug №607: a compartment's fire was put out. The layer carries
    /// `fire_intensity` either way, so this event marks the moment rather than
    /// carrying state a client could not otherwise learn.
    FireExtinguished {
        compartment: EntityId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(id: u64) -> Projectile {
        Projectile {
            entity_id: EntityId::new(id),
            projectile_type: ProjectileType::Cannonball,
            position: Vec3f::ZERO,
            prev_position: Vec3f::ZERO,
            velocity: Vec3f::new(400.0, 0.0, 0.0),
            spawn_tick: Tick(0),
            lifetime: 0.0,
            max_lifetime: 12.0,
            distance_traveled: 0.0,
            owner: EntityId::new(1),
            weapon: EntityId::new(2001),
            damage: 100.0,
            penetration: 50.0,
            explosion_radius: 0.0,
        }
    }

    /// Bug №196: the old comment on this function promised "drop with a counter",
    /// and no counter existed. A refused shot was therefore unobservable, which
    /// is the whole of the defect — the drop itself was correct, its silence was
    /// not.
    #[test]
    fn a_refused_shell_is_counted() {
        let sim = TickSystem::new();
        assert_eq!(sim.dropped_projectiles(), 0);

        let bad = Projectile {
            position: Vec3f::new(f32::NAN, 0.0, 0.0),
            ..shell(1)
        };
        assert_eq!(
            sim.fire_projectile(bad),
            Err(ProjectileRejected::NonFiniteBallistics)
        );
        assert_eq!(
            sim.dropped_projectiles(),
            1,
            "the refusal was not counted"
        );
    }

    /// An accepted shell must not be counted, or the number means nothing.
    #[test]
    fn an_accepted_shell_is_not_counted() {
        let sim = TickSystem::new();
        assert_eq!(sim.fire_projectile(shell(1)), Ok(()));
        assert_eq!(sim.get_active_projectiles().len(), 1);
        assert_eq!(sim.dropped_projectiles(), 0);
    }

    /// The DoS cap has to actually refuse, and refuse with the reason that
    /// distinguishes it from a bad-geometry refusal.
    #[test]
    fn the_projectile_cap_refuses_and_is_visible() {
        let sim = TickSystem::new();
        const MAX: usize = 8192;
        for i in 0..MAX {
            assert_eq!(sim.fire_projectile(shell(i as u64 + 1)), Ok(()), "refused early at {i}");
        }

        assert_eq!(
            sim.fire_projectile(shell(99999)),
            Err(ProjectileRejected::AtCapacity),
            "the cap did not refuse the shell past its limit"
        );
        assert_eq!(sim.dropped_projectiles(), 1);
        assert_eq!(
            sim.get_active_projectiles().len(),
            MAX,
            "a refused shell was stored anyway"
        );
    }

    /// Both reasons must be distinguishable, since the caller reports one of
    /// them to the player.
    #[test]
    fn the_two_rejections_are_distinguishable() {
        assert_ne!(
            ProjectileRejected::NonFiniteBallistics,
            ProjectileRejected::AtCapacity
        );
        assert!(!ProjectileRejected::AtCapacity.as_str().is_empty());
        assert!(!ProjectileRejected::NonFiniteBallistics.as_str().is_empty());
        assert_ne!(
            ProjectileRejected::AtCapacity.as_str(),
            ProjectileRejected::NonFiniteBallistics.as_str()
        );
    }

    /// Each non-finite field is rejected on its own, so a NaN cannot sneak in
    /// through the one the guard forgets.
    #[test]
    fn every_non_finite_field_is_rejected() {
        let cases = [
            Projectile { position: Vec3f::new(f32::NAN, 0.0, 0.0), ..shell(1) },
            Projectile { position: Vec3f::new(f32::INFINITY, 0.0, 0.0), ..shell(2) },
            Projectile { velocity: Vec3f::new(f32::NAN, 0.0, 0.0), ..shell(3) },
            Projectile { velocity: Vec3f::new(0.0, f32::NEG_INFINITY, 0.0), ..shell(4) },
            Projectile { lifetime: f32::NAN, ..shell(5) },
        ];
        for (i, bad) in cases.into_iter().enumerate() {
            let sim = TickSystem::new();
            assert_eq!(
                sim.fire_projectile(bad),
                Err(ProjectileRejected::NonFiniteBallistics),
                "case {i} was accepted"
            );
            assert!(sim.get_active_projectiles().is_empty(), "case {i} was stored");
        }
    }

    /// Bug №43 was still listed as open, but the tick pipeline already deferred
    /// damage: `collect_damage` at the end of a tick queues into `pending_damage`,
    /// and `apply_pending_damage` runs first on the next tick's settled state
    /// (plan §5.1). This pins the ordering so it cannot regress into damaging a
    /// ship inside the same tick its collision was detected in — which is what
    /// made a client see one tick's update and its event disagree about health.
    #[test]
    fn damage_from_a_collision_lands_on_the_next_tick_not_the_same_one() {
        let mut sim = TickSystem::new();
        let start_health = {
            let ship = rfs_ship::ship::Ship::new(
                rfs_ship::ship::ShipConfig {
                    max_health: 1000.0,
                    // A ship with no compartments has no bounds, so nothing could
                    // ever hit it. One compartment centred on the origin gives
                    // the collision pass something to find.
                    compartments: vec![rfs_core::entity::CompartmentTemplate {
                        name: "hull".into(),
                        local_bounds: rfs_core::math::Bounds::new(
                            Vec3f::new(-20.0, -20.0, -20.0),
                            Vec3f::new(20.0, 20.0, 20.0),
                        ),
                        max_water_level: 100.0,
                        pump_capacity: 5.0,
                        connected_compartments: Vec::new(),
                        bulkheads: Vec::new(),
                        stations: Vec::new(),
                    }],
                    ..Default::default()
                },
                EntityId::new(1),
                std::sync::Arc::new(rfs_damage::DamageSystem::new()),
            );
            let health = ship.health();
            sim.add_ship(ship);
            health
        };
        let ship = sim
            .get_ship(EntityId::new(1))
            .expect("the ship was not registered");

        // A shell flying along +x straight through the hull, so the swept
        // segment crosses the compartment rather than starting inside it.
        sim.fire_projectile(Projectile {
            entity_id: EntityId::new(500),
            // A different owner: `check_collisions` skips a projectile whose
            // owner is the ship it would hit, so reusing the fixture's default
            // owner would silently test nothing.
            owner: EntityId::new(999),
            position: Vec3f::ZERO,
            prev_position: Vec3f::new(-30.0, 0.0, 0.0),
            velocity: Vec3f::new(400.0, 0.0, 0.0),
            damage: 250.0,
            ..shell(500)
        })
        .expect("the shell was accepted");

        // First tick: the collision is detected and queued.
        sim.tick();
        assert_eq!(
            ship.health(),
            start_health,
            "damage was applied in the same tick as the collision"
        );

        // Second tick: the queued damage lands.
        sim.tick();
        assert!(
            ship.health() < start_health,
            "the queued damage never landed: health {} vs {}",
            ship.health(),
            start_health
        );
    }

    /// The box a shell can hit and the boxes that can take damage are not the
    /// same size, and the gap between them is wider than the distance at which
    /// damage stops.
    ///
    /// The test above passes because its shell starts *inside* the hull's
    /// collision box, so the recorded hit point lands on the compartment and the
    /// full damage is dealt. A shell fired from outside stops at the collision
    /// box, which is nowhere near anything that can be damaged, and the falloff
    /// returns zero. This runs the path a player actually takes.
    #[test]
    fn a_shell_fired_at_a_ship_from_outside_actually_damages_it() {
        let mut sim = TickSystem::new();
        let ship = rfs_ship::ship::Ship::new(
            rfs_ship::ship::ShipConfig {
                max_health: 1000.0,
                // A compartment on the origin, which is 130 m from the hull's
                // collision box in x, 10 m in y and 130 m in z.
                compartments: vec![rfs_core::entity::CompartmentTemplate {
                    name: "hull".into(),
                    local_bounds: rfs_core::math::Bounds::new(
                        Vec3f::new(-20.0, -20.0, -20.0),
                        Vec3f::new(20.0, 20.0, 20.0),
                    ),
                    max_water_level: 100.0,
                    pump_capacity: 5.0,
                    connected_compartments: Vec::new(),
                    bulkheads: Vec::new(),
                    stations: Vec::new(),
                }],
                ..Default::default()
            },
            EntityId::new(1),
            std::sync::Arc::new(rfs_damage::DamageSystem::new()),
        );
        let start_health = ship.health();
        sim.add_ship(ship);
        let ship = sim
            .get_ship(EntityId::new(1))
            .expect("the ship was not registered");

        // Outside the hull's collision box, which spans x in [-50, 50]. Fired
        // from y = 10 rather than y = 0: the waterline is at y = 0, and a shell
        // that starts there falls below it on the first tick and is then flown
        // by water drag, which stops it where it stands.
        sim.fire_projectile(Projectile {
            entity_id: EntityId::new(500),
            owner: EntityId::new(999),
            position: Vec3f::new(-400.0, 10.0, 0.0),
            prev_position: Vec3f::new(-400.0, 10.0, 0.0),
            velocity: Vec3f::new(400.0, 0.0, 0.0),
            damage: 250.0,
            ..shell(500)
        })
        .expect("the shell was accepted");

        // Far enough ticks for a 250 m run at `FIXED_DT * 400` per tick, plus
        // the tick the queued damage lands on.
        let mut health = start_health;
        for _ in 0..60 {
            sim.tick();
            health = ship.health();
            if health < start_health {
                break;
            }
        }
        assert!(
            health < start_health,
            "a shell fired at the ship from 400 m dealt no damage at all: health {health} vs {start_health}"
        );
    }

    /// Bug №607, on the path a match actually runs: fire had no source at all.
    /// `set_fire` was called only from tests, so `fire_intensity` stayed zero for
    /// a whole match, `ExtinguishFire` acked a fire that never existed, and a
    /// compartment could never actually burn. An explosive shell must light the
    /// compartment its blast reached — and a second shell into a live fire must
    /// not report a fresh start.
    #[test]
    fn an_explosive_hit_lights_the_compartment_the_blast_reached() {
        let mut sim = TickSystem::new();
        let ship = rfs_ship::ship::Ship::new(
            rfs_ship::ship::ShipConfig {
                max_health: 1000.0,
                compartments: vec![rfs_core::entity::CompartmentTemplate {
                    name: "hull".into(),
                    local_bounds: rfs_core::math::Bounds::new(
                        Vec3f::new(-20.0, -20.0, -20.0),
                        Vec3f::new(20.0, 20.0, 20.0),
                    ),
                    max_water_level: 100.0,
                    pump_capacity: 5.0,
                    connected_compartments: Vec::new(),
                    bulkheads: Vec::new(),
                    stations: Vec::new(),
                }],
                ..Default::default()
            },
            EntityId::new(1),
            std::sync::Arc::new(rfs_damage::DamageSystem::new()),
        );
        sim.add_ship(ship);
        let ship = sim
            .get_ship(EntityId::new(1))
            .expect("the ship was not registered");
        let compartment = *ship
            .get_state()
            .compartment_states
            .keys()
            .next()
            .expect("the fixture has no compartment, so this would prove nothing");
        assert_eq!(
            ship.get_state().compartment_states[&compartment].fire_intensity,
            0.0,
            "the fixture starts on fire"
        );

        let mut starts: Vec<(EntityId, f32)> = Vec::new();
        for shot in 0..2u64 {
            // A shell fired from outside the hull box (x in [-50, 50]) with a
            // live blast radius, i.e. one that starts a fire. y = 10 keeps it
            // above the waterline so water drag does not stop it short.
            sim.fire_projectile(Projectile {
                entity_id: EntityId::new(500 + shot),
                // A different owner: `check_collisions` skips a projectile whose
                // owner is the ship it would hit.
                owner: EntityId::new(999),
                position: Vec3f::new(-400.0, 10.0, 0.0),
                prev_position: Vec3f::new(-400.0, 10.0, 0.0),
                velocity: Vec3f::new(400.0, 0.0, 0.0),
                explosion_radius: 30.0,
                ..shell(500 + shot)
            })
            .expect("the shell was accepted");

            // ~27 ticks to cross 350 m at `FIXED_DT * 400`, plus the tick the
            // queued hit lands on.
            for _ in 0..40 {
                let result = sim.tick();
                starts.extend(result.events.iter().filter_map(|e| match e {
                    GameEvent::FireStarted {
                        compartment,
                        intensity,
                    } => Some((*compartment, *intensity)),
                    _ => None,
                }));
            }
        }

        let fire = ship.get_state().compartment_states[&compartment].fire_intensity;
        assert!(
            fire > 0.0,
            "an explosive hit lit nothing: fire_intensity is {fire}"
        );
        assert_eq!(
            starts,
            vec![(compartment, FIRE_INTENSITY_ON_EXPLOSIVE_HIT)],
            "expected exactly one FireStarted for the lit compartment"
        );
    }

    /// The two constants that decide whether a shell can hurt a ship live in
    /// different files and were never compared against each other: the hull box
    /// a shell can hit (`ship/src/ship.rs`) and `DamageConfig::max_damage_radius`
    /// (`damage/src/damage.rs`). With the reach smaller than the hull, every
    /// shell struck the surface and did nothing at all, while every test that
    /// fed `apply_damage_to_ship` a point inside the ship stayed green.
    ///
    /// This asks the question directly: from any point of the hull a shell can
    /// strike, is some compartment within the reach? Move either constant on
    /// its own and this fails, which is the point — the two are one decision
    /// that happens to be written in two places.
    #[test]
    fn every_part_of_the_default_hull_can_be_damaged() {
        let ship = Ship::new(
            rfs_ship::loader::create_default_frigate().into(),
            EntityId::new(1),
            std::sync::Arc::new(rfs_damage::DamageSystem::new()),
        );
        let hull = ship.bounds();
        let lo = [hull.min.x, hull.min.y, hull.min.z];
        let hi = [hull.max.x, hull.max.y, hull.max.z];
        let reach = rfs_damage::damage::DamageConfig::default().max_damage_radius;
        let compartments = rfs_damage::damage::DamageTarget::compartment_hits(&ship);
        assert!(
            !compartments.is_empty(),
            "the default ship has no compartments, so this test would pass without checking anything"
        );

        let span = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let mut unreachable: Vec<(Vec3f, f32)> = Vec::new();
        for axis in 0..3 {
            for side in 0..2 {
                for i in 0..9 {
                    for j in 0..9 {
                        let mut p = [0.0f32; 3];
                        let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
                        p[b] = span(lo[b], hi[b], i as f32 / 8.0);
                        p[c] = span(lo[c], hi[c], j as f32 / 8.0);
                        p[axis] = if side == 0 { lo[axis] } else { hi[axis] };

                        let point = Vec3f::new(p[0], p[1], p[2]);
                        let nearest = compartments
                            .iter()
                            .map(|hit| distance_point_to_bounds(point, &hit.bounds))
                            .fold(f32::INFINITY, f32::min);
                        if nearest >= reach {
                            unreachable.push((point, nearest));
                        }
                    }
                }
            }
        }

        assert!(
            unreachable.is_empty(),
            "{} of 486 points on the hull lie outside the damage reach of {reach} m; \
             the furthest compartment from any struck point is {:.1} m away",
            unreachable.len(),
            unreachable
                .iter()
                .map(|(_, nearest)| *nearest)
                .fold(0.0f32, f32::max)
        );
    }

    /// A world a shell and a ship can both be reasoned about, small enough that
    /// the numbers below are legible.
    ///
    /// The rock sits within one tick of shell travel from the origin: a shell
    /// covers `FIXED_DT * velocity` per tick, so a rock placed further out would
    /// need the test to reproduce that arithmetic to place the shell correctly.
    fn test_world() -> crate::map::WorldMap {
        crate::map::WorldMap {
            name: "tick-test".into(),
            bounds: rfs_core::math::Bounds::new(
                Vec3f::new(-1000.0, -1000.0, -1000.0),
                Vec3f::new(1000.0, 1000.0, 1000.0),
            ),
            obstacles: vec![crate::map::MapObstacle {
                id: 100_001,
                bounds: rfs_core::math::Bounds::new(
                    Vec3f::new(5.0, -10.0, -10.0),
                    Vec3f::new(25.0, 10.0, 10.0),
                ),
                kind: crate::map::ObstacleKind::Structure,
            }],
        }
    }

    /// Starts one ship at a position and runs a single tick, returning where it
    /// ended up. The same ship, the same tick, with and without a world.
    fn ship_after_one_tick(world: Option<crate::map::WorldMap>, position: Vec3f, velocity: Vec3f) -> ShipState {
        let mut sim = TickSystem::new();
        if let Some(map) = world {
            sim.set_world(map).expect("the test world is not valid");
        }
        let ship = rfs_ship::ship::Ship::new(
            rfs_ship::ship::ShipConfig::default(),
            EntityId::new(1),
            std::sync::Arc::new(rfs_damage::DamageSystem::new()),
        );
        let mut state = ship.get_state();
        state.transform.position = position;
        state.velocity = velocity;
        ship.apply_state(state);
        sim.add_ship(ship);
        sim.tick();
        sim.get_ship(EntityId::new(1))
            .expect("ship registered")
            .get_state()
    }

    /// A ship placed outside the world has to be brought back in. The velocity
    /// half matters as much: a ship whose position is corrected while it keeps
    /// its outward speed sits against the face at full throttle, which reads as
    /// a collision bug rather than as a boundary.
    #[test]
    fn a_ship_outside_the_world_is_pulled_back_and_its_outward_speed_is_gone() {
        let after = ship_after_one_tick(
            Some(test_world()),
            Vec3f::new(5000.0, 0.0, 0.0),
            Vec3f::new(120.0, 0.0, 0.0),
        );
        assert!(
            after.transform.position.x < 1000.0,
            "the ship is still outside the world at {:?}",
            after.transform.position
        );
        assert!(
            after.velocity.x <= 0.0,
            "the ship still carries outward speed: {}",
            after.velocity.x
        );
    }

    /// Loading a map must not change anything for a ship already inside it.
    ///
    /// Written as a comparison against the same tick with no world rather than
    /// against a literal position: a ship moves under its own physics every
    /// tick, so "the position equals what I set" would be false whether or not
    /// the boundary touched it. The only difference between the two runs is the
    /// map, so any difference in the outcome is the map's doing.
    #[test]
    fn a_ship_inside_the_world_is_left_alone() {
        let start = Vec3f::new(10.0, 20.0, 30.0);
        let velocity = Vec3f::new(35.0, 0.0, 0.0);
        let with_world = ship_after_one_tick(Some(test_world()), start, velocity);
        let without_world = ship_after_one_tick(None, start, velocity);

        assert_eq!(
            with_world.transform.position, without_world.transform.position,
            "the world moved a ship that was inside it"
        );
        assert_eq!(
            with_world.velocity, without_world.velocity,
            "the world changed the speed of a ship that was inside it"
        );
    }

    /// A shell that crosses a rock stops on it, is reported as hitting it, and
    /// is spent. The rock is checked before the fleet so a shell cannot pass
    /// through stone to hit a ship behind it.
    #[test]
    fn a_shell_that_crosses_a_rock_stops_on_it_and_is_spent() {
        let mut sim = TickSystem::new();
        sim.set_world(test_world()).expect("the test world is not valid");
        sim.fire_projectile(Projectile {
            position: Vec3f::new(0.0, 0.0, 0.0),
            prev_position: Vec3f::new(-400.0, 0.0, 0.0),
            velocity: Vec3f::new(400.0, 0.0, 0.0),
            owner: EntityId::new(999),
            ..shell(500)
        })
        .expect("the shell was accepted");

        let result = sim.tick();

        assert!(
            sim.get_active_projectiles().is_empty(),
            "the shell survived the rock"
        );
        let hits = result
            .collisions
            .iter()
            .filter(|c| c.target == EntityId::new(100_001))
            .collect::<Vec<_>>();
        assert_eq!(hits.len(), 1, "the rock was hit {} times", hits.len());
        assert_eq!(
            hits[0].target_kind,
            CollisionTarget::Obstacle(crate::map::ObstacleKind::Structure)
        );
        assert!(
            hits[0].position.x > 0.0 && hits[0].position.x < 25.0,
            "the impact was reported at {:?}, which is not the rock",
            hits[0].position
        );
    }

    /// The negative control for the test above, in the same binary and the same
    /// shape: the identical shell, against a world with no rock on its line,
    /// must pass through and report nothing. Without it, "the rock was hit"
    /// could be satisfied by any collision at all.
    #[test]
    fn without_a_rock_on_its_line_the_same_shell_passes_through() {
        let mut sim = TickSystem::new();
        let mut world = test_world();
        world.obstacles.clear();
        sim.set_world(world).expect("the empty world is not valid");
        sim.fire_projectile(Projectile {
            position: Vec3f::new(0.0, 0.0, 0.0),
            prev_position: Vec3f::new(-400.0, 0.0, 0.0),
            velocity: Vec3f::new(400.0, 0.0, 0.0),
            owner: EntityId::new(999),
            ..shell(500)
        })
        .expect("the shell was accepted");

        let result = sim.tick();

        assert!(
            result.collisions.is_empty(),
            "an empty world still produced {:?} collisions",
            result.collisions
        );
    }

    /// A map that fails validation must never reach the tick. `set_world` is the
    /// only way in, so this is the test that the door is actually shut.
    ///
    /// The inverted box is built with a struct literal, not with `Bounds::new`:
    /// `new` sorts a reversed pair into order, so a "broken" map assembled
    /// through it is silently repaired and validation has nothing left to
    /// catch. Deserialisation goes the other way — straight into the fields —
    /// which is why an authored file can produce an invalid map and a
    /// hand-built one cannot.
    #[test]
    fn an_invalid_map_is_refused_at_the_door() {
        let sim = TickSystem::new();
        let mut broken = test_world();
        broken.obstacles[0].bounds = rfs_core::math::Bounds {
            min: Vec3f::new(5.0, 10.0, -10.0),
            max: Vec3f::new(25.0, -10.0, 10.0),
        };
        assert!(sim.set_world(broken).is_err(), "an inverted rock was accepted");
        assert!(
            sim.world().is_none(),
            "a refused map was installed anyway"
        );
    }

    /// Bug №608: a shell that has lost all its speed freezes — `integrate_step`
    /// and `simulate_underwater` both early-return below 0.1 m/s, and nothing in
    /// the loop then retires it. It therefore sits at one point for the rest of
    /// its 12 s life and is replicated to every client the whole time. A shell
    /// at rest is spent: `max_lifetime` is a ceiling, not a schedule.
    ///
    /// The waterline is how it is reached: fired at `y = 0` the shell drops below
    /// the surface in one tick of gravity, and water drag (~380 000 m/s² at
    /// 400 m/s) removes the rest of its speed in the next one. That part is
    /// honest physics and is left alone; what must not happen is the wreck then
    /// hanging in the world for another eleven seconds.
    #[test]
    fn a_shell_that_comes_to_rest_is_spent_rather_than_left_frozen() {
        let mut sim = TickSystem::new();
        sim.fire_projectile(Projectile {
            position: Vec3f::new(0.0, 0.0, 0.0),
            prev_position: Vec3f::new(0.0, 0.0, 0.0),
            velocity: Vec3f::new(400.0, 0.0, 0.0),
            owner: EntityId::new(999),
            ..shell(500)
        })
        .expect("the shell was accepted");

        // Long enough for the shell to stop and then to outlive a whole second
        // of frozen replication.
        for _ in 0..30 {
            sim.tick();
        }
        assert!(
            sim.get_active_projectiles()
                .iter()
                .all(|p| p.entity_id != EntityId::new(500)),
            "a shell at rest is still in the world: {:?}",
            sim.get_active_projectiles()
                .iter()
                .map(|p| (p.entity_id, p.position, p.velocity))
                .collect::<Vec<_>>()
        );
    }

    /// The tick a shell is brought to rest is not the tick it is retired.
    ///
    /// The harness oracle `underwater_shell_stops_dead_no_teleport` asserts a
    /// shell that hits water at speed is *still present* right after, stopped
    /// exactly where it was: the impact position is the thing worth observing,
    /// and a shell that vanished on arrival could not be told from one that
    /// teleported away. So the rest check runs on entry — the arriving tick keeps
    /// the shell, the following tick (entered already at rest, nothing left to
    /// add) drops it. This pins that boundary from my side, so a future "retire
    /// as soon as the step returns zero" cannot silently re-break the oracle.
    #[test]
    fn a_shell_is_spent_the_tick_after_it_stops_not_the_tick_it_stops() {
        let mut sim = TickSystem::new();
        sim.fire_projectile(Projectile {
            position: Vec3f::new(0.0, -5.0, 0.0),
            prev_position: Vec3f::new(0.0, -5.0, 0.0),
            velocity: Vec3f::new(100.0, 0.0, 0.0),
            owner: EntityId::new(999),
            ..shell(501)
        })
        .expect("the shell was accepted");

        sim.tick();
        let live = sim.get_active_projectiles();
        assert_eq!(live.len(), 1, "the shell must survive the tick it stops on");
        assert!(
            live[0].velocity.length() < 1.0,
            "water drag must stop it, speed={}",
            live[0].velocity.length()
        );
        assert!(
            (live[0].position.x - 0.0).abs() < 10.0,
            "no teleport, x={}",
            live[0].position.x
        );

        sim.tick();
        assert!(
            sim.get_active_projectiles().is_empty(),
            "a shell entered at rest must be spent on the following tick"
        );
    }
}