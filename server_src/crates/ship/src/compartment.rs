use rfs_core::entity::{Bounds, CompartmentTemplate, EntityId, Vec3f};
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

/// A pending water move between two compartments (Bug №90). The ship applies
/// both sides of the transfer so volume is conserved.
#[derive(Debug, Clone, Copy)]
pub struct WaterTransfer {
    pub from: EntityId,
    pub to: EntityId,
    pub amount: f32,
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

    pub(crate) fn config_mut(&mut self) -> &mut CompartmentConfig {
        &mut self.config
    }

    pub fn update(&self, dt: f32, state: &mut CompartmentState) {
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
    }

    /// Bug №90: compute how much water this compartment wants to push into
    /// each open neighbour. The caller applies the transfers to the real map,
    /// so the receiver actually gains what the source loses.
    pub fn compute_water_spread(
        &self,
        state: &CompartmentState,
        all_compartments: &HashMap<EntityId, CompartmentState>,
        dt: f32,
    ) -> Vec<WaterTransfer> {
        let mut transfers = Vec::new();
        if self.get_water_volume(state) <= 0.0 {
            return transfers;
        }
        for &connected_id in &state.connected_compartments {
            if let Some(other) = all_compartments.get(&connected_id) {
                let bulkhead = state
                    .bulkhead_states
                    .iter()
                    .find(|b| b.connects_to == connected_id);
                if let Some(bh) = bulkhead {
                    if !bh.is_sealed || bh.is_destroyed {
                        let diff = self.get_fill_ratio(state) - other.fill_ratio();
                        if diff > 0.0 {
                            let flow = diff * 50.0 * dt;
                            // Cap by what the receiver has room for; the
                            // caller additionally caps by the source water.
                            let room = (other.max_water_level - other.water_level).max(0.0);
                            let transfer = flow.min(state.water_level).min(room);
                            if transfer > 0.0 {
                                transfers.push(WaterTransfer {
                                    from: self.entity_id,
                                    to: connected_id,
                                    amount: transfer,
                                });
                            }
                        }
                    }
                }
            }
        }
        transfers
    }

    /// Bug №90: fire spreads to open neighbours instead of the old dead code
    /// that computed `_spread_chance` and threw it away.
    pub fn fire_spread(
        &self,
        state: &CompartmentState,
        all_compartments: &HashMap<EntityId, CompartmentState>,
        dt: f32,
    ) -> Vec<(EntityId, f32)> {
        let mut spreads = Vec::new();
        // Bug №198: NaN made this guard false — "burning" — and
        // `(NaN * 0.1 * dt).clamp(0.0, 1.0)` stayed NaN, so unusable
        // intensities would have been handed to every open neighbour.
        let intensity = rfs_damage::unit_or_zero(state.fire_intensity);
        if intensity <= 0.0 {
            return spreads;
        }
        for &connected_id in &state.connected_compartments {
            if let Some(_other) = all_compartments.get(&connected_id) {
                let bulkhead = state
                    .bulkhead_states
                    .iter()
                    .find(|b| b.connects_to == connected_id);
                if let Some(bh) = bulkhead {
                    if !bh.is_sealed || bh.is_destroyed {
                        let amount = rfs_damage::unit_or_zero(intensity * 0.1 * dt);
                        if amount > 0.0 {
                            spreads.push((connected_id, amount));
                        }
                    }
                }
            }
        }
        spreads
    }

    pub fn breach(&self, state: &mut CompartmentState) {
        state.is_breached = true;
        // Bug №668: the seal/unseal pair is the single way a breached state's
        // open flag is written, so the blast path drives the same helper as
        // the seal command does — not a parallel raw write.
        self.unseal(state);
    }

    pub fn seal(&self, state: &mut CompartmentState) {
        state.is_sealed = true;
    }

    pub fn unseal(&self, state: &mut CompartmentState) {
        state.is_sealed = false;
    }

    pub fn activate_pump(&self, state: &mut CompartmentState) {
        state.pump_active = true;
    }

    pub fn deactivate_pump(&self, state: &mut CompartmentState) {
        state.pump_active = false;
    }

