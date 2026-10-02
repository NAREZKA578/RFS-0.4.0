use rfs_core::entity::{ShipClass, CompartmentTemplate, StationTemplate, StationType, EntityId};
use rfs_core::math::{Vec3f, Transform, Quatf, Bounds as CoreBounds};
use crate::station::{FireResult, Station, StationState};
use crate::compartment::{Compartment, CompartmentState, WaterTransfer};
use rfs_damage::damage::{CompartmentHit, DamageSystem, DamageTarget};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipConfig {
    pub class_id: u32,
    pub name: String,
    pub mass: f32,
    pub max_health: f32,
    pub max_fuel: f32,
    pub max_speed: f32,
    pub acceleration: f32,
    pub turn_rate: f32,
    pub armor_thickness: f32,
    pub center_of_mass: Vec3f,
    pub center_of_buoyancy: Vec3f,
    pub waterline_height: f32,
    pub compartments: Vec<CompartmentTemplate>,
    pub stations: Vec<StationTemplate>,
    pub crew_min: u32,
    pub crew_max: u32,
}

impl Default for ShipConfig {
    fn default() -> Self {
        Self {
            class_id: 1,
            name: "Frigate".to_string(),
            mass: 500000.0,
            max_health: 10000.0,
            max_fuel: 10000.0,
            max_speed: 30.0,
            acceleration: 0.5,
            turn_rate: 1.0,
            armor_thickness: 100.0,
            center_of_mass: Vec3f::new(0.0, -2.0, 0.0),
            center_of_buoyancy: Vec3f::new(0.0, -5.0, 0.0),
            waterline_height: 5.0,
            compartments: Vec::new(),
            stations: Vec::new(),
            crew_min: 10,
            crew_max: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipState {
    pub transform: Transform,
    pub velocity: Vec3f,
    pub angular_velocity: Vec3f,
    pub health: f32,
    pub fuel: f32,
    pub speed: f32,
    pub heading: f32,
    pub rudder_angle: f32,
    pub throttle: f32,
    pub compartment_states: HashMap<EntityId, CompartmentState>,
    pub station_states: HashMap<EntityId, StationState>,
    pub is_sinking: bool,
    pub sink_timer: f32,
    /// Bug №57: one-shot "the ship went under" flag — it lets the sim remove
    /// the ship instead of sinking forever.
    pub has_sunk: bool,
    /// Bug №272: the most recent hit taken, replicated in layer 0.
    ///
    /// Set by `apply_damage` and cleared by the replication layer once it has
    /// been published. It exists because the `ShipHit` event became unreliable
    /// (events carry no state, and the client's world lives in the layers), and
    /// the impact normal plus the struck compartment were the only parts of an
    /// event that no layer carried — so without this, losing the event packet
    /// would lose the hit effect and the survivability feedback.
    ///
    /// `None` means "nothing new to publish". The replication side consumes it,
    /// so it is a one-shot marker rather than history.
    pub last_hit: Option<HitMark>,
}

/// Bug №272: durable record of one hit, carried in the ship layer so the client
/// can draw the impact without relying on an event packet.
///
/// Separate from the `ShipHit` event on purpose: the event is a notification
/// that may be lost, this is state that is diffed and eventually consistent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HitMark {
    /// Simulation tick the hit landed on.
    pub tick: u64,
    /// Impact point in the WORLD frame (plan §8.1: ship state is local, this is
    /// the one-time transform to world).
    pub position: Vec3f,
    /// Surface normal in the world frame.
    pub normal: Vec3f,
    /// Compartment that took the hit, when the point resolved to one.
    pub compartment: Option<EntityId>,
    pub damage: f32,
}

impl Default for ShipState {
    fn default() -> Self {
        Self {
            transform: Transform::IDENTITY,
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: 10000.0,
            fuel: 10000.0,
            speed: 0.0,
            heading: 0.0,
            rudder_angle: 0.0,
            throttle: 0.0,
            compartment_states: HashMap::new(),
            station_states: HashMap::new(),
            is_sinking: false,
            sink_timer: 0.0,
            has_sunk: false,
            last_hit: None,
        }
    }
}

pub struct Ship {
    config: ShipConfig,
    state: RwLock<ShipState>,
    pub(crate) compartments: HashMap<EntityId, Compartment>,
    pub(crate) stations: HashMap<EntityId, Station>,
    damage_system: Arc<DamageSystem>,
    entity_id: EntityId,
}

impl Ship {
    pub fn new(config: ShipConfig, entity_id: EntityId, damage_system: Arc<DamageSystem>) -> Self {
        let mut ship = Self {
            config: config.clone(),
            state: RwLock::new(ShipState {
                health: config.max_health,
                fuel: config.max_fuel,
                ..Default::default()
            }),
            compartments: HashMap::new(),
            stations: HashMap::new(),
            damage_system,
            entity_id,
        };
        
        ship.initialize_compartments();
        ship.initialize_stations();

        // Bug №65: the production DamageSystem graph must know the real
        // topology, otherwise water has no edges to flow through — lazily
        // created nodes would have arrived with no bulkheads at all.
        {
            let nodes: Vec<(EntityId, String, f32, f32)> = ship
                .compartments
                .iter()
                .map(|(id, comp)| {
                    let cfg = comp.config();
                    (*id, cfg.name.clone(), cfg.max_water_level, cfg.pump_capacity)
                })
                .collect();
            let edges: Vec<(EntityId, EntityId, EntityId, f32)> = ship
                .compartments
                .values()
                .flat_map(|comp| {
                    let cfg = comp.config();
                    cfg.bulkheads
                        .iter()
                        .filter(|b| !b.connects_to.is_nil())
                        .map(|b| {
                            (
                                b.entity_id,
                                comp.entity_id(),
                                b.connects_to,
                                b.seal_strength,
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            ship.damage_system.register_topology(&nodes, &edges);
        }

        ship
    }

    fn initialize_compartments(&mut self) {
        // Bug №152: compartment/station/bulkhead regions are carved from the
        // ship's id stride (+1000, +2000, +3000). More than 1000 compartments
        // would spill into the station region and collide with another ship's
        // or this ship's other entities. Cap so every id stays unique.
        const MAX_COMPARTMENTS: usize = 900;
        // Ship-local names resolve to ids once here so the state copy is
        // born complete (bug №52 id scheme + bug №54 empty bulkheads).
        let name_to_id: HashMap<String, EntityId> = self
            .config
            .compartments
            .iter()
            .enumerate()
            .take(MAX_COMPARTMENTS)
            .map(|(i, t)| (t.name.clone(), EntityId::new(self.entity_id.0 + 1000 + i as u64)))
            .collect();

        let mut states = self.state.write();
        for (i, template) in self.config.compartments.iter().enumerate().take(MAX_COMPARTMENTS) {
            let comp_id = EntityId::new(self.entity_id.0 + 1000 + i as u64);

            let connected: Vec<EntityId> = template
                .connected_compartments
                .iter()
                .filter_map(|name| name_to_id.get(name).copied())
                .collect();

            // bug №52: bulkheads live in their own region (3000 + comp_rank*8 + bh_rank)
            // so they can never collide with another ship's compartments/stations.
            // Guard: more than 8 bulkheads per compartment would spill into the
            // next compartment region (0*8+8 == 1*8+0) — cap at 8.
            let bulkheads: Vec<crate::compartment::BulkheadState> = template
                .bulkheads
                .iter()
                .enumerate()
                .take(8)
                .map(|(bi, bt)| crate::compartment::BulkheadState {
                    entity_id: EntityId::new(self.entity_id.0 + 3000 + i as u64 * 8 + bi as u64),
                    connects_to: name_to_id
                        .get(&bt.connects_to)
                        .copied()
                        .unwrap_or_else(EntityId::nil),
                    local_position: bt.local_position,
                    is_sealed: true,
                    is_destroyed: false,
                    seal_strength: bt.seal_strength,
                    health: bt.seal_strength * 10.0,
                    max_health: bt.seal_strength * 10.0,
                })
                .collect();

            states.compartment_states.insert(
                comp_id,
                crate::compartment::CompartmentState {
                    water_level: 0.0,
                    max_water_level: template.max_water_level,
                    is_sealed: true,
                    is_breached: false,
                    fire_intensity: 0.0,
                    pump_active: false,
                    pump_capacity: template.pump_capacity,
                    bulkhead_states: bulkheads.clone(),
                    connected_compartments: connected.clone(),
                },
            );

            self.compartments.insert(
                comp_id,
                Compartment::new(
                    comp_id,
                    self.entity_id,
                    template.clone(),
                    connected,
                    bulkheads,
                ),
            );
        }
    }

    fn initialize_stations(&mut self) {
        let name_to_comp: HashMap<String, EntityId> = self
            .config
            .compartments
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name.clone(), EntityId::new(self.entity_id.0 + 1000 + i as u64)))
            .collect();

        // Guard station region spill: compartments use +1000+i, stations
        // +2000+i — cap stations so they never reach bulkhead region (+3000).
        const MAX_STATIONS: usize = 900;
        for (i, template) in self.config.stations.iter().enumerate().take(MAX_STATIONS) {
            let station_id = EntityId::new(self.entity_id.0 + 2000 + i as u64);
            let compartment_id = name_to_comp
                .get(&template.compartment_name)
                .copied()
                .unwrap_or_else(EntityId::nil);

            let mut station = Station::new(station_id, self.entity_id, template.clone());
            station.set_compartment_id(compartment_id);

            self.state.write().station_states.insert(
                station_id,
                StationState {
                    station_type: template.station_type,
                    max_ammo: template.max_ammo,
                    ammo_count: template.max_ammo,
                    ammo_type: template.default_ammo_type,
                    health: template.max_health,
                    max_health: template.max_health,
                    is_operational: true,
                    ..Default::default()
                },
            );
            self.stations.insert(station_id, station);
        }

        // Backfill compartment.config.stations: Compartment::new only knew
        // template station NAMES (mapped to EntityId(0) placeholder). Resolve
        // them now that station ids exist (station template -> compartment).
        {
            let station_by_comp: HashMap<EntityId, Vec<EntityId>> = {
                let mut map: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
                for (sid, st) in &self.stations {
                    map.entry(st.compartment_id()).or_default().push(*sid);
                }
                map
            };
            for (cid, comp) in &mut self.compartments {
                if let Some(list) = station_by_comp.get(cid) {
                    comp.config_mut().stations = list.clone();
                }
            }
        }
    }

    pub fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    pub fn class_id(&self) -> u32 {
        self.config.class_id
    }

    pub fn config(&self) -> &ShipConfig {
        &self.config
    }

    pub fn get_state(&self) -> ShipState {
        self.state.read().clone()
    }

    pub fn apply_state(&self, state: ShipState) {
        *self.state.write() = state;
    }

    pub fn transform(&self) -> Transform {
        self.state.read().transform
    }

    pub fn velocity(&self) -> Vec3f {
        self.state.read().velocity
    }

    pub fn angular_velocity(&self) -> Vec3f {
        self.state.read().angular_velocity
    }

    pub fn health(&self) -> f32 {
        self.state.read().health
    }

    pub fn max_health(&self) -> f32 {
        self.config.max_health
    }

    pub fn fuel(&self) -> f32 {
        self.state.read().fuel
    }

    pub fn max_fuel(&self) -> f32 {
        self.config.max_fuel
    }

    pub fn speed(&self) -> f32 {
        self.state.read().speed
    }

    pub fn heading(&self) -> f32 {
        self.state.read().heading
    }

    pub fn rudder_angle(&self) -> f32 {
        self.state.read().rudder_angle
    }

    pub fn throttle(&self) -> f32 {
        self.state.read().throttle
    }

    pub fn update(&self, dt: f32, state: &mut ShipState) {
        self.update_physics(dt, state);
        self.update_compartments(dt, state);
        self.update_stations(dt, state);
        self.check_sinking(dt, state);
    }

    fn update_physics(&self, dt: f32, state: &mut ShipState) {
        let throttle = state.throttle.clamp(-1.0, 1.0);
        // turn_rate comes from JSON unvalidated; a negative value must not
        // turn the clamp into a panic (min > max).
        let turn_limit = self.config.turn_rate.abs();
        let rudder = state.rudder_angle.clamp(-turn_limit, turn_limit);
        
        let forward = state.transform.rotation.mul_vec3(Vec3f::FORWARD);

        // Bug №57: unvalidated JSON mass (0 / negative / NaN) used to divide
        // into inf/NaN. Clamp to a floor so dynamics always stay finite.
        let mass = self.config.mass.max(0.0001);
        
        let thrust_force = throttle * self.config.acceleration * mass;
        let drag_force = -state.velocity.length() * state.velocity * 0.1;
        
        let acceleration = (forward * thrust_force + drag_force) / mass;
        state.velocity += acceleration * dt;
        state.transform.position += state.velocity * dt;
        
        state.speed = state.velocity.length();
        
        // turn_rate sign must not invert steering; damping must be
        // dt-dependent (pow), not per-tick constant.
        let turn_torque = rudder * turn_limit * state.speed;
        state.angular_velocity.y += turn_torque * dt;
        let damping = 0.98f32.powf(dt * 60.0);
        state.angular_velocity *= damping;
        
        let yaw_change = state.angular_velocity.y * dt;
        state.heading += yaw_change;
        // from_euler(yaw, pitch, roll): heading spins around Y, so it goes first.
        state.transform.rotation = Quatf::from_euler(state.heading, 0.0, 0.0);
    }

    fn update_compartments(&self, dt: f32, state: &mut ShipState) {
        // Phase 1: per-compartment intake / pump outflow / fire decay.
        for comp in self.compartments.values() {
            if let Some(comp_state) = state.compartment_states.get_mut(&comp.entity_id()) {
                comp.update(dt, comp_state);
            }
        }

        // Bug №90: phase 2 collects all transfers/spreads first, then applies
        // them to the real map. The old code passed a read-only CLONE to the
        // neighbours and subtracted from it — the receiver never gained the
        // water, so total_water_volume shrank every tick (x2 with two flooded
        // neighbours). Fire spread was computed and then discarded.
        let mut water_transfers: Vec<WaterTransfer> = Vec::new();
        let mut fire_spreads: Vec<(EntityId, f32)> = Vec::new();
        for comp in self.compartments.values() {
            let comp_id = comp.entity_id();
            if let Some(cs) = state.compartment_states.get(&comp_id) {
                water_transfers.extend(comp.compute_water_spread(cs, &state.compartment_states, dt));
                fire_spreads.extend(comp.fire_spread(cs, &state.compartment_states, dt));
            }
        }

        for transfer in water_transfers {
            let from = transfer.from;
            let to = transfer.to;
            let amount = transfer.amount;
            if amount <= 0.0 || !amount.is_finite() {
                continue;
            }
            // Compute room first; only move what fits — excess stays at the
            // source (fan-in safe, volume conserved, no destruction).
            let room = match state.compartment_states.get(&to) {
                Some(dst) => (dst.max_water_level - dst.water_level).max(0.0),
                None => continue,
            };
            let Some(src) = state.compartment_states.get_mut(&from) else {
                continue;
            };
            if src.water_level <= 0.0 {
                continue;
            }
            let moved = amount.min(src.water_level).min(room);
            if moved <= 0.0 {
                continue;
            }
            src.water_level -= moved;
            if let Some(dst) = state.compartment_states.get_mut(&to) {
                dst.water_level = (dst.water_level + moved).min(dst.max_water_level);
            }
        }

        for (id, amount) in fire_spreads {
            if !amount.is_finite() || amount <= 0.0 {
                continue;
            }
            if let Some(cs) = state.compartment_states.get_mut(&id) {
                // Flooded compartments cannot burn (matches graph gate).
                if cs.water_level > cs.max_water_level * 0.9 {
                    continue;
                }
                cs.fire_intensity = (cs.fire_intensity + amount).min(1.0);
            }
        }
    }

    fn update_stations(&self, dt: f32, state: &mut ShipState) {
        for (station_id, station) in &self.stations {
            if let Some(station_state) = state.station_states.get_mut(station_id) {
                let comp_state = state.compartment_states.get(&station.compartment_id());
                station.update(dt, station_state, comp_state);
            }
        }
    }

    fn check_sinking(&self, dt: f32, state: &mut ShipState) {
        if state.health <= 0.0 && !state.is_sinking {
            state.is_sinking = true;
            state.sink_timer = 30.0;
        }
        
        if state.is_sinking {
            // Bug №57: use the real dt — the old FIXED_DT timer lied the
            // moment the tick rate changed.
            state.sink_timer -= dt;
            state.transform.position.y -= 0.1 * dt;
            
            // Bug №57: the old body was an empty `if` — the sink never
            // finished and the ship was never removed. Flag it for the sim
            // to despawn.
            if state.sink_timer <= 0.0 {
                state.has_sunk = true;
            }
        }
    }

    pub fn set_throttle(&self, throttle: f32) {
        // Bug №150: f32::clamp passes NaN through — a non-finite throttle
        // poisoned physics. Non-finite inputs are treated as 0.
        let mut state = self.state.write();
        state.throttle = if throttle.is_finite() {
            throttle.clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }

    pub fn set_rudder(&self, angle: f32) {
        // Bug №150: same NaN guard. A non-finite turn_rate must not turn the
        // clamp into a panic (min > max) or into NaN output.
        if !angle.is_finite() {
            return;
        }
        let turn_limit = if self.config.turn_rate.is_finite() {
            self.config.turn_rate.abs()
        } else {
            1.0
        };
        self.state.write().rudder_angle = angle.clamp(-turn_limit, turn_limit);
    }

    pub fn apply_damage(&self, damage: f32, position: Vec3f) {
        let local_pos = {
            let state = self.state.read();
            state.transform.inverse().transform_point(position)
        };
        // Bug №151: the damage graph accumulates per-compartment damage in its
        // parallel world and marks breaches there — but nothing ever copied that
        // back into the ship's own compartment states, so battle damage never
        // made a ship flood. The graph now returns every compartment that just
        // breached; mirror it into the real state so Compartment::update starts
        // the water intake.
        let breached = self.damage_system.apply_damage_to_ship(self, local_pos, damage);
        if breached.is_empty() {
            return;
        }
        let mut state = self.state.write();
        for comp_id in breached {
            if let (Some(compartment), Some(cs)) = (
                self.compartments.get(&comp_id),
                state.compartment_states.get_mut(&comp_id),
            ) {
                compartment.breach(cs);
            }
        }
    }

    pub fn get_compartment(&self, comp_id: EntityId) -> Option<&Compartment> {
        self.compartments.get(&comp_id)
    }

    pub fn get_station(&self, station_id: EntityId) -> Option<&Station> {
        self.stations.get(&station_id)
    }

    pub fn get_compartment_mut(&mut self, comp_id: EntityId) -> Option<&mut Compartment> {
        self.compartments.get_mut(&comp_id)
    }

    pub fn get_station_mut(&mut self, station_id: EntityId) -> Option<&mut Station> {
        self.stations.get_mut(&station_id)
    }

    pub fn occupy_station(&self, station_id: EntityId, player_id: EntityId) -> bool {
        let Some(station) = self.stations.get(&station_id) else {
            return false;
        };
        let mut state = self.state.write();
        // Invariant: one player — one station. Reject if the player already
        // occupies any station (prevents N-station hold + ghost states).
        if state
            .station_states
            .values()
            .any(|s| s.occupant == Some(player_id))
        {
            return false;
        }
        let Some(station_state) = state.station_states.get_mut(&station_id) else {
            return false;
        };
        if !station.can_occupy(station_state) {
            return false;
        }
        // Bug №58: occupancy must NOT resurrect a station's operational flag
        // (can_occupy already guarantees it is operational). The old
        // unconditional `is_operational = true` undid damage-based deactivation.
        station_state.occupant = Some(player_id);
        true
    }

    pub fn station_angles(&self, station_id: EntityId) -> Option<(f32, f32)> {
        let station = self.stations.get(&station_id)?;
        let state = self.state.read();
        let station_state = state.station_states.get(&station_id)?;
        Some(station.get_angles(station_state))
    }

    pub fn set_station_target_angles(&self, station_id: EntityId, yaw: f32, pitch: f32) {
        let Some(station) = self.stations.get(&station_id) else {
            return;
        };
        let mut state = self.state.write();
        if let Some(station_state) = state.station_states.get_mut(&station_id) {
            station.set_target_angles(station_state, yaw, pitch);
        }
    }

    pub fn try_fire_station(&self, station_id: EntityId, ammo_type: Option<u8>) -> Option<FireResult> {
        let station = self.stations.get(&station_id)?;
        let mut state = self.state.write();
        let station_state = state.station_states.get_mut(&station_id)?;
        station.fire(station_state, ammo_type)
    }

    pub fn reload_station(&self, station_id: EntityId, ammo_type: u8) {
        let Some(station) = self.stations.get(&station_id) else {
            return;
        };
        let mut state = self.state.write();
        if let Some(station_state) = state.station_states.get_mut(&station_id) {
            station.reload(station_state, ammo_type);
        }
    }

    pub fn vacate_station(&self, station_id: EntityId) -> Option<EntityId> {
        let mut state = self.state.write();
        if let Some(station_state) = state.station_states.get_mut(&station_id) {
            // Operational state is decided by health/flooding in update(),
            // so vacating must NOT flip is_operational off — that would
            // permanently block re-occupation.
            station_state.occupant.take()
        } else {
            None
        }
    }

    // Bug №157: engineering controls previously had no Ship API — the
    // Pump/Repair/Extinguish/Open/Seal commands and station interactions fell
    // into `_ => {}` and did nothing. These are the real implementations.

    pub fn set_compartment_pump(&self, compartment_id: EntityId, active: bool) {
        let mut state = self.state.write();
        if let Some(cs) = state.compartment_states.get_mut(&compartment_id) {
            cs.pump_active = active;
        }
    }

    pub fn toggle_compartment_pump(&self, compartment_id: EntityId) -> bool {
        let mut state = self.state.write();
        if let Some(cs) = state.compartment_states.get_mut(&compartment_id) {
            cs.pump_active = !cs.pump_active;
            true
        } else {
            false
        }
    }

    /// Resolve a pump station id to the compartment it sits in and toggle the
    /// pump of that compartment.
    pub fn toggle_station_pump(&self, station_id: EntityId) -> bool {
        let Some(station) = self.stations.get(&station_id) else {
            return false;
        };
        self.toggle_compartment_pump(station.compartment_id())
    }

    /// Resolve a pump station id to its compartment and set the pump on/off.
    pub fn set_station_pump(&self, station_id: EntityId, active: bool) -> bool {
        let Some(station) = self.stations.get(&station_id) else {
            return false;
        };
        self.set_compartment_pump(station.compartment_id(), active);
        true
    }

    pub fn set_bulkhead_seal(&self, bulkhead_id: EntityId, sealed: bool) -> bool {
        let mut state = self.state.write();
        for cs in state.compartment_states.values_mut() {
            for b in cs.bulkhead_states.iter_mut() {
                if b.entity_id == bulkhead_id {
                    b.is_sealed = sealed;
                    return true;
                }
            }
        }
        false
    }

    pub fn repair_station(&self, station_id: EntityId, amount: f32) -> bool {
        let Some(station) = self.stations.get(&station_id) else {
            return false;
        };
        let mut state = self.state.write();
        if let Some(s) = state.station_states.get_mut(&station_id) {
            station.repair(s, amount);
            true
        } else {
            false
        }
    }

    pub fn extinguish_compartment(&self, compartment_id: EntityId) -> bool {
        let Some(compartment) = self.compartments.get(&compartment_id) else {
            return false;
        };
        let mut state = self.state.write();
        if let Some(cs) = state.compartment_states.get_mut(&compartment_id) {
            compartment.extinguish_fire(cs);
            true
        } else {
            false
        }
    }

    pub fn bounds(&self) -> CoreBounds {
        let half_size = Vec3f::new(50.0, 30.0, 150.0);
        CoreBounds::new(
            self.transform().position - half_size,
            self.transform().position + half_size,
        )
    }

    pub fn world_to_local(&self, world_pos: Vec3f) -> Vec3f {
        self.transform().inverse().transform_point(world_pos)
    }

    pub fn local_to_world(&self, local_pos: Vec3f) -> Vec3f {
        self.transform().transform_point(local_pos)
    }

    pub fn get_compartment_at(&self, local_pos: Vec3f) -> Option<EntityId> {
        for (id, comp) in &self.compartments {
            if comp.bounds().contains(local_pos) {
                return Some(*id);
            }
        }
        None
    }

    /// Bug №272: record a hit for replication in the ship layer.
    ///
    /// The `ShipHit` event is unreliable by design now (events carry no
    /// state), so the durable copy of "this hull was struck here, along this
    /// normal, in this compartment" has to live in the layer instead. The
    /// simulation calls this where it already knows the world-frame normal and
    /// the compartment, so nothing is recomputed here.
    ///
    /// One entry per ship, overwritten by each new hit: a layer delta is built
    /// by diffing two snapshots, so a per-tick list would need to accumulate
    /// history to stay correct, and the client only needs the latest to draw
    /// the effect.
    pub fn note_hit(&self, tick: u64, position: Vec3f, normal: Vec3f, compartment: Option<EntityId>, damage: f32) {
        self.state.write().last_hit = Some(HitMark {
            tick,
            position,
            normal,
            compartment,
            damage,
        });
    }

    /// Bug №272: take the pending hit marker, clearing it.
    ///
    /// Called by the replication path once the mark has been put into a layer.
    /// Consuming it is what makes this a one-shot publish rather than state
    /// that would be re-sent forever: a client that already has the mark does
    /// not need it again, and a client that misses it gets the next hit.
    pub fn take_last_hit(&self) -> Option<HitMark> {
        self.state.write().last_hit.take()
    }

    /// Bug №272: read the pending hit without consuming it.
    pub fn last_hit(&self) -> Option<HitMark> {
        self.state.read().last_hit
    }

    pub fn get_station_at(&self, local_pos: Vec3f) -> Option<EntityId> {
        for (id, station) in &self.stations {
            if station.bounds().contains(local_pos) {
                return Some(*id);
            }
        }
        None
    }

    pub fn total_water_volume(&self) -> f32 {
        let state = self.state.read();
        state.compartment_states.values().map(|c| c.water_level).sum()
    }

    pub fn is_operational(&self) -> bool {
        self.state.read().health > 0.0 && !self.state.read().is_sinking
    }
}

impl DamageTarget for Ship {
    fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    fn compartment_hits(&self) -> Vec<CompartmentHit> {
        self.compartments
            .values()
            .map(|comp| CompartmentHit {
                entity_id: comp.entity_id(),
                name: comp.name().to_string(),
                bounds: comp.bounds(),
                max_water_level: comp.config().max_water_level,
                pump_capacity: comp.pump_capacity(),
            })
            .collect()
    }

    fn apply_health_damage(&self, amount: f32) {
        // NaN/negative guards: NaN damage must not poison health, negative
        // max_health must not panic clamp (min>max).
        if !amount.is_finite() {
            return;
        }
        let max = if self.config.max_health.is_finite() && self.config.max_health > 0.0 {
            self.config.max_health
        } else {
            1.0
        };
        let mut state = self.state.write();
        if !state.health.is_finite() {
            state.health = max;
        }
        state.health = (state.health - amount).clamp(0.0, max);
        if state.health <= 0.0 {
            state.is_sinking = true;
            state.sink_timer = 30.0;
        }
    }

    fn apply_compartment_damage(&self, compartment_id: EntityId, damage: f32, position: Vec3f) {
        let Some(compartment) = self.compartments.get(&compartment_id) else {
            return;
        };
        let mut state = self.state.write();
        if let Some(cs) = state.compartment_states.get_mut(&compartment_id) {
            compartment.apply_damage(cs, damage, position);
        }
    }
}

impl From<ShipClass> for ShipConfig {
    fn from(class: ShipClass) -> Self {
        Self {
            class_id: class.class_id,
            name: class.name,
            mass: class.displacement * 1000.0,
            max_health: class.max_health,
            max_fuel: class.max_fuel,
            max_speed: class.max_speed,
            acceleration: class.acceleration,
            turn_rate: class.turn_rate,
            armor_thickness: class.armor_thickness,
            center_of_mass: Vec3f::new(0.0, -2.0, 0.0),
            center_of_buoyancy: Vec3f::new(0.0, -5.0, 0.0),
            waterline_height: 5.0,
            compartments: class.compartments,
            stations: class.stations,
            crew_min: class.crew_min,
            crew_max: class.crew_max,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipSnapshot {
    pub entity_id: EntityId,
    pub class_id: u32,
    pub transform: Transform,
    pub velocity: Vec3f,
    pub angular_velocity: Vec3f,
    pub health: f32,
    pub fuel: f32,
    pub speed: f32,
    pub heading: f32,
    pub rudder_angle: f32,
    pub throttle: f32,
    pub compartments: Vec<ShipCompartmentSnapshot>,
    pub stations: Vec<ShipStationSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipCompartmentSnapshot {
    pub entity_id: EntityId,
    pub water_level: f32,
    pub is_sealed: bool,
    pub is_breached: bool,
    pub fire_intensity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipStationSnapshot {
    pub entity_id: EntityId,
    pub station_type: StationType,
    pub occupant: Option<EntityId>,
    pub yaw: f32,
    pub pitch: f32,
    pub reload_progress: f32,
    pub ammo_type: u8,
    pub is_operational: bool,
}

impl From<&Ship> for ShipSnapshot {
    fn from(ship: &Ship) -> Self {
        let state = ship.get_state();
        Self {
            entity_id: ship.entity_id(),
            class_id: ship.class_id(),
            transform: state.transform,
            velocity: state.velocity,
            angular_velocity: state.angular_velocity,
            health: state.health,
            fuel: state.fuel,
            speed: state.speed,
            heading: state.heading,
            rudder_angle: state.rudder_angle,
            throttle: state.throttle,
            compartments: state.compartment_states.iter()
                .map(|(id, s)| ShipCompartmentSnapshot {
                    entity_id: *id,
                    water_level: s.water_level,
                    is_sealed: s.is_sealed,
                    is_breached: s.is_breached,
                    fire_intensity: s.fire_intensity,
                })
                .collect(),
            stations: state.station_states.iter()
                .map(|(id, s)| ShipStationSnapshot {
                    entity_id: *id,
                    station_type: s.station_type,
                    occupant: s.occupant,
                    yaw: s.yaw,
                    pitch: s.pitch,
                    reload_progress: s.reload_progress,
                    ammo_type: s.ammo_type,
                    is_operational: s.is_operational,
                })
                .collect(),
        }
    }
}