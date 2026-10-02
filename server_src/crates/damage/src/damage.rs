use crate::graph::{BulkheadEdge, CompartmentNode, DamageGraph};
use rfs_core::entity::EntityId;
use rfs_core::math::{Bounds, Vec3f};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamageConfig {
    pub hull_damage_multiplier: f32,
    pub direct_damage_multiplier: f32,
    pub falloff_distance: f32,
    pub falloff_power: f32,
    pub max_damage_radius: f32,
}

impl Default for DamageConfig {
    fn default() -> Self {
        Self {
            hull_damage_multiplier: 1.0,
            direct_damage_multiplier: 1.0,
            falloff_distance: 3.0,
            falloff_power: 1.5,
            max_damage_radius: 12.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentHit {
    pub entity_id: EntityId,
    pub name: String,
    pub bounds: Bounds,
    pub max_water_level: f32,
    pub pump_capacity: f32,
}

pub trait DamageTarget {
    fn entity_id(&self) -> EntityId;
    fn compartment_hits(&self) -> Vec<CompartmentHit>;
    fn apply_health_damage(&self, amount: f32);
    fn apply_compartment_damage(&self, compartment_id: EntityId, damage: f32, position: Vec3f);
}

pub struct DamageSystem {
    config: DamageConfig,
    graph: Mutex<DamageGraph>,
}

impl DamageSystem {
    pub fn new() -> Self {
        Self {
            config: DamageConfig::default(),
            graph: Mutex::new(DamageGraph::new()),
        }
    }

    pub fn with_config(config: DamageConfig) -> Self {
        Self {
            config,
            graph: Mutex::new(DamageGraph::new()),
        }
    }

    pub fn config(&self) -> &DamageConfig {
        &self.config
    }

    /// Apply damage and report which compartments breached for the first time
    /// (Bug №151: the caller mirrors the breach into the ship's own state so
    /// battle damage actually floods it).
    pub fn apply_damage_to_ship(&self, target: &impl DamageTarget, position: Vec3f, damage: f32) -> Vec<EntityId> {
        // Bug №65: NaN/negative "damage" must never reach the hull or the
        // compartments — a negative blast used to heal the ship.
        // Spelled out rather than `!(damage > 0.0)`: the NaN case has to stay
        // rejected, and `<=` alone would let NaN through.
        if damage.is_nan() || damage <= 0.0 {
            return Vec::new();
        }

        let hits = target.compartment_hits();
        let mut inflicted = Vec::with_capacity(hits.len());

        // Bug №65: the falloff distance is measured to the NEAREST point of
        // each compartment box, not its center — an edge hit of a large box
        // used to under-damage that box and over-damage its neighbours.
        for hit in &hits {
            let distance = distance_to_bounds(position, &hit.bounds);
            let factor = self.falloff(distance);
            let damage_dealt = damage * factor * self.config.direct_damage_multiplier;

            if damage_dealt <= 0.0 {
                continue;
            }

            target.apply_compartment_damage(hit.entity_id, damage_dealt, position);
            inflicted.push((hit.entity_id, damage_dealt));
        }

        let mut newly_breached: Vec<EntityId> = Vec::new();
        {
            let mut graph = self.graph.lock();
            for (entity_id, amount) in &inflicted {
                if graph.get_compartment(*entity_id).is_none() {
                    if let Some(hit) = hits.iter().find(|h| h.entity_id == *entity_id) {
                        graph.add_compartment(CompartmentNode::new(
                            hit.entity_id,
                            hit.name.clone(),
                            hit.max_water_level,
                            hit.pump_capacity,
                        ));

                        // Bug №65: a node created here used to arrive with ZERO
                        // edges, so water flooding it could never flow anywhere.
                        // Best-effort wire it to every ship box it touches so the
                        // replay/spoofed path still propagates.
                        for other in &hits {
                            if other.entity_id == hit.entity_id {
                                continue;
                            }
                            let touching = hit.bounds.expand(2.0).intersects(&other.bounds);
                            if !touching {
                                continue;
                            }
                            if graph
                                .get_bulkhead_between(hit.entity_id, other.entity_id)
                                .is_some()
                            {
                                continue;
                            }
                            // Synthetic deterministic id outside ship regions.
                            // Guard: hash may collide with a real compartment or
                            // bulkhead id — never reuse such an id.
                            let edge_id = EntityId::new(
                                hit.entity_id.0.wrapping_mul(0x9E37_79B9)
                                    ^ other.entity_id.0.wrapping_add(0x517C_C1B7),
                            );
                            if graph.get_bulkhead(edge_id).is_none()
                                && graph.get_compartment(edge_id).is_none()
                            {
                                // Bug №241: this used to be `BulkheadEdge::new`,
                                // which sets `is_sealed: true`, and
                                // `is_passable()` is `!is_sealed || is_destroyed`
                                // — so every synthetic edge was permanently
                                // impassable and the water never flowed, which
                                // is the exact opposite of what the comment
                                // above this block claims. A synthetic edge
                                // exists to provide a route, so it is built
                                // open.
                                graph.add_bulkhead(BulkheadEdge::open(
                                    edge_id,
                                    hit.entity_id,
                                    other.entity_id,
                                    100.0,
                                ));
                            }
                        }
                    }
                }
                let was_breached = graph
                    .get_compartment(*entity_id)
                    .map(|c| c.is_breached)
                    .unwrap_or(false);
                graph.apply_damage(*entity_id, *amount);
                // Bug №151: only the edge TRUE is reported — an already-sunk
                // compartment must not re-fire the breach.
                if !was_breached
                    && graph
                        .get_compartment(*entity_id)
                        .map(|c| c.is_breached)
                        .unwrap_or(false)
                {
                    newly_breached.push(*entity_id);
                }
            }
        }

        // Bug №65: the hull took the FULL blast regardless of falloff — a
        // far-edge hit dealt 0 to every compartment and 100 to the hull.
        // Hull damage now scales with the best factor seen by any compartment.
        let mut max_factor: f32 = 0.0;
        for hit in &hits {
            let factor = self.falloff(distance_to_bounds(position, &hit.bounds));
            max_factor = max_factor.max(factor);
        }
        let hull_factor = if hits.is_empty() { 1.0 } else { max_factor };
        target.apply_health_damage(damage * self.config.hull_damage_multiplier * hull_factor);
        newly_breached
    }

    /// Register the ship's compartment/bulkhead topology so the production
    /// graph has real edges (Bug №65's lazy nodes) and water can flow.
    pub fn register_topology(
        &self,
        nodes: &[(EntityId, String, f32, f32)],
        edges: &[(EntityId, EntityId, EntityId, f32)],
    ) {
        let mut graph = self.graph.lock();
        for (id, name, capacity, pump) in nodes {
            if let Some(node) = graph.get_compartment_mut(*id) {
                // Bug №64: registering a duplicate used to add an orphan node.
                node.max_capacity = *capacity;
                node.pump_capacity = *pump;
            } else {
                graph.add_compartment(CompartmentNode::new(*id, name.clone(), *capacity, *pump));
            }
        }
        for (id, a, b, seal_strength) in edges {
            if graph.get_bulkhead(*id).is_none() {
                graph.add_bulkhead(BulkheadEdge::new(*id, *a, *b, *seal_strength));
            }
        }
    }

    fn falloff(&self, distance: f32) -> f32 {
        // Validated falloff: non-finite/inverted configs must not invert damage
        // (negative power = farther hurts more) or NaN-poison.
        if !distance.is_finite() || distance < 0.0 {
            return 1.0;
        }
        let falloff = if self.config.falloff_distance.is_finite() && self.config.falloff_distance >= 0.0 {
            self.config.falloff_distance
        } else {
            0.0
        };
        let max_r = if self.config.max_damage_radius.is_finite() && self.config.max_damage_radius > falloff {
            self.config.max_damage_radius
        } else {
            falloff + 1.0
        };
        if distance <= falloff {
            1.0
        } else if distance >= max_r {
            0.0
        } else {
            let range = (max_r - falloff).max(0.01);
            let t = (1.0 - (distance - falloff) / range).clamp(0.0, 1.0);
            let power = if self.config.falloff_power.is_finite() && self.config.falloff_power >= 0.0 {
                self.config.falloff_power
            } else {
                1.0
            };
            t.powf(power).clamp(0.0, 1.0)
        }
    }
}

/// Distance from a point to the nearest point of an axis-aligned box
/// (Bug №65). If the point is inside the box the distance is 0.
fn distance_to_bounds(point: Vec3f, bounds: &Bounds) -> f32 {
    // Safe clamp: inverted Bounds (min>max) from bad templates must not panic.
    fn safe_clamp(v: f32, lo: f32, hi: f32) -> f32 {
        if !v.is_finite() {
            return 0.0;
        }
        if !lo.is_finite() || !hi.is_finite() || lo > hi {
            return v;
        }
        v.clamp(lo, hi)
    }
    let clamped = Vec3f::new(
        safe_clamp(point.x, bounds.min.x, bounds.max.x),
        safe_clamp(point.y, bounds.min.y, bounds.max.y),
        safe_clamp(point.z, bounds.min.z, bounds.max.z),
    );
    clamped.distance(point)
}

impl Default for DamageSystem {
    fn default() -> Self {
        Self::new()
    }
}