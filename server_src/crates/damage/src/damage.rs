use crate::graph::{CompartmentNode, DamageGraph};
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
        let hits = target.compartment_hits();
        let mut inflicted = Vec::with_capacity(hits.len());

        for hit in &hits {
            let distance = hit.bounds.center().distance(position);
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
                    }
                }
                graph.apply_damage(*entity_id, *amount);
            }
        }

        target.apply_health_damage(damage * self.config.hull_damage_multiplier);
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

impl Default for DamageSystem {
    fn default() -> Self {
        Self::new()
    }
}