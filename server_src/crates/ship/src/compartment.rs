use rfs_core::entity::{CompartmentTemplate, EntityId, Bounds, Vec3f};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentConfig {
    pub name: String,
    pub local_bounds: Bounds,
    pub max_water_level: f32,
    pub pump_capacity: f32,
    pub connected_compartments: Vec<EntityId>,
    pub bulkheads: Vec<BulkheadState>,
    pub stations: Vec<EntityId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkheadState {
    pub entity_id: EntityId,
    pub connects_to: EntityId,
    pub local_position: Vec3f,
    pub is_sealed: bool,
    pub is_destroyed: bool,
    pub seal_strength: f32,
    pub health: f32,
    pub max_health: f32,
}

impl Default for BulkheadState {
    fn default() -> Self {
        Self {
            entity_id: EntityId::nil(),
            connects_to: EntityId::nil(),
            local_position: Vec3f::ZERO,
            is_sealed: true,
            is_destroyed: false,
            seal_strength: 1000.0,
            health: 1000.0,
            max_health: 1000.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentState {
    pub water_level: f32,
    pub max_water_level: f32,
    pub is_sealed: bool,
    pub is_breached: bool,
    pub fire_intensity: f32,
    pub pump_active: bool,
    pub pump_capacity: f32,
    pub bulkhead_states: Vec<BulkheadState>,
    pub connected_compartments: Vec<EntityId>,
}

impl CompartmentState {
    pub fn is_flooded(&self) -> bool {
        self.water_level > self.max_water_level * 0.9
    }

    pub fn is_critical(&self) -> bool {
        self.water_level > self.max_water_level * 0.5
    }

    pub fn fill_ratio(&self) -> f32 {
        if self.max_water_level > 0.0 {
            self.water_level / self.max_water_level
        } else {
            0.0
        }
    }
}

/// A compartment is a config holder. All mutable state lives in
/// `ShipState::compartment_states` (single source of truth, bug №53):
/// the tick copy is what flooding/fire/pumps mutate, what damage mutates,
/// and what gets replicated.
pub struct Compartment {
    config: CompartmentConfig,
    entity_id: EntityId,
    ship_id: EntityId,
}

impl Compartment {
    pub fn new(
        entity_id: EntityId,
        ship_id: EntityId,
        template: CompartmentTemplate,
        connected: Vec<EntityId>,
        bulkheads: Vec<BulkheadState>,
    ) -> Self {
        Self {
            config: CompartmentConfig {
                name: template.name,
                local_bounds: template.local_bounds,
                max_water_level: template.max_water_level,
                pump_capacity: template.pump_capacity,
                connected_compartments: connected,
                bulkheads,
                stations: template.stations.iter().map(|_| EntityId::new(0)).collect(),
            },
            entity_id,
            ship_id,
        }
    }

    pub fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    pub fn ship_id(&self) -> EntityId {
        self.ship_id
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    pub fn config(&self) -> &CompartmentConfig {
        &self.config
    }

    pub fn set_connected_compartments(&mut self, compartments: Vec<EntityId>) {
        self.config.connected_compartments = compartments;
    }

    pub fn set_bulkhead_connections(&mut self, connections: HashMap<EntityId, EntityId>) {
        for bulkhead in self.config.bulkheads.iter_mut() {
            if let Some(&target) = connections.get(&bulkhead.entity_id) {
                bulkhead.connects_to = target;
            }
        }
    }

    pub fn update(&self, dt: f32, state: &mut CompartmentState, all_compartments: &HashMap<EntityId, CompartmentState>) {
        if state.is_breached && !state.is_sealed {
            let intake_rate = 10.0;
            state.water_level = (state.water_level + intake_rate * dt).min(state.max_water_level);
        }

        if state.pump_active && state.water_level > 0.0 {
            let pump_out = state.pump_capacity * dt;
            state.water_level = (state.water_level - pump_out).max(0.0);
        }

        if state.fire_intensity > 0.0 {
            state.fire_intensity = (state.fire_intensity - dt * 0.1).max(0.0);
        }

        self.spread_water(state, all_compartments, dt);
        self.spread_fire(state, all_compartments, dt);
    }

    fn spread_water(&self, state: &mut CompartmentState, all_compartments: &HashMap<EntityId, CompartmentState>, dt: f32) {
        for &connected_id in &state.connected_compartments {
            if let Some(other) = all_compartments.get(&connected_id) {
                let bulkhead = state.bulkhead_states.iter()
                    .find(|b| b.connects_to == connected_id);
                
                if let Some(bh) = bulkhead {
                    if !bh.is_sealed || bh.is_destroyed {
                        let pressure_diff = state.fill_ratio() - other.fill_ratio();
                        if pressure_diff > 0.0 {
                            let flow_rate = pressure_diff * 50.0 * dt;
                            let transfer = flow_rate.min(state.water_level);
                            
                            state.water_level -= transfer;
                        }
                    }
                }
            }
        }
    }

    fn spread_fire(&self, state: &mut CompartmentState, all_compartments: &HashMap<EntityId, CompartmentState>, dt: f32) {
        if state.fire_intensity <= 0.0 {
            return;
        }

        for &connected_id in &state.connected_compartments {
            if let Some(_other) = all_compartments.get(&connected_id) {
                let bulkhead = state.bulkhead_states.iter()
                    .find(|b| b.connects_to == connected_id);
                
                if let Some(bh) = bulkhead {
                    if !bh.is_sealed || bh.is_destroyed {
                        let _spread_chance = state.fire_intensity * 0.1 * dt;
                    }
                }
            }
        }
    }

    pub fn breach(&self, state: &mut CompartmentState) {
        state.is_breached = true;
        state.is_sealed = false;
    }

    pub fn seal(&self, state: &mut CompartmentState) {
        state.is_sealed = true;
    }

    pub fn unseal(&self, state: &mut CompartmentState) {
        state.is_sealed = false;
    }

    pub fn toggle_seal(&self, state: &mut CompartmentState) -> bool {
        state.is_sealed = !state.is_sealed;
        state.is_sealed
    }

    pub fn activate_pump(&self, state: &mut CompartmentState) {
        state.pump_active = true;
    }

    pub fn deactivate_pump(&self, state: &mut CompartmentState) {
        state.pump_active = false;
    }

    pub fn set_fire(&self, state: &mut CompartmentState, intensity: f32) {
        state.fire_intensity = intensity.clamp(0.0, 1.0);
    }

    pub fn extinguish_fire(&self, state: &mut CompartmentState) {
        state.fire_intensity = 0.0;
    }

    pub fn apply_damage(&self, state: &mut CompartmentState, damage: f32, position: Vec3f) {
        // Non-positive damage must not inflate bulkhead health.
        if damage <= 0.0 {
            return;
        }
        for bulkhead in &mut state.bulkhead_states {
            let dist = (bulkhead.local_position - position).length();
            if dist < 5.0 {
                bulkhead.health -= damage * (1.0 - dist / 5.0);
                if bulkhead.health <= 0.0 {
                    bulkhead.is_destroyed = true;
                    bulkhead.is_sealed = false;
                }
            }
        }
    }

    pub fn get_water_volume(&self, state: &CompartmentState) -> f32 {
        state.water_level
    }

    pub fn get_fill_ratio(&self, state: &CompartmentState) -> f32 {
        state.fill_ratio()
    }

    pub fn bounds(&self) -> Bounds {
        self.config.local_bounds
    }

    pub fn is_breached(&self, state: &CompartmentState) -> bool {
        state.is_breached
    }

    pub fn is_sealed(&self, state: &CompartmentState) -> bool {
        state.is_sealed
    }

    pub fn pump_capacity(&self) -> f32 {
        self.config.pump_capacity
    }
}