    pub fn set_fire(&self, state: &mut CompartmentState, intensity: f32) {
        // Bug №198: `f32::clamp` returns NaN unchanged, so this stored a NaN
        // intensity. Every `fire_intensity <= 0.0` guard downstream is then
        // false and the compartment reads as burning forever. Refused rather
        // than written as zero, so garbage cannot extinguish live fire either.
        if !intensity.is_finite() {
            return;
        }
        state.fire_intensity = intensity.clamp(0.0, 1.0);
    }

    pub fn extinguish_fire(&self, state: &mut CompartmentState) {
        state.fire_intensity = 0.0;
    }

    pub fn apply_damage(&self, state: &mut CompartmentState, damage: f32, position: Vec3f) {
        // Non-positive damage must not inflate bulkhead health.
        //
        // Bug №197: NaN also passes that guard, and `f32::max` handed back the
        // full health instead of leaving the bulkhead alone.
        let damage = rfs_damage::positive_or_zero(damage);
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

    /// Bug №668: these two were public getters no one called — the volume
    /// answer belongs to the water-spread path, and the spread path now
    /// reads it here, so the helpers stay off the public surface with one
    /// internal consumer instead.
    fn get_water_volume(&self, state: &CompartmentState) -> f32 {
        state.water_level
    }

    fn get_fill_ratio(&self, state: &CompartmentState) -> f32 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rfs_core::entity::CompartmentTemplate;

    fn empty_state() -> (Compartment, CompartmentState) {
        let template = CompartmentTemplate {
            name: "test".to_string(),
            local_bounds: Bounds::new(Vec3f::ZERO, Vec3f::new(1.0, 1.0, 1.0)),
            max_water_level: 100.0,
            pump_capacity: 10.0,
            connected_compartments: Vec::new(),
            bulkheads: Vec::new(),
            stations: Vec::new(),
        };
        let comp = Compartment::new(
            EntityId::new(1001),
            EntityId::new(1),
            template,
            Vec::new(),
            Vec::new(),
        );
        // A freshly built compartment is born sealed, as `Ship::new` leaves it.
        let state = CompartmentState {
            water_level: 0.0,
            max_water_level: 100.0,
            is_sealed: true,
            is_breached: false,
            fire_intensity: 0.0,
            pump_active: false,
            pump_capacity: 10.0,
            bulkhead_states: Vec::new(),
            connected_compartments: Vec::new(),
        };
        (comp, state)
    }

    /// Bug №668: the blast path must write the open flag through the same
    /// helper the seal command does, not a parallel raw write — otherwise
    /// the two paths can drift apart.
    #[test]
    fn a_breach_opens_the_seal_through_the_unseal_helper() {
        let (comp, mut cs) = empty_state();
        comp.breach(&mut cs);
        assert!(cs.is_breached);
        assert!(!cs.is_sealed, "a breach must stay open to take water");

        comp.seal(&mut cs);
        assert!(cs.is_sealed);
    }

    /// The whole point of the compartment seal: a breached compartment takes
    /// water only while it is open; sealing stops the intake, unsealing lets
    /// it back. This rides the live tick path (`update` is called by the
    /// ship every frame).
    #[test]
    fn a_sealed_breach_stops_taking_water() {
        let (comp, mut cs) = empty_state();
        comp.breach(&mut cs);

        comp.update(1.0, &mut cs);
        assert_eq!(cs.water_level, 10.0);

        comp.seal(&mut cs);
        comp.update(1.0, &mut cs);
        assert_eq!(cs.water_level, 10.0, "a sealed breach must not take water");

        comp.unseal(&mut cs);
        comp.update(1.0, &mut cs);
        assert_eq!(cs.water_level, 20.0, "opening the seal lets water back in");
    }

    /// The pump helpers are the single way the flag is written, so the
    /// direct and toggle paths in `Ship` share them.
    #[test]
    fn the_pump_helpers_move_one_flag() {
        let (comp, mut cs) = empty_state();
        comp.activate_pump(&mut cs);
        assert!(cs.pump_active);
        comp.deactivate_pump(&mut cs);
        assert!(!cs.pump_active);
    }
}
