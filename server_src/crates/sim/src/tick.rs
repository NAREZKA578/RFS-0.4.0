use rfs_core::entity::EntityId;
use rfs_core::math::{Vec3f, Transform};
use rfs_core::packet::ProjectileType;
use rfs_core::time::{Tick, FIXED_DT};
use rfs_ship::ship::Ship;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

impl TickSystem {
    pub fn new() -> Self {
        Self {
            ships: RwLock::new(HashMap::new()),
            current_tick: Tick(0),
            projectiles: RwLock::new(Vec::new()),
            collisions: RwLock::new(Vec::new()),
            pending_damage: RwLock::new(Vec::new()),
            ballistics: BallisticsCalculator::new(BallisticsConfig::default()),
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
        
        let updates: Vec<_> = ships.par_iter()
            .map(|(id, ship)| {
                let mut ship_state = ship.get_state();
                ship.update(FIXED_DT, &mut ship_state);
                (*id, ship_state)
            })
            .collect();

        drop(ships);

        let ships = self.ships.write();
        for (id, state) in updates {
            if let Some(ship) = ships.get(&id) {
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
                    let step = if proj.position.y <= 0.0 {
                        // Below the waterline: buoyancy + water drag.
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

                result.events.push(GameEvent::ShipHit {
                    target: hit.target,
                    projectile: hit.projectile,
                    position: hit.position,
                    normal: hit.normal,
                    damage: hit.damage,
                    penetration: hit.penetration,
                    hit_compartment: None,
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

    pub fn fire_projectile(&self, projectile: Projectile) {
        self.projectiles.write().push(projectile);
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