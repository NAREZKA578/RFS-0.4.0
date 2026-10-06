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

#[derive(Debug, Clone)]
pub struct PendingDamage {
    pub target: EntityId,
    pub projectile: EntityId,
    pub damage: f32,
    pub penetration: f32,
    pub position: Vec3f,
    pub normal: Vec3f,
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

#[derive(Debug, Clone)]
pub struct CollisionEvent {
    pub tick: Tick,
    pub projectile: EntityId,
    pub target: EntityId,
    pub position: Vec3f,
    pub normal: Vec3f,
    pub velocity: Vec3f,
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
        self.update_projectiles(&mut result);
        self.check_collisions(&mut result);
        self.collect_damage();
        
        result
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
        let prev_flooded: HashMap<EntityId, bool> = prev
            .compartment_states
            .iter()
            .map(|(id, c)| (*id, c.is_flooded()))
            .collect();
        for (cid, c) in &next.compartment_states {
            let was_flooded = prev_flooded.get(cid).copied().unwrap_or(false);
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
        // Snapshot inputs first, publish after: no nested cross-locking,
        // and the history holds exactly one tick (no unbounded growth).
        let projectiles = self.projectiles.read().clone();
        let ships: Vec<Arc<Ship>> = self.ships.read().values().cloned().collect();

        let mut collisions = Vec::new();

        for proj in projectiles.iter() {
            for ship in ships.iter() {
                if ship.entity_id() == proj.owner {
                    continue;
                }

                let bounds = ship.bounds();
                if segment_intersects_bounds(proj.prev_position, proj.position, &bounds) {
                    let normal = Self::calculate_impact_normal(ship, proj.position);
                    collisions.push(CollisionEvent {
                        tick: self.current_tick,
                        projectile: proj.entity_id,
                        target: ship.entity_id(),
                        position: proj.position,
                        normal,
                        velocity: proj.velocity,
                    });
                    // One hit per projectile per tick is enough; the shell is
                    // despawned in collect_damage below.
                    break;
                }
            }
        }

        result.collisions = collisions.clone();
        *self.collisions.write() = collisions;
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
        let collisions: Vec<CollisionEvent> = self.collisions.read().clone();
        if collisions.is_empty() {
            return;
        }

        let projectiles = self.projectiles.read().clone();

        let mut pending: Vec<PendingDamage> = Vec::new();
        for collision in collisions.iter() {
            if let Some(proj) = projectiles
                .iter()
                .find(|p| p.entity_id == collision.projectile)
            {
                pending.push(PendingDamage {
                    target: collision.target,
                    projectile: collision.projectile,
                    damage: proj.damage,
                    penetration: proj.penetration,
                    position: collision.position,
                    normal: collision.normal,
                });
            }
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
                        });
                    }
                }
            }
        }
        drop(projectiles);

        if pending.is_empty() {
            return;
        }

        // A shell that hit is spent: remove it so it cannot damage again next
        // tick. (Explosive area damage is handled by the caller via
        // explosion_radius in a follow-up pass.)
        let spent: Vec<EntityId> = pending.iter().map(|h| h.projectile).collect();
        self.projectiles
            .write()
            .retain(|p| !spent.contains(&p.entity_id));

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
                ship.apply_damage(hit.damage, hit.position);

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
}