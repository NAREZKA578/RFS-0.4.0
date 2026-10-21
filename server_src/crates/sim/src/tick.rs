use rayon::prelude::*;
use rfs_core::entity::EntityId;
use rfs_core::math::{Transform, Vec3f};
use rfs_core::packet::ProjectileType;
use rfs_core::time::{Tick, FIXED_DT};
use rfs_ship::ship::{Ship, ShipState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::ballistics::{BallisticsCalculator, BallisticsConfig, RICOCHET_ELASTICITY};

/// How much of the kinetic energy of a ram becomes hull damage.
///
/// The design note asks for real damage, and for it to be asymmetric by mass
/// and speed. Both follow from the energy, so the only free number is the
/// conversion. It is sized so that a ramming run at combat speed hurts the
/// ship that is hit badly (tens of percent of its health) and the ship that
/// rams barely at all (a few percent) — the asymmetry the note describes —
/// and so that the cap, not the arithmetic, is what stops a full-speed hit
/// from ending the fight in one pass.
pub const RAM_DAMAGE_PER_JOULE: f32 = 2.0e-5;

/// The share of a ship's health one ram may take.
///
/// A ram is a blunt instrument, and a fight that ends in a single contact is
/// not a fight. Without a cap, a destroyer at full speed would erase a
/// frigate's health in one pass, and the asymmetry the note asks for would
/// read as "the heavy ship always wins", not as "the heavy ship hurts more".
pub const RAM_MAX_HEALTH_SHARE: f32 = 0.25;

/// Damage of a ram from the kinetic energy it carried, capped so that one
/// contact cannot take more than `RAM_MAX_HEALTH_SHARE` of the hull's health.
fn ram_damage(energy: f32, max_health: f32) -> f32 {
    if !energy.is_finite() || energy <= 0.0 {
        return 0.0;
    }
    let raw = energy * RAM_DAMAGE_PER_JOULE;
    let cap = max_health.max(0.0) * RAM_MAX_HEALTH_SHARE;
    raw.min(cap)
}

/// A contact between two axis-aligned boxes.
struct AabbContact {
    /// Unit normal pointing from the first box to the second.
    normal: Vec3f,
    /// The point where the two boxes meet.
    point: Vec3f,
    /// How deeply the boxes overlap.
    penetration: f32,
}

/// The contact between two boxes, or `None` when they do not overlap.
///
/// The normal is the axis of the SHALLOWEST overlap: pushing the hulls apart
/// along that axis separates them in one step, while any other axis would
/// leave them still overlapping.
fn aabb_contact(a: &rfs_core::math::Bounds, b: &rfs_core::math::Bounds) -> Option<AabbContact> {
    let a_min = [a.min.x, a.min.y, a.min.z];
    let a_max = [a.max.x, a.max.y, a.max.z];
    let b_min = [b.min.x, b.min.y, b.min.z];
    let b_max = [b.max.x, b.max.y, b.max.z];

    let mut normal = [0.0f32; 3];
    let mut penetration = f32::INFINITY;
    let mut point = [0.0f32; 3];
    for axis in 0..3 {
        let overlap = (a_max[axis].min(b_max[axis])) - (a_min[axis].max(b_min[axis]));
        if overlap <= 0.0 {
            return None;
        }
        if overlap < penetration {
            penetration = overlap;
            // The normal names ONE axis, so the previous candidate has to be
            // cleared: leaving it behind would push the hull diagonally out of
            // a box it was never inside on that axis.
            normal = [0.0; 3];
            // The direction that separates A from B on this axis: the normal
            // points FROM A TO B, so it is +1 when A lies on the lower side.
            normal[axis] = if a_min[axis] + a_max[axis] <= b_min[axis] + b_max[axis] {
                1.0
            } else {
                -1.0
            };
        }
        // The contact point on this axis: the middle of the overlap.
        point[axis] = (a_min[axis].max(b_min[axis]) + a_max[axis].min(b_max[axis])) * 0.5;
    }
    if !penetration.is_finite() {
        return None;
    }
    Some(AabbContact {
        normal: Vec3f::new(normal[0], normal[1], normal[2]),
        point: Vec3f::new(point[0], point[1], point[2]),
        penetration,
    })
}

/// How deeply a box overlaps another, and the direction to push it out.
///
/// The result is the direction and distance that separates the FIRST box from
/// the SECOND: `normal * penetration` is the offset that takes the first box
/// out of the second.
fn aabb_penetration(
    a: &rfs_core::math::Bounds,
    b: &rfs_core::math::Bounds,
) -> Option<(Vec3f, f32)> {
    let contact = aabb_contact(a, b)?;
    // `aabb_contact` points from A to B, so pushing A out means going the
    // other way.
    Some((-contact.normal, contact.penetration))
}

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

/// Bug №664 (remainder): how far a bouncing shell is pushed back out along the
/// struck face. Enough that the next tick's flight starts outside the hull (a
/// sweep that begins on the face reads as a second collision), short enough
/// that the reflection reads as "came off the armour", not "was teleported".
pub const RICOCHET_EJECT_OFFSET: f32 = 1.0;

/// Bug №664 (remainder): one hull impact the model priced as a ricochet, with
/// the shell alive to reflect. Priced in `collect_damage` (where the model
/// runs), executed in `apply_pending_damage` (where the projectile list can be
/// mutated). The bounce reuses the same shell, so the client sees one tracer
/// coming off the plate, not a shell disappearing and another spawning.
#[derive(Debug, Clone)]
pub struct RicochetBounce {
    pub position: Vec3f,
    pub velocity: Vec3f,
    pub previous_count: u32,
}

#[derive(Debug, Clone)]
pub struct PendingDamage {
    pub target: EntityId,
    /// What caused the damage. A shell names the shell; a ram names the ship
    /// that hit. The two are the same damage system (the same compartment
    /// graph does the work) but they are different events to a client, and a
    /// single `projectile` field could not say which.
    pub source: DamageSource,
    pub damage: f32,
    pub penetration: f32,
    pub position: Vec3f,
    pub normal: Vec3f,
    /// Bug №607: whether the shell that produced this entry was explosive, and
    /// so should set the struck compartment on fire when the damage lands.
    pub incendiary: bool,
    /// Bug №664 (remainder): when the hit was priced as a ricochet and the
    /// shell has bounce budget left, this carries the follow-up flight —
    /// position, velocity, and how many bounces preceded. `None` means the
    /// shell is spent where it struck.
    pub ricochet_bounce: Option<RicochetBounce>,
}

/// What produced a piece of damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    /// A shell: the id names the projectile.
    Projectile(EntityId),
    /// A ramming ship: the id names the ship that hit.
    Ram(EntityId),
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
    /// Bug №664 (remainder): how many hull bounces this shell already made.
    /// `BallisticsConfig::max_ricochets` is the ceiling; the count lives on the
    /// shell rather than in a side table so the bounce rule cannot disagree
    /// with the visible flight.
    pub ricochet_count: u32,
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

    /// Bug №664 (remainder): a TickSystem built with a chosen ballistics
    /// config. Tests use it to pin flag behaviour (`max_ricochets`); a server
    /// backend could use the same door later to load the config from data.
    pub fn with_ballistics(config: BallisticsConfig) -> Self {
        Self {
            ballistics: BallisticsCalculator::new(config),
            ..Self::new()
        }
    }

    /// Read access to the calculator the tick loop flies shells with. The
    /// fire-control solution (`server/src/tick.rs::ballistic_aim`) shares this
    /// one instance, so the arc it aims along and the integrator the shell
    /// actually flies cannot read different rules.
    pub fn ballistics(&self) -> &BallisticsCalculator {
        &self.ballistics
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
        // Bug №308/the design note: in an open sea the only thing a ship can
        // run into is another ship (or a structure on an authored map). The
        // contact pass runs in the same phase as the ballistics — the note
        // asks for it explicitly ("the same general tick phase, not a separate
        // step") — and before the projectile pass, so a shell fired this tick
        // already sees the hulls that just moved.
        self.resolve_ship_contacts(&mut result);
        self.update_projectiles(&mut result);
        self.check_collisions(&mut result);
        self.collect_damage(&mut result);

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
    /// Bug №308: the clamp looks at the ship's CENTRE, and that is the
    /// documented model: a hull may cross the face of the world, because
    /// telling "leaning against the world" from "half out of it" needs a
    /// contact model this pass does not have, and inventing one is the
    /// owner's call. A hull that is entirely outside is not a separate case —
    /// its centre is outside too, and the centre is what gets brought back.
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
            let outside = [
                state.transform.position.x - position.x,
                state.transform.position.y - position.y,
                state.transform.position.z - position.z,
            ];
            let v = [state.velocity.x, state.velocity.y, state.velocity.z];
            let mut damped = v;
            for axis in 0..3 {
                if (outside[axis] > 0.0 && v[axis] > 0.0) || (outside[axis] < 0.0 && v[axis] < 0.0)
                {
                    damped[axis] = 0.0;
                }
            }
            state.transform.position = position;
            state.velocity = Vec3f::new(damped[0], damped[1], damped[2]);
            ship.apply_state(state);
        }
    }

    /// Bug №308 / the design note's ramming: hull against hull, and hull
    /// against structure.
    ///
    /// The design note is explicit about both halves:
    ///
    /// * "Open ocean without terrain means a hull collision is the only kind of
    ///   'ran into something' in the game" — and that it should be REAL damage,
    ///   not a push: "a breach from a ram is the same Torricelli formula and
    ///   the same compartment graph, just a different source of damage".
    /// * The damage is asymmetric by mass and speed: "a heavy fast destroyer
    ///   rams a light frigate devastatingly and barely notices the hit itself".
    ///
    /// So the contact is resolved as an impulse along the contact normal, and
    /// the damage each side takes is the kinetic energy its own change of
    /// velocity carries. The asymmetry is not a rule written on top of the
    /// physics — it falls out of it: the light ship takes a large share of
    /// the impulse and therefore a large share of the energy, and the heavy
    /// one barely changes speed.
    fn resolve_ship_contacts(&self, result: &mut TickResult) {
        let ships: Vec<Arc<Ship>> = self.ships.read().values().cloned().collect();
        if ships.len() < 2 {
            // A single ship still has to respect the world, so the structure
            // pass runs even with one hull.
            self.resolve_obstacle_contacts(&ships, result);
            return;
        }

        for i in 0..ships.len() {
            for j in (i + 1)..ships.len() {
                let a = &ships[i];
                let b = &ships[j];
                let Some(contact) = aabb_contact(&a.bounds(), &b.bounds()) else {
                    continue;
                };
                let normal = contact.normal;
                let position = contact.point;

                // The normal points from A to B, so the closing speed is how
                // much faster A moves towards B along it. Below zero means the
                // hulls are already separating — nothing to do.
                let relative = (a.velocity() - b.velocity()).dot(normal);
                if relative <= 0.0 {
                    continue;
                }
                let mass_a = a.config().mass.max(1.0);
                let mass_b = b.config().mass.max(1.0);
                // Owner decision (2026-10-21): a ram is a LOSS OF WAY, not a
                // bounce and not a grip. Each hull loses the NORMAL part of
                // its own velocity, and how much of that loss is its own
                // depends on the mass of the other hull: the share that
                // reaches the other side is
                //
                //   k_a = m_b / (m_a + m_b),  k_b = m_a / (m_a + m_b)
                //
                // so a heavy hull barely notices and a light one is stopped
                // dead — which is the asymmetry the design note asks for, and
                // it is the same asymmetry the damage below is priced from.
                //
                // The loss is the normal component, so it depends on the
                // ANGLE: a head-on ram takes the way away almost completely, a
                // glancing blow takes almost none. Nothing is transferred
                // back (that would be a bounce), and nothing sticks (that
                // would be a grip) — the energy of the normal component simply
                // goes into the hulls.
                let total_mass = mass_a + mass_b;
                let k_a = mass_b / total_mass;
                let k_b = mass_a / total_mass;
                let into_a = a.velocity().dot(normal);
                let into_b = b.velocity().dot(normal);
                let dv_a = into_a * k_a;
                let dv_b = into_b * k_b;

                // Separate the hulls so they do not stay inside each other:
                // the overlap is split along the contact normal.
                let separation = contact.penetration * 0.5;
                {
                    let mut state_a = a.get_state();
                    state_a.transform.position -= normal * separation;
                    state_a.velocity -= normal * dv_a;
                    a.apply_state(state_a);
                }
                {
                    let mut state_b = b.get_state();
                    state_b.transform.position += normal * separation;
                    state_b.velocity -= normal * dv_b;
                    b.apply_state(state_b);
                }

                // Damage: the kinetic energy each hull lost with its normal
                // velocity. The heavier hull loses far less of it, so the
                // strike lands where the design note says it should.
                let damage_a = ram_damage(0.5 * mass_a * dv_a * dv_a, a.config().max_health);
                let damage_b = ram_damage(0.5 * mass_b * dv_b * dv_b, b.config().max_health);
                if damage_b > 0.0 {
                    self.pending_damage.write().push(PendingDamage {
                        target: b.entity_id(),
                        source: DamageSource::Ram(a.entity_id()),
                        damage: damage_b,
                        penetration: 0.0,
                        position,
                        normal,
                        // A hull is not a shell: nothing catches fire.
                        incendiary: false,
                        ricochet_bounce: None,
                    });
                }
                if damage_a > 0.0 {
                    self.pending_damage.write().push(PendingDamage {
                        target: a.entity_id(),
                        source: DamageSource::Ram(b.entity_id()),
                        damage: damage_a,
                        penetration: 0.0,
                        position,
                        // The normal seen from A's side points the other way.
                        normal: -normal,
                        incendiary: false,
                        ricochet_bounce: None,
                    });
                }
            }
        }

        self.resolve_obstacle_contacts(&ships, result);
    }

    /// Hulls against the world's solid volumes.
    ///
    /// Bug №308: a shell stopped by a rock while the ship drove through the
    /// same rock — the world was solid for projectiles and permeable for
    /// ships. A ship that overlaps a structure is pushed out along the
    /// shallowest axis and loses the velocity component that was carrying it
    /// in. No damage: a structure has no health model, and inventing one is a
    /// protocol decision, not a physics fix.
    fn resolve_obstacle_contacts(&self, ships: &[Arc<Ship>], _result: &mut TickResult) {
        let world = self.world.read();
        let Some(map) = world.as_ref() else {
            return;
        };
        if map.obstacles.is_empty() {
            return;
        }

        for ship in ships {
            for obstacle in &map.obstacles {
                let Some((normal, penetration)) =
                    aabb_penetration(&ship.bounds(), &obstacle.bounds)
                else {
                    continue;
                };
                let mut state = ship.get_state();
                state.transform.position += normal * penetration;

                // The velocity that was carrying the hull INTO the structure is
                // removed. "Into" is measured on every axis the hull overlaps
                // on, not only on the axis it was pushed out along: a hull
                // that lies lengthwise inside a rock and slides along it must
                // not keep driving deeper sideways.
                let hull = ship.bounds();
                let hull_min = [hull.min.x, hull.min.y, hull.min.z];
                let hull_max = [hull.max.x, hull.max.y, hull.max.z];
                let rock_min = [
                    obstacle.bounds.min.x,
                    obstacle.bounds.min.y,
                    obstacle.bounds.min.z,
                ];
                let rock_max = [
                    obstacle.bounds.max.x,
                    obstacle.bounds.max.y,
                    obstacle.bounds.max.z,
                ];
                let position = [
                    state.transform.position.x,
                    state.transform.position.y,
                    state.transform.position.z,
                ];
                let mut velocity = [state.velocity.x, state.velocity.y, state.velocity.z];
                for axis in 0..3 {
                    let overlap =
                        (hull_max[axis].min(rock_max[axis])) - (hull_min[axis].max(rock_min[axis]));
                    if overlap <= 0.0 {
                        continue;
                    }
                    let centre = (rock_min[axis] + rock_max[axis]) * 0.5;
                    let towards_centre = centre - position[axis];
                    if towards_centre * velocity[axis] > 0.0 {
                        velocity[axis] = 0.0;
                    }
                }
                state.velocity = Vec3f::new(velocity[0], velocity[1], velocity[2]);
                ship.apply_state(state);
            }
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
        let updates: Vec<_> = ships
            .par_iter()
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
                    //
                    // Bug №664 (remainder): a diving shell did not see the
                    // waterline. The air branch dropped it a whole tick length
                    // (~13 m at 400 m/s) past the surface, and only the next
                    // tick switched it to the water integrator. `check_water_
                    // entry` names the crossing; when the step would cross the
                    // surface this tick, fly air until it and water after it —
                    // both segments inside this one tick.
                    let step = if proj.position.y < 0.0 {
                        ballistics.simulate_underwater(proj.position, proj.velocity, config, dt)
                    } else if let Some(entry) =
                        ballistics.check_water_entry(proj.position, proj.velocity, 0.0)
                    {
                        let remaining = (dt - entry.time_to_entry * dt).max(0.0);
                        ballistics.simulate_underwater(
                            entry.entry_position,
                            entry.entry_velocity,
                            config,
                            remaining,
                        )
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
            let obstacle_hit = map
                .as_ref()
                .and_then(|m| m.swept_obstacle(proj.prev_position, proj.position));
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
                    let impact = segment_entry_point(proj.prev_position, proj.position, &bounds)
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

    fn collect_damage(&self, result: &mut TickResult) {
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
        // Bug №664: the model needs the struck ship's declared armor, so a
        // third read guard joins the queue (still only reads, released below
        // before the projectile write).
        let ships = self.ships.read();

        let mut pending: Vec<PendingDamage> = Vec::new();
        let mut obstacle_hits = 0u64;
        for collision in collisions.iter() {
            let Some(proj) = projectiles
                .iter()
                .find(|p| p.entity_id == collision.projectile)
            else {
                continue;
            };
            match collision.target_kind {
                CollisionTarget::Ship => {
                    // Bug №664: the declared shell numbers used to land raw;
                    // calculate_penetration / calculate_damage existed but only
                    // the tests called them. The model runs here: the struck
                    // ship's declared plate, the incidence angle from the
                    // actual impact geometry, and the flight distance the
                    // projectile already accumulated. A shell with no
                    // registered config, or a target whose ship object is gone,
                    // keeps the raw legacy number.
                    let armor = ships
                        .get(&collision.target)
                        .map(|s| s.config().armor_thickness);
                    let (damage, ricochet_bounce) =
                        match (self.ballistics.get_config(proj.projectile_type), armor) {
                            (Some(config), Some(armor)) => {
                                // The struck hull face is the armor plate, so
                                // the incidence angle between the shell path
                                // and the surface is the plate angle the
                                // thickness roll divides by.
                                let impact_angle = collision
                                    .velocity
                                    .normalize()
                                    .dot(collision.normal)
                                    .abs()
                                    .clamp(0.0, 1.0)
                                    .acos();
                                let penetration = self.ballistics.calculate_penetration(
                                    config,
                                    collision.velocity,
                                    collision.normal,
                                    armor,
                                    impact_angle,
                                );
                                // Bug №664 (remainder): a shell priced as a
                                // ricochet is spent as damage but lives on,
                                // reflected. Two gates beyond the model itself:
                                // blast shells explode on contact and never
                                // bounce, and the config caps the count per
                                // shell (`max_ricochets`).
                                let bounce = if penetration.ricochet
                                    && proj.explosion_radius.is_finite()
                                    && proj.explosion_radius <= 0.0
                                    && proj.ricochet_count < self.ballistics.config().max_ricochets
                                {
                                    Some(RicochetBounce {
                                        position: collision.position
                                            + collision.normal * RICOCHET_EJECT_OFFSET,
                                        velocity: self.ballistics.simulate_ricochet(
                                            collision.velocity,
                                            collision.normal,
                                            RICOCHET_ELASTICITY,
                                        ),
                                        previous_count: proj.ricochet_count,
                                    })
                                } else {
                                    None
                                };
                                (
                                    self.ballistics.calculate_damage(
                                        config,
                                        &penetration,
                                        proj.distance_traveled,
                                    ),
                                    bounce,
                                )
                            }
                            _ => (proj.damage, None),
                        };
                    pending.push(PendingDamage {
                        target: collision.target,
                        source: DamageSource::Projectile(collision.projectile),
                        damage,
                        penetration: proj.penetration,
                        position: collision.position,
                        normal: collision.normal,
                        // Bug №607: only an explosive shell starts a fire, and
                        // only on the ship it actually struck.
                        incendiary: proj.explosion_radius.is_finite()
                            && proj.explosion_radius > 0.0,
                        ricochet_bounce,
                    });
                }
                CollisionTarget::Obstacle(_) => {
                    // Not queued as damage. `apply_pending_damage` looks the
                    // target up among ships and would silently drop it, so
                    // queueing it here would cost a lookup to achieve nothing
                    // - and the silence is the problem, not the cost. Nothing
                    // tracks structure health (the world is static by the
                    // owner's decision), so a rock cannot be hurt; what *is*
                    // real is the shell being spent on it, and that has to be
                    // countable rather than invisible. Same lesson as Bug
                    // #196, where a missing counter made dropped shots
                    // impossible to see.
                    obstacle_hits += 1;
                    // Bug №307: and countable is not enough — the client has to
                    // see that the shot ended HERE, or the shell simply
                    // disappears at the rock from the player's side.
                    result.events.push(GameEvent::ProjectileImpact {
                        projectile: collision.projectile,
                        position: collision.position,
                        normal: collision.normal,
                    });
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
                            source: DamageSource::Projectile(collision.projectile),
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
                            // Bug №664 (remainder): the blast's direct hit owns
                            // the shell's ricochet route, if any — a splash
                            // entry describes damage to a neighbour, not a
                            // second flight for the shell.
                            ricochet_bounce: None,
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
        //
        // Bug №664 (remainder): the exception is a shell the model priced as
        // a ricochet with bounce budget left. It is spent as damage but keeps
        // flying, reflected — removed from the world here, it would read as a
        // shell vanishing into its own bounce.
        let bouncing: Vec<EntityId> = pending
            .iter()
            .filter_map(|hit| {
                hit.ricochet_bounce.as_ref()?;
                match hit.source {
                    DamageSource::Projectile(id) => Some(id),
                    // A ram has no shell to keep flying.
                    DamageSource::Ram(_) => None,
                }
            })
            .collect();
        let spent: Vec<EntityId> = collisions
            .iter()
            .filter(|c| !bouncing.contains(&c.projectile))
            .map(|c| c.projectile)
            .collect();
        // Bug №611: the read guards are dropped before the projectile list is
        // mutated, so the write never happens with a read guard on the same
        // lock. `spent` is what the old snapshot was cloned for.
        drop(ships);
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

        let mut bounces: Vec<(EntityId, RicochetBounce)> = Vec::new();
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

                result.events.push(match hit.source {
                    DamageSource::Projectile(projectile) => GameEvent::ShipHit {
                        target: hit.target,
                        projectile,
                        position: hit.position,
                        normal: hit.normal,
                        damage: hit.damage,
                        penetration: hit.penetration,
                        hit_compartment,
                    },
                    // A ram is the same damage through the same compartment
                    // graph, but a different source: the client has to be able
                    // to tell a hull from a shell.
                    DamageSource::Ram(rammer) => GameEvent::ShipRam {
                        rammer,
                        target: hit.target,
                        position: hit.position,
                        normal: hit.normal,
                        damage: hit.damage,
                    },
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

            // Bug №664 (remainder): the reflection this impact priced. The
            // bounce is collected here — where the ship write guard is held —
            // and applied below, after the guard is down: the projectile list
            // may not be written while `ships` is held (Bug №611's rule).
            if let Some(bounce) = hit.ricochet_bounce {
                if let DamageSource::Projectile(id) = hit.source {
                    bounces.push((id, bounce));
                }
            }
        }
        drop(ships);

        // The same shell, alive and reflected: restart the sweep from the
        // eject point with the reflected velocity. `prev_position` follows so
        // the next tick's hit test sweeps out of the hull, not from inside it,
        // and the bounce count is bumped against the config's ceiling.
        if !bounces.is_empty() {
            let mut projectiles = self.projectiles.write();
            for (entity_id, bounce) in bounces {
                if let Some(proj) = projectiles.iter_mut().find(|p| p.entity_id == entity_id) {
                    proj.position = bounce.position;
                    proj.prev_position = bounce.position;
                    proj.velocity = bounce.velocity;
                    proj.ricochet_count = bounce.previous_count + 1;
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
        self.collisions
            .read()
            .iter()
            .filter(|c| c.tick > since_tick)
            .cloned()
            .collect()
    }
}

/// Swept point-vs-AABB test over one tick segment (plan §4: the shell must
/// not tunnel through a thin target between two ticks). Slab method with
/// explicit parallel-axis handling, so no NaN on zero-length segments.
pub fn segment_intersects_bounds(
    prev: Vec3f,
    curr: Vec3f,
    bounds: &rfs_core::math::Bounds,
) -> bool {
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
    /// Two hulls met and the contact did damage.
    ///
    /// The design note asks for the ram to be a different SOURCE of damage,
    /// not a new damage system: the same compartment graph does the work, and
    /// this event exists so a client can tell a ram from a shell.
    ShipRam {
        rammer: EntityId,
        target: EntityId,
        position: Vec3f,
        normal: Vec3f,
        damage: f32,
    },
    /// A shell ended its flight on something.
    ///
    /// Bug №307: a shell that struck a structure used to leave no trace at
    /// all — the shot was spent, the damage was a no-op (a structure has no
    /// health model, and the world is static by the owner's decision), and the
    /// client never heard about it. From the player's side the shell simply
    /// disappeared at the rock. This event is that trace: the shot was spent
    /// HERE.
    ProjectileImpact {
        projectile: EntityId,
        position: Vec3f,
        normal: Vec3f,
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
            ricochet_count: 0,
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
        assert_eq!(sim.dropped_projectiles(), 1, "the refusal was not counted");
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
            assert_eq!(
                sim.fire_projectile(shell(i as u64 + 1)),
                Ok(()),
                "refused early at {i}"
            );
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
            Projectile {
                position: Vec3f::new(f32::NAN, 0.0, 0.0),
                ..shell(1)
            },
            Projectile {
                position: Vec3f::new(f32::INFINITY, 0.0, 0.0),
                ..shell(2)
            },
            Projectile {
                velocity: Vec3f::new(f32::NAN, 0.0, 0.0),
                ..shell(3)
            },
            Projectile {
                velocity: Vec3f::new(0.0, f32::NEG_INFINITY, 0.0),
                ..shell(4)
            },
            Projectile {
                lifetime: f32::NAN,
                ..shell(5)
            },
        ];
        for (i, bad) in cases.into_iter().enumerate() {
            let sim = TickSystem::new();
            assert_eq!(
                sim.fire_projectile(bad),
                Err(ProjectileRejected::NonFiniteBallistics),
                "case {i} was accepted"
            );
            assert!(
                sim.get_active_projectiles().is_empty(),
                "case {i} was stored"
            );
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
    fn ship_after_one_tick(
        world: Option<crate::map::WorldMap>,
        position: Vec3f,
        velocity: Vec3f,
    ) -> ShipState {
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
    ///
    /// Bug №308: the world is now solid for ships too, so the ship has to start
    /// clear of the test world's structure — a hull inside a rock is pushed out
    /// (that is the point of the new contact pass), and this test is about the
    /// BOUNDARY, not about the rock.
    /// The world holds a ship by its CENTRE, and a hull may cross the face.
    ///
    /// Bug №308: the record noticed that the boundary kept the ship by its
    /// centre only, so a hull 300 m long could sit half out of the world and
    /// still count as inside. The model is now a decision, not an accident:
    /// the world holds the CENTRE, the hull may cross the face. Telling
    /// "leaning against the world" from "half out of it" needs a contact model
    /// (the same one the hull-to-hull and hull-to-structure contacts still do
    /// not have), and choosing it is the owner's call — so this test pins the
    /// decision instead of hiding it.
    #[test]
    fn the_world_holds_the_ship_by_its_centre_and_the_hull_may_cross_the_face() {
        // The test world spans ±1000 and a hull is 300 m long, so a centre at
        // 990 already puts the stern outside.
        let after = ship_after_one_tick(
            Some(test_world()),
            Vec3f::new(990.0, 0.0, 0.0),
            Vec3f::new(0.0, 0.0, 0.0),
        );
        assert!(
            after.transform.position.x > 900.0,
            "the centre must not be dragged in: {:?}",
            after.transform.position
        );
    }

    #[test]
    fn a_ship_inside_the_world_is_left_alone() {
        let start = Vec3f::new(10.0, 100.0, 30.0);
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
        sim.set_world(test_world())
            .expect("the test world is not valid");
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

    /// Bug №307: a shell that strikes a structure must leave a trace the
    /// client can see.
    ///
    /// The world is static by the owner's decision, so the structure cannot be
    /// hurt and no damage is dealt — but the shot IS spent, and from the
    /// player's side the shell used to simply disappear at the rock. The
    /// impact event is that trace.
    #[test]
    fn a_shell_that_strikes_a_structure_reports_the_impact() {
        let mut sim = TickSystem::new();
        sim.set_world(test_world())
            .expect("the test world is not valid");
        sim.fire_projectile(Projectile {
            position: Vec3f::new(0.0, 0.0, 0.0),
            prev_position: Vec3f::new(-400.0, 0.0, 0.0),
            velocity: Vec3f::new(400.0, 0.0, 0.0),
            owner: EntityId::new(999),
            ..shell(500)
        })
        .expect("the shell was accepted");

        let result = sim.tick();

        let impacts: Vec<&GameEvent> = result
            .events
            .iter()
            .filter(|e| matches!(e, GameEvent::ProjectileImpact { .. }))
            .collect();
        assert_eq!(
            impacts.len(),
            1,
            "the impact must be reported once; events were {:?}",
            result.events
        );
        match impacts[0] {
            GameEvent::ProjectileImpact {
                projectile,
                position,
                ..
            } => {
                assert_eq!(*projectile, EntityId::new(500), "the wrong shell");
                assert!(
                    position.x > 0.0 && position.x < 25.0,
                    "the impact was reported at {position:?}, which is not the rock"
                );
            }
            other => panic!("unexpected event {other:?}"),
        }
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
        assert!(
            sim.set_world(broken).is_err(),
            "an inverted rock was accepted"
        );
        assert!(sim.world().is_none(), "a refused map was installed anyway");
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

    // Bug №664: a live ship for the model tests. The loader's frigate class is
    // the same production data the server spawns, so the plate these tests face
    // (armor 50.0) is the plate players face.
    fn test_frigate(id: u64) -> Ship {
        use rfs_damage::damage::DamageSystem;
        use rfs_ship::loader::{create_default_frigate, ShipLoader};
        let mut loader = ShipLoader::new("tests");
        loader.register_class(create_default_frigate());
        loader
            .create_ship(1, EntityId::new(id), Arc::new(DamageSystem::new()))
            .expect("the frigate class builds a ship")
    }

    /// Bug №664: the live damage path models the hit instead of applying the
    /// declared number.
    ///
    /// A chain-shot round (declared damage 50, penetration 10) driven squarely
    /// into the frigate's plate cannot punch through (10 « armor 50), so the
    /// model's stopped branch halves the damage: the hull loses ~25 health,
    /// not the raw 50. Impact angle 0 also means no ricochet, so the number is
    /// independent of the penetration jitter.
    #[test]
    fn a_square_hit_stops_on_the_plate_and_halves_the_damage() {
        let mut sim = TickSystem::new();
        sim.add_ship(test_frigate(10));
        let hull = sim
            .get_ship(EntityId::new(10))
            .expect("the frigate sits in the sim");
        let bounds = hull.bounds();
        let center = bounds.center();
        // Spawn outside the +X face, straight at it: the swept segment crosses
        // the face head-on, so the collision normal is the face normal and the
        // incidence angle is zero.
        let spawn = Vec3f::new(bounds.max.x + 4.0, center.y, center.z);
        sim.fire_projectile(Projectile {
            position: spawn,
            prev_position: spawn,
            velocity: Vec3f::new(-200.0, 0.0, 0.0),
            projectile_type: ProjectileType::ChainShot,
            damage: 50.0,
            penetration: 10.0,
            ..shell(2)
        })
        .expect("the chain shot was accepted");

        // Tick 1 records the collision and prices the damage; tick 2 applies
        // it at the boundary (plan §5.1). The applied event carries the exact
        // modelled number, so the assertion is on the model, not on how the
        // damage graph later splits it across compartments.
        sim.tick();
        let applied = sim.tick();

        let hits: Vec<f32> = applied
            .events
            .iter()
            .filter_map(|e| match e {
                GameEvent::ShipHit { target, damage, .. } if *target == EntityId::new(10) => {
                    Some(*damage)
                }
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "one collision, one hit priced: {hits:?}");
        assert!(
            (hits[0] - 25.0).abs() < 0.5,
            "a stopped plate must halve the damage, got {} (the raw path would land 50)",
            hits[0]
        );
        assert!(
            10000.0 - hull.health() > 0.5,
            "the modelled damage never reached the hull"
        );
    }

    /// Bug №664: the ricochet branch fires off shallow impacts.
    ///
    /// The same chain shot skimming nearly parallel to the plate (|v·n| ≈ 0.32,
    /// i.e. ~72° off the normal) is past the ricochet threshold (0.707 rad),
    /// so the model keeps a quarter: ~12.5, not the raw 50 — and not the ~25
    /// the square hit dealt.
    #[test]
    fn a_shallow_hit_ricochets_for_a_quarter() {
        let mut sim = TickSystem::new();
        sim.add_ship(test_frigate(10));
        let hull = sim
            .get_ship(EntityId::new(10))
            .expect("the frigate sits in the sim");
        let bounds = hull.bounds();
        let center = bounds.center();
        let half_x = (bounds.max.x - bounds.min.x) * 0.5;
        // Skim the +X face: mostly parallel drift (z), slight inward creep
        // (x). The contact point stays near the face middle in z, so the
        // dominant-axis normal is the face normal.
        let spawn = Vec3f::new(bounds.max.x + 0.8, center.y, center.z - 0.25 * half_x);
        sim.fire_projectile(Projectile {
            position: spawn,
            prev_position: spawn,
            velocity: Vec3f::new(-30.0, 0.0, 90.0),
            projectile_type: ProjectileType::ChainShot,
            damage: 50.0,
            penetration: 10.0,
            ..shell(2)
        })
        .expect("the chain shot was accepted");

        sim.tick();
        let applied = sim.tick();

        let hits: Vec<f32> = applied
            .events
            .iter()
            .filter_map(|e| match e {
                GameEvent::ShipHit { target, damage, .. } if *target == EntityId::new(10) => {
                    Some(*damage)
                }
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "one collision, one hit priced: {hits:?}");
        assert!(
            (hits[0] - 12.5).abs() < 0.5,
            "a grazing hit must ricochet for a quarter, got {} (raw would be 50)",
            hits[0]
        );
        assert!(
            10000.0 - hull.health() > 0.5,
            "the modelled damage never reached the hull"
        );
    }

    /// Bug №664 (remainder): a ricochet is a shell that keeps going.
    ///
    /// The same grazing chain shot as above, but on a rise (+3 m/s in y) so
    /// the impact lands on the freeboard — OVER the waterline. This matters:
    /// gravity drops the flat shot's impact point to y ≈ −0.009, just under
    /// the surface, and the water integrator then honestly drowns the bounce
    /// (water drag stops the reflected shell in the same tick). The mechanic
    /// is tested where it is visible: the hit is still priced once (~12.5),
    /// and the shell stays in the world, reflected off the plate and with its
    /// bounce count bumped. The reflected velocity points OUT of the hull
    /// face (the inbound creep was in -X, so the bounce flips that). The old
    /// model treated a graze as hit-and-vanish, where nothing would remain in
    /// flight.
    #[test]
    fn a_ricochet_reflects_the_shell_rather_than_vanishing() {
        let mut sim = TickSystem::new();
        sim.add_ship(test_frigate(10));
        let hull = sim
            .get_ship(EntityId::new(10))
            .expect("the frigate sits in the sim");
        let bounds = hull.bounds();
        let center = bounds.center();
        let half_x = (bounds.max.x - bounds.min.x) * 0.5;
        let spawn = Vec3f::new(bounds.max.x + 0.8, center.y, center.z - 0.25 * half_x);
        sim.fire_projectile(Projectile {
            position: spawn,
            prev_position: spawn,
            velocity: Vec3f::new(-30.0, 3.0, 90.0),
            projectile_type: ProjectileType::ChainShot,
            damage: 50.0,
            penetration: 10.0,
            ..shell(2)
        })
        .expect("the chain shot was accepted");

        sim.tick();
        let applied = sim.tick();

        let hits: Vec<f32> = applied
            .events
            .iter()
            .filter_map(|e| match e {
                GameEvent::ShipHit { target, damage, .. } if *target == EntityId::new(10) => {
                    Some(*damage)
                }
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "one collision, one hit priced: {hits:?}");
        assert!(
            (hits[0] - 12.5).abs() < 0.5,
            "the grazing pricing is unchanged by the bounce: {hits:?}"
        );

        let live = sim.get_active_projectiles();
        assert_eq!(
            live.len(),
            1,
            "a ricochet keeps the shell in flight, it does not vanish"
        );
        assert_eq!(
            live[0].ricochet_count, 1,
            "the bounce must be counted on the shell itself"
        );
        assert!(
            live[0].velocity.x > 0.0,
            "the bounce must push the shell back OFF the +X face, got vx {} (inbound was -30)",
            live[0].velocity.x
        );
        assert!(
            live[0].position.x > bounds.max.x,
            "the shell must rest outside the plate it was ejected from, x {}",
            live[0].position.x
        );

        // And it flies on without re-colliding with the hull it just left.
        sim.tick();
        assert_eq!(
            sim.get_active_projectiles().len(),
            1,
            "a bounced shell must not immediately collide again"
        );
    }

    /// Bug №664 (remainder): the config caps the bounces, not just the math.
    ///
    /// The same graze with `max_ricochets: 0`: no bounce budget means no
    /// reflection — the shell is spent where it struck, which is the original
    /// hit-and-vanish behaviour. A cap that was written but never read would
    /// not make this difference.
    #[test]
    fn the_ricochet_budget_caps_the_bounces() {
        let mut sim = TickSystem::with_ballistics(BallisticsConfig {
            max_ricochets: 0,
            ..BallisticsConfig::default()
        });
        sim.add_ship(test_frigate(10));
        let hull = sim
            .get_ship(EntityId::new(10))
            .expect("the frigate sits in the sim");
        let bounds = hull.bounds();
        let center = bounds.center();
        let half_x = (bounds.max.x - bounds.min.x) * 0.5;
        let spawn = Vec3f::new(bounds.max.x + 0.8, center.y, center.z - 0.25 * half_x);
        sim.fire_projectile(Projectile {
            position: spawn,
            prev_position: spawn,
            velocity: Vec3f::new(-30.0, 0.0, 90.0),
            projectile_type: ProjectileType::ChainShot,
            damage: 50.0,
            penetration: 10.0,
            ..shell(2)
        })
        .expect("the chain shot was accepted");

        sim.tick();
        let applied = sim.tick();

        let hits: Vec<f32> = applied
            .events
            .iter()
            .filter_map(|e| match e {
                GameEvent::ShipHit { target, damage, .. } if *target == EntityId::new(10) => {
                    Some(*damage)
                }
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "the hit is still priced: {hits:?}");
        assert!(
            10000.0 - hull.health() > 0.5,
            "the modelled damage never reached the hull"
        );
        assert!(
            sim.get_active_projectiles().is_empty(),
            "a zero bounce budget must spend the shell at the impact point"
        );
    }

    /// Bug №664 (remainder): a diving shell stops ON the waterline, not past
    /// it.
    ///
    /// A 400 m/s dive from 5 m up crosses the surface 12.5 ms into the tick;
    /// the old single-step air branch carried the shell the FULL tick (~13 m)
    /// past the surface and only switched it to the water integrator on the
    /// next tick. `check_water_entry` names the crossing, so the tick flies
    /// air until it and water after it — the shell ends THIS tick at the
    /// surface, and the water drag (2.37·v²) has already stopped it dead, so
    /// the following tick retires it.
    #[test]
    fn a_diving_shell_stops_on_the_waterline_rather_than_tunnelling() {
        let mut sim = TickSystem::new();
        sim.fire_projectile(Projectile {
            position: Vec3f::new(0.0, 5.0, 0.0),
            prev_position: Vec3f::new(0.0, 5.0, 0.0),
            velocity: Vec3f::new(0.0, -400.0, 0.0),
            ..shell(2)
        })
        .expect("the diving shell was accepted");

        sim.tick();
        let live = sim.get_active_projectiles();
        assert_eq!(live.len(), 1, "the shell survived the entry tick");
        assert!(
            live[0].position.y.abs() < 0.5,
            "the shell must end the entry tick AT the waterline, got y {} (the old air step put it ~13 m under)",
            live[0].position.y
        );

        // Dead in the water: the next tick retires it, it does not keep
        // diving into the sea.
        sim.tick();
        assert!(
            sim.get_active_projectiles().is_empty(),
            "a shell stopped by water drag must be retired, not left sinking"
        );
    }
}

/// Bug №308 and the design note's ramming: what a hull contact does.
///
/// The note is specific: "a heavy fast destroyer rams a light frigate
/// devastatingly and barely notices the hit itself", and the damage has to be
/// the same compartment graph a shell uses — a different SOURCE, not a new
/// damage system.
#[cfg(test)]
mod ram_tests {
    use super::*;
    use rfs_core::math::Bounds;
    use rfs_damage::damage::DamageSystem;
    use rfs_ship::ship::{Ship, ShipConfig};

    const HEAVY: f32 = 2_000_000.0;
    const LIGHT: f32 = 200_000.0;

    fn ship(id: u64, mass: f32, position: Vec3f, velocity: Vec3f) -> Ship {
        let config = ShipConfig {
            mass,
            ..ShipConfig::default()
        };
        let ship = Ship::new(config, EntityId::new(id), Arc::new(DamageSystem::new()));
        let mut state = ship.get_state();
        state.transform.position = position;
        state.velocity = velocity;
        ship.apply_state(state);
        ship
    }

    /// Two hulls on a closing course, one tick of contact, and the tick that
    /// lands the damage.
    ///
    /// The hulls start nose to nose: a ship's box is 50×30×150 around its
    /// centre, so at ±40 along X they already overlap and the contact
    /// happens in the first tick.
    fn ram(heavy_mass: f32, light_mass: f32, speed: f32) -> TickSystem {
        let mut sim = TickSystem::new();
        let heavy = ship(
            1,
            heavy_mass,
            Vec3f::new(-40.0, 0.0, 0.0),
            Vec3f::new(speed, 0.0, 0.0),
        );
        let light = ship(
            2,
            light_mass,
            Vec3f::new(40.0, 0.0, 0.0),
            Vec3f::new(-speed, 0.0, 0.0),
        );
        sim.add_ship(heavy);
        sim.add_ship(light);
        sim.tick();
        sim.tick();
        sim
    }

    #[test]
    fn a_ram_damages_the_light_ship_far_more_than_the_heavy_one() {
        let sim = ram(HEAVY, LIGHT, 12.0);
        let heavy_health = sim.get_ship(EntityId::new(1)).unwrap().get_state().health;
        let light_health = sim.get_ship(EntityId::new(2)).unwrap().get_state().health;

        let heavy_loss = ShipConfig::default().max_health - heavy_health;
        let light_loss = ShipConfig::default().max_health - light_health;
        assert!(
            light_loss > heavy_loss * 5.0,
            "the heavy ship must barely notice the hit: it lost {heavy_loss}, the light one {light_loss}"
        );
        assert!(
            light_loss > 0.0,
            "a ram at 12 m/s must actually hurt the ship it hits"
        );
    }

    #[test]
    fn a_ram_is_reported_as_a_ram_and_not_as_a_shell_hit() {
        let mut sim = TickSystem::new();
        let heavy = ship(
            1,
            HEAVY,
            Vec3f::new(-40.0, 0.0, 0.0),
            Vec3f::new(12.0, 0.0, 0.0),
        );
        let light = ship(
            2,
            LIGHT,
            Vec3f::new(40.0, 0.0, 0.0),
            Vec3f::new(-12.0, 0.0, 0.0),
        );
        sim.add_ship(heavy);
        sim.add_ship(light);
        let first = sim.tick();
        let second = sim.tick();

        let rams: Vec<&GameEvent> = second
            .events
            .iter()
            .filter(|e| matches!(e, GameEvent::ShipRam { .. }))
            .collect();
        assert!(
            !rams.is_empty(),
            "the ram must be reported; events were {:?}",
            second.events
        );
        assert!(
            first
                .events
                .iter()
                .all(|e| !matches!(e, GameEvent::ShipRam { .. })),
            "the damage lands on the next tick, not in the contact tick"
        );
    }

    #[test]
    fn the_ramming_ships_do_not_pass_through_each_other() {
        let mut sim = TickSystem::new();
        let heavy = ship(
            1,
            HEAVY,
            Vec3f::new(-40.0, 0.0, 0.0),
            Vec3f::new(40.0, 0.0, 0.0),
        );
        let light = ship(
            2,
            LIGHT,
            Vec3f::new(40.0, 0.0, 0.0),
            Vec3f::new(-40.0, 0.0, 0.0),
        );
        sim.add_ship(heavy);
        sim.add_ship(light);
        for _ in 0..5 {
            sim.tick();
        }
        let a = sim.get_ship(EntityId::new(1)).unwrap().get_state();
        let b = sim.get_ship(EntityId::new(2)).unwrap().get_state();
        let gap = b.transform.position.x - a.transform.position.x;
        assert!(
            gap > 0.0,
            "the hulls must not end up on the wrong sides of each other, gap={gap}"
        );
    }

    /// Owner decision (2026-10-21): the loss of speed depends on the angle of
    /// the contact — a head-on ram takes the way away, a glancing blow barely
    /// touches it. No bounce, no grip.
    #[test]
    fn a_glancing_ram_costs_little_way_and_a_head_on_ram_costs_a_lot() {
        let glancing = ram_speed_loss(75.0);
        let head_on = ram_speed_loss(0.0);
        assert!(
            head_on > glancing * 3.0,
            "a head-on ram must cost far more way than a glancing one: {head_on} vs {glancing}"
        );
        assert!(
            glancing < 1.0,
            "a glancing blow must barely take the way away, it took {glancing}"
        );
    }

    /// How much of its own speed the struck hull loses in a ram, for a
    /// contact at `angle_degrees` off the head-on direction.
    ///
    /// The two hulls start ALREADY overlapping, so the contact happens in the
    /// first tick whatever their directions are: a glancing blow is a hull
    /// that slides along another, not one that misses it.
    fn ram_speed_loss(angle_degrees: f32) -> f32 {
        let mut sim = TickSystem::new();
        let speed = 12.0;
        let angle = angle_degrees.to_radians();
        let a = ship(
            1,
            LIGHT,
            Vec3f::new(-40.0, 0.0, 0.0),
            Vec3f::new(speed, 0.0, 0.0),
        );
        let b = ship(
            2,
            LIGHT,
            Vec3f::new(40.0, 0.0, 0.0),
            Vec3f::new(-speed * angle.cos(), -speed * angle.sin(), 0.0),
        );
        sim.add_ship(a);
        sim.add_ship(b);
        sim.tick();
        let after = sim.get_ship(EntityId::new(2)).unwrap().get_state();
        let before = speed;
        (before - after.velocity.length()).abs()
    }

    #[test]
    fn a_ship_inside_a_structure_is_pushed_out_of_it() {
        // Bug №308: the world was solid for shells and permeable for ships.
        //
        // The contact model here is "leaning": a hull that overlaps a solid
        // volume is pushed out along the shallowest axis and loses the speed
        // that was carrying it in. A full "stopped outside" model — the hull
        // never entering the volume at all — is a different decision about
        // contact, and the record leaves it open; what is NOT open is a hull
        // that drives deeper into the rock every tick.
        let mut sim = TickSystem::new();
        let structure = Bounds::new(
            Vec3f::new(-100.0, -100.0, -100.0),
            Vec3f::new(100.0, 100.0, 100.0),
        );
        sim.set_world(crate::map::WorldMap {
            name: "structure-test".into(),
            bounds: Bounds::new(
                Vec3f::new(-5000.0, -5000.0, -5000.0),
                Vec3f::new(5000.0, 5000.0, 5000.0),
            ),
            obstacles: vec![crate::map::MapObstacle {
                id: 1,
                bounds: structure,
                kind: crate::map::ObstacleKind::Structure,
            }],
        })
        .expect("the test world is valid");

        let hull = ship(
            1,
            LIGHT,
            Vec3f::new(0.0, 0.0, 0.0),
            Vec3f::new(-30.0, 0.0, 0.0),
        );
        sim.add_ship(hull);
        sim.tick();
        let after = sim.get_ship(EntityId::new(1)).unwrap().get_state();

        // The hull was pushed out of the structure: it starts centred in it,
        // and afterwards it lies on the surface.
        assert!(
            after.transform.position.length() > 1.0,
            "the hull was not pushed out of the structure at all: {:?}",
            after.transform.position
        );

        // And it must not sink deeper with every tick: the overlap with the
        // structure may not grow.
        let mut previous = structure_overlap(&after.transform.position, &structure);
        for _ in 0..3 {
            sim.tick();
            let state = sim.get_ship(EntityId::new(1)).unwrap().get_state();
            let depth = structure_overlap(&state.transform.position, &structure);
            assert!(
                depth <= previous + 1e-3,
                "the hull sank deeper into the structure: {previous} -> {depth}"
            );
            previous = depth;
        }
    }

    /// How deeply a hull overlaps a structure, summed over the axes.
    fn structure_overlap(position: &Vec3f, structure: &Bounds) -> f32 {
        let half = Vec3f::new(50.0, 30.0, 150.0);
        let hull_min = [
            position.x - half.x,
            position.y - half.y,
            position.z - half.z,
        ];
        let hull_max = [
            position.x + half.x,
            position.y + half.y,
            position.z + half.z,
        ];
        let rock_min = [structure.min.x, structure.min.y, structure.min.z];
        let rock_max = [structure.max.x, structure.max.y, structure.max.z];
        let mut total = 0.0;
        for axis in 0..3 {
            let overlap =
                (hull_max[axis].min(rock_max[axis])) - (hull_min[axis].max(rock_min[axis]));
            if overlap > 0.0 {
                total += overlap;
            }
        }
        total
    }
}
