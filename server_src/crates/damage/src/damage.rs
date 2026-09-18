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

    pub fn apply_damage_to_ship(&self, target: &impl DamageTarget, position: Vec3f, damage: f32) {
        // Bug №65: NaN/negative "damage" must never reach the hull or the
        // compartments — a negative blast used to heal the ship.
        if !(damage > 0.0) {
            return;
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
                            // Synthetic, deterministic id. Real bulkheads live in
                            // the ship_id+3000 region; this hash sits elsewhere.
                            let edge_id = EntityId::new(
                                hit.entity_id.0.wrapping_mul(0x9E37_79B9)
                                    ^ other.entity_id.0.wrapping_add(0x517C_C1B7),
                            );
                            if graph.get_bulkhead(edge_id).is_none() {
                                graph.add_bulkhead(BulkheadEdge::new(
                                    edge_id,
                                    hit.entity_id,
                                    other.entity_id,
                                    100.0,
                                ));
                            }
                        }
                    }
                }
                graph.apply_damage(*entity_id, *amount);
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
        if distance <= self.config.falloff_distance {
            1.0
        } else if distance >= self.config.max_damage_radius {
            0.0
        } else {
            let range = self.config.max_damage_radius - self.config.falloff_distance;
            let t = 1.0 - (distance - self.config.falloff_distance) / range.max(0.01);
            t.powf(self.config.falloff_power).clamp(0.0, 1.0)
        }
    }
}

/// Distance from a point to the nearest point of an axis-aligned box
/// (Bug №65). If the point is inside the box the distance is 0.
fn distance_to_bounds(point: Vec3f, bounds: &Bounds) -> f32 {
    let clamped = Vec3f::new(
        point.x.clamp(bounds.min.x, bounds.max.x),
        point.y.clamp(bounds.min.y, bounds.max.y),
        point.z.clamp(bounds.min.z, bounds.max.z),
    );
    clamped.distance(point)
}

impl Default for DamageSystem {
    fn default() -> Self {
        Self::new()
    }
}