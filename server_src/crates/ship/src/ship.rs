use crate::compartment::{Compartment, CompartmentState, WaterTransfer};
use crate::station::{DefaultBehavior, FireResult, Station, StationState};
use parking_lot::RwLock;
use rfs_core::entity::{CompartmentTemplate, EntityId, ShipClass, StationTemplate, StationType};
use rfs_core::math::{Bounds as CoreBounds, Quatf, Transform, Vec3f};
use rfs_damage::damage::{CompartmentHit, DamageSystem, DamageTarget, StationHit};
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
                    (
                        *id,
                        cfg.name.clone(),
                        cfg.max_water_level,
                        cfg.pump_capacity,
                    )
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
            .map(|(i, t)| {
                (
                    t.name.clone(),
                    EntityId::new(self.entity_id.0 + 1000 + i as u64),
                )
            })
            .collect();

        let mut states = self.state.write();
        for (i, template) in self
            .config
            .compartments
            .iter()
            .enumerate()
            .take(MAX_COMPARTMENTS)
        {
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
            .map(|(i, t)| {
                (
                    t.name.clone(),
                    EntityId::new(self.entity_id.0 + 1000 + i as u64),
                )
            })
            .collect();

        // Guard station region spill: compartments use +1000+i, stations
        // +2000+i — cap stations so they never reach bulkhead region (+3000).
        const MAX_STATIONS: usize = 900;
        let templates = self.config.stations.clone();
        for (i, template) in templates.iter().enumerate().take(MAX_STATIONS) {
            let station_id = EntityId::new(self.entity_id.0 + 2000 + i as u64);
            let compartment_id = name_to_comp
                .get(&template.compartment_name)
                .copied()
                .unwrap_or_else(EntityId::nil);

            let station = Station::new(station_id, self.entity_id, template.clone());

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
            // Bug №670: the accessor is the single mutable door for an
            // inserted station; the template binding is applied after the
            // insert, not on a local before it.
            if let Some(station) = self.get_station_mut(station_id) {
                station.set_compartment_id(compartment_id);
            }
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
        // Bug №716: the compartments live in a HashMap, and the iteration order
        // decided which compartment saw which mid-tick value and in which order
        // the collected transfers later hit the map — the same tick produced
        // different water distributions between runs. Sort by EntityId so the
        // model is a function of the state, not of hash order.
        let mut ordered: Vec<_> = self.compartments.values().collect();
        ordered.sort_by_key(|comp| comp.entity_id().0);

        // Phase 1: per-compartment intake / pump outflow / fire decay.
        for comp in &ordered {
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
        for comp in &ordered {
            let comp_id = comp.entity_id();
            if let Some(cs) = state.compartment_states.get(&comp_id) {
                water_transfers.extend(comp.compute_water_spread(
                    cs,
                    &state.compartment_states,
                    dt,
                ));
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

    pub fn apply_damage(&self, damage: f32, position: Vec3f) -> Vec<(EntityId, f32)> {
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
        let report = self
            .damage_system
            .apply_damage_to_ship(self, local_pos, damage);
        if !report.breached.is_empty() {
            let mut state = self.state.write();
            for comp_id in &report.breached {
                if let (Some(compartment), Some(cs)) = (
                    self.compartments.get(comp_id),
                    state.compartment_states.get_mut(comp_id),
                ) {
                    compartment.breach(cs);
                }
            }
        }
        // Bug №607: hand back every compartment the blast reached, so an
        // explosive hit can light the one it struck. The impact point alone
        // names no compartment — the hull collision box a shell stops on is far
        // larger than any compartment inside it.
        report.inflicted
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

    /// Bug №630: whether the station has finished slewing to the angles
    /// `set_station_target_angles` last requested. An unknown station is
    /// "never aimed" — a deferred shot on it can only end in a deadline, not
    /// in a shot.
    pub fn station_aim_reached(&self, station_id: EntityId) -> bool {
        let Some(station) = self.stations.get(&station_id) else {
            return false;
        };
        let state = self.state.read();
        state
            .station_states
            .get(&station_id)
            .is_some_and(|s| station.aim_reached(s))
    }

    pub fn try_fire_station(
        &self,
        station_id: EntityId,
        ammo_type: Option<u8>,
    ) -> Option<FireResult> {
        let station = self.stations.get(&station_id)?;
        let mut state = self.state.write();
        let station_state = state.station_states.get_mut(&station_id)?;
        station.fire(station_state, ammo_type)
    }

    /// Loads a round of `ammo_type` into `station_id`.
    ///
    /// Returns whether the shell was loaded. `false` covers both an unknown
    /// station and a rejected request (Bug №192: an ammo type that is not a
    /// real shell).
    pub fn reload_station(&self, station_id: EntityId, ammo_type: u8) -> bool {
        let Some(station) = self.stations.get(&station_id) else {
            return false;
        };
        let mut state = self.state.write();
        match state.station_states.get_mut(&station_id) {
            Some(station_state) => station.reload(station_state, ammo_type),
            None => false,
        }
    }

    /// Sets a station's default behavior — `Active` puts the station up,
    /// `Passive` and `Disabled` take it down (distinct only as wire codes
    /// for now; bug №669). The command rule is that the occupant of the
    /// station decides.
    ///
    /// Returns whether anything was set. `false` covers both an unknown
    /// station and a station whose state is missing — a pairing the server
    /// is not supposed to break. Setting an already-set behavior succeeds:
    /// repeat requests are idempotent, not errors.
    pub fn set_station_behavior(&self, station_id: EntityId, behavior: DefaultBehavior) -> bool {
        let Some(station) = self.stations.get(&station_id) else {
            return false;
        };
        let mut state = self.state.write();
        match state.station_states.get_mut(&station_id) {
            Some(station_state) => {
                station.set_default_behavior(station_state, behavior);
                true
            }
            None => false,
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

    /// Bug №668: the pump is written through the Compartment API, not by
    /// reaching into the state map from the side. Both direct and toggle
    /// paths share the same helpers.
    pub fn set_compartment_pump(&self, compartment_id: EntityId, active: bool) {
        let Some(comp) = self.compartments.get(&compartment_id) else {
            return;
        };
        let mut state = self.state.write();
        if let Some(cs) = state.compartment_states.get_mut(&compartment_id) {
            if active {
                comp.activate_pump(cs);
            } else {
                comp.deactivate_pump(cs);
            }
        }
    }

    pub fn toggle_compartment_pump(&self, compartment_id: EntityId) -> bool {
        let Some(comp) = self.compartments.get(&compartment_id) else {
            return false;
        };
        let mut state = self.state.write();
        match state.compartment_states.get_mut(&compartment_id) {
            Some(cs) => {
                let was_active = cs.pump_active;
                if was_active {
                    comp.deactivate_pump(cs);
                } else {
                    comp.activate_pump(cs);
                }
                !was_active
            }
            None => false,
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

    /// Bug №668: the compartment-level seal existed as methods with no
    /// caller; the bulkhead seal was live but a breached compartment's own
    /// `is_sealed` flag (which decides whether it keeps taking water at all)
    /// was unreachable from the command path. The new command drives the
    /// same helpers: `Compartment::seal` / `unseal`.
    ///
    /// Returns whether anything was set: an unknown compartment is the only
    /// refusal.
    pub fn set_compartment_seal(&self, compartment_id: EntityId, sealed: bool) -> bool {
        let Some(comp) = self.compartments.get(&compartment_id) else {
            return false;
        };
        let mut state = self.state.write();
        match state.compartment_states.get_mut(&compartment_id) {
            Some(cs) => {
                if sealed {
                    comp.seal(cs);
                } else {
                    comp.unseal(cs);
                }
                true
            }
            None => false,
        }
    }

    /// Apply a repair action to a station. Returns whether anything was
    /// applied: `false` is either an unknown station/state or a station still
    /// on its repair cooldown (Bug №633) — callers that need the difference
    /// read `station_repair_cooldown`.
    pub fn repair_station(&self, station_id: EntityId, amount: f32) -> bool {
        let Some(station) = self.stations.get(&station_id) else {
            return false;
        };
        let mut state = self.state.write();
        if let Some(s) = state.station_states.get_mut(&station_id) {
            station.repair(s, amount)
        } else {
            false
        }
    }

    /// Remaining repair cooldown on a post, `None` when the station or its
    /// state does not exist (Bug №633: lets the command path tell "not found"
    /// apart from "on cooldown" honestly).
    pub fn station_repair_cooldown(&self, station_id: EntityId) -> Option<f32> {
        self.state
            .read()
            .station_states
            .get(&station_id)
            .map(|s| s.repair_cooldown)
    }

    pub fn extinguish_compartment(&self, compartment_id: EntityId) -> bool {
        let Some(compartment) = self.compartments.get(&compartment_id) else {
            return false;
        };
        let mut state = self.state.write();
        let Some(cs) = state.compartment_states.get_mut(&compartment_id) else {
            return false;
        };
        // Bug №607 (analysis 4.5): this used to return `true` whenever the
        // compartment existed, so `ExtinguishFire` acked success on a
        // compartment that was never on fire — the ship-state fire stayed 0.0
        // and the command went into the void. Report honestly: nothing
        // burning means nothing was extinguished.
        if cs.fire_intensity <= 0.0 {
            return false;
        }
        compartment.extinguish_fire(cs);
        true
    }

    /// Lights a fire in `compartment_id`. Returns `true` only when the
    /// compartment was not already burning, so the caller can emit a single
    /// `FireStarted` instead of one per shell that lands on a live fire.
    ///
    /// Bug №607 (analysis 4.5): the fire model, its spread and its decay were
    /// all written and ticked every frame, but nothing on the live path ever
    /// raised `fire_intensity` above zero — the only ignition calls lived in
    /// tests. This is that missing source, driven from explosive hits.
    pub fn ignite_compartment(&self, compartment_id: EntityId, intensity: f32) -> bool {
        if !intensity.is_finite() || intensity <= 0.0 {
            return false;
        }
        let Some(compartment) = self.compartments.get(&compartment_id) else {
            return false;
        };
        let mut state = self.state.write();
        let Some(cs) = state.compartment_states.get_mut(&compartment_id) else {
            return false;
        };
        let was_burning = cs.fire_intensity > 0.0;
        compartment.set_fire(cs, intensity);
        !was_burning && cs.fire_intensity > 0.0
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

    /// Bug №244: this iterated a `HashMap` and returned the first hit. Where
    /// compartment bounds overlap, the answer then depended on hash order —
    /// different between runs and between platforms — so a hit, a shot and a
    /// flood could be attributed to different compartments, and a report could
    /// not be reproduced.
    ///
    /// Sorting by id makes the choice deterministic. Ids are assigned in
    /// template order (`entity_id + 1000 + index`), so the tie-break is the
    /// first compartment in the class file, which is predictable to a player.
    /// `EntityId` derives no `Ord`, so the sort is on its public `u64` rather
    /// than on the wrapper.
    pub fn get_compartment_at(&self, local_pos: Vec3f) -> Option<EntityId> {
        let mut ids: Vec<EntityId> = self.compartments.keys().copied().collect();
        ids.sort_unstable_by_key(|id| id.0);
        ids.into_iter().find(|id| {
            self.compartments
                .get(id)
                .is_some_and(|comp| comp.bounds().contains(local_pos))
        })
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
    pub fn note_hit(
        &self,
        tick: u64,
        position: Vec3f,
        normal: Vec3f,
        compartment: Option<EntityId>,
        damage: f32,
    ) {
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

    /// Bug №244, same defect as `get_compartment_at`: station bounds can overlap,
    /// and the first match out of a `HashMap` was not a stable choice.
    pub fn get_station_at(&self, local_pos: Vec3f) -> Option<EntityId> {
        let mut ids: Vec<EntityId> = self.stations.keys().copied().collect();
        ids.sort_unstable_by_key(|id| id.0);
        ids.into_iter().find(|id| {
            self.stations
                .get(id)
                .is_some_and(|station| station.bounds().contains(local_pos))
        })
    }

    pub fn total_water_volume(&self) -> f32 {
        let state = self.state.read();
        state
            .compartment_states
            .values()
            .map(|c| c.water_level)
            .sum()
    }

    pub fn is_operational(&self) -> bool {
        self.state.read().health > 0.0 && !self.state.read().is_sinking
    }

    /// Bug №701: the raw sinking flag, without the "operational" reading on
    /// top. The sim takes the crossing edge across a whole tick — the flag
    /// can flip during `apply_pending_damage` (`apply_health_damage`, №191's
    /// guard), before `update_ships_parallel` captures its pre-tick state —
    /// so the sim needs this before the tick's fallout lands.
    pub fn is_sinking(&self) -> bool {
        self.state.read().is_sinking
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

    /// Bug №194: stations are reachable by a blast now. They are reported
    /// separately from compartments because they carry no water and no
    /// bulkhead edges, so adding them to the flooding graph would fabricate
    /// capacity for a gun mount.
    fn station_hits(&self) -> Vec<StationHit> {
        self.stations
            .values()
            .map(|st| StationHit {
                entity_id: st.entity_id(),
                name: st.config().compartment_name.clone(),
                bounds: st.bounds(),
            })
            .collect()
    }

    fn apply_station_damage(&self, station_id: EntityId, damage: f32, _position: Vec3f) {
        let Some(station) = self.stations.get(&station_id) else {
            return;
        };
        let mut state = self.state.write();
        if let Some(ss) = state.station_states.get_mut(&station_id) {
            station.apply_damage(ss, damage);
        }
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
        // Bug №191: this guard used to be absent, so every further hit on an
        // already-sinking ship reset `sink_timer` to 30. While damage kept
        // landing the timer could never reach zero, `has_sunk` was never set,
        // and the wreck stayed in the simulation forever — colliding,
        // replicating, and keeping its crew aboard. `check_sinking` already had
        // the `!is_sinking` guard; this path did not.
        if state.health <= 0.0 && !state.is_sinking {
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
            compartments: state
                .compartment_states
                .iter()
                .map(|(id, s)| ShipCompartmentSnapshot {
                    entity_id: *id,
                    water_level: s.water_level,
                    is_sealed: s.is_sealed,
                    is_breached: s.is_breached,
                    fire_intensity: s.fire_intensity,
                })
                .collect(),
            stations: state
                .station_states
                .iter()
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

#[cfg(test)]
mod tests {
    use super::*;
    use rfs_damage::DamageSystem;

    fn ship(max_health: f32) -> Ship {
        let cfg = ShipConfig {
            max_health,
            ..Default::default()
        };
        Ship::new(cfg, EntityId::new(1), Arc::new(DamageSystem::new()))
    }

    /// One compartment holding a single gun mount, so a blast has something
    /// to destroy.
    fn ship_with_station() -> (Ship, EntityId) {
        let cfg = ShipConfig {
            max_health: 1000.0,
            compartments: vec![CompartmentTemplate {
                name: "bridge".into(),
                local_bounds: CoreBounds::new(
                    Vec3f::new(-5.0, -5.0, -5.0),
                    Vec3f::new(5.0, 5.0, 5.0),
                ),
                max_water_level: 100.0,
                pump_capacity: 5.0,
                connected_compartments: Vec::new(),
                bulkheads: Vec::new(),
                stations: vec!["main gun".into()],
            }],
            stations: vec![StationTemplate {
                name: "main gun".into(),
                station_type: StationType::Gun,
                compartment_name: "bridge".into(),
                local_transform: Transform::from_position(Vec3f::ZERO),
                max_health: 50.0,
                max_ammo: 10,
                default_ammo_type: 0,
                cooldown: 1.0,
                yaw_range: (-90.0, 90.0),
                pitch_range: (-30.0, 30.0),
            }],
            ..Default::default()
        };
        // Station ids are derived from the ship id (+2000+index).
        let ship = Ship::new(cfg, EntityId::new(1), Arc::new(DamageSystem::new()));
        (ship, EntityId::new(2001))
    }

    /// The pump and bulkhead commands all reach their compartment *through*
    /// the station, so a station built with an unset template link would
    /// strand them. The binding is decided once, when the station list is
    /// built, and this pins it (bug №670).
    #[test]
    fn a_station_starts_bound_to_its_compartment() {
        let (s, station_id) = ship_with_station();
        let station = s.get_station(station_id).expect("fixture station exists");
        assert_eq!(
            station.compartment_id(),
            EntityId::new(1001),
            "the station must be bound to the first compartment (+1000+0)"
        );
    }

    /// A station's behavior is the mutable connection between a command and
    /// the operational flag: `Disabled` turns the station off, `Active`
    /// turns it back on, and repeating a set is a success, not an error
    /// (bug №669).
    #[test]
    fn a_station_behavior_can_be_set_and_repeated() {
        let (s, station_id) = ship_with_station();
        let operational = |s: &Ship| {
            s.get_state()
                .station_states
                .get(&station_id)
                .expect("fixture station exists")
                .is_operational
        };
        assert!(
            operational(&s),
            "the fixture station starts active (bug №58 pins this)"
        );
        assert!(
            s.set_station_behavior(station_id, DefaultBehavior::Disabled),
            "an existing station renders its behavior unreachable"
        );
        assert!(
            !operational(&s),
            "Disabled must de-operationalize the station"
        );
        // Idempotency: the same request again is a success, not a refusal.
        assert!(s.set_station_behavior(station_id, DefaultBehavior::Disabled));
        assert!(!operational(&s));
        assert!(s.set_station_behavior(station_id, DefaultBehavior::Active));
        assert!(operational(&s), "Active must put the station back");
    }

    /// `Disabled` de-operationalizes the station without evicting the seated
    /// player: seats belong to the player, damage is what clears them, and
    /// the fire refusal carries the same way it does for a broken station.
    #[test]
    fn a_disabled_station_keeps_its_seat() {
        let (s, station_id) = ship_with_station();
        let player = EntityId::new(4242);
        assert!(
            s.occupy_station(station_id, player),
            "fixture could not sit"
        );
        assert!(
            s.set_station_behavior(station_id, DefaultBehavior::Disabled),
            "an existing station renders its behavior unreachable"
        );
        let remained = s
            .get_state()
            .station_states
            .get(&station_id)
            .expect("fixture station exists")
            .occupant;
        assert_eq!(remained, Some(player), "Disabled evicted the seated player");
    }

    /// Bug №194: `Station::apply_damage` had no caller, so a station could
    /// never be destroyed — `is_operational` and `occupant` never cleared and
    /// the gun fired forever.
    #[test]
    fn a_blast_can_destroy_a_station() {
        let (s, station_id) = ship_with_station();
        let before = s.get_state();
        let station = before.station_states.get(&station_id).expect("no station");
        assert!(
            station.is_operational,
            "the fixture station is not operational"
        );
        assert!(station.health > 0.0);

        s.apply_damage(10_000.0, Vec3f::new(1.0, 0.0, 0.0));

        let after = s.get_state();
        let station = after
            .station_states
            .get(&station_id)
            .expect("station vanished");
        assert!(
            !station.is_operational,
            "a direct hit left the station operational"
        );
        assert!(
            station.health <= 0.0,
            "the station kept health after a huge hit"
        );
        assert!(
            station.occupant.is_none(),
            "a destroyed station kept its crew"
        );
    }

    /// The falloff has to apply to stations too: a blast far away must not
    /// reach a gun that a near one destroys.
    #[test]
    fn station_damage_obeys_the_falloff() {
        let (near_ship, station_id) = ship_with_station();
        near_ship.apply_damage(60.0, Vec3f::new(1.0, 0.0, 0.0));
        assert!(
            !near_ship.get_state().station_states[&station_id].is_operational,
            "a 50-health station survived 60 damage at point blank"
        );

        // Same damage, far outside max_damage_radius (12.0).
        let (far_ship, far_station) = ship_with_station();
        far_ship.apply_damage(60.0, Vec3f::new(900.0, 900.0, 900.0));
        let s = far_ship.get_state().station_states[&far_station].clone();
        assert!(s.is_operational, "a station was hit from 1500 units away");
        assert_eq!(s.health, 50.0, "a distant blast damaged the station");
    }

    /// Bug №194 secondary: a template with a negative `max_health` used to
    /// abort the process in `f32::clamp`, because clamp asserts min <= max.
    #[test]
    fn a_negative_station_max_health_does_not_panic() {
        let cfg = ShipConfig {
            max_health: 100.0,
            stations: vec![StationTemplate {
                name: "bad gun".into(),
                station_type: StationType::Gun,
                compartment_name: String::new(),
                local_transform: Transform::from_position(Vec3f::ZERO),
                max_health: -10.0,
                max_ammo: 10,
                default_ammo_type: 0,
                cooldown: 1.0,
                yaw_range: (-90.0, 90.0),
                pitch_range: (-30.0, 30.0),
            }],
            ..Default::default()
        };
        let s = Ship::new(cfg, EntityId::new(1), Arc::new(DamageSystem::new()));
        let station_id = EntityId::new(2001);

        // Must not panic, and must not report a bogus negative health.
        s.apply_damage(100_000.0, Vec3f::new(1.0, 0.0, 0.0));
        let st = s.get_state().station_states[&station_id].clone();
        assert!(st.health >= 0.0, "health went negative: {}", st.health);
        assert!(!st.health.is_nan(), "health became NaN");
    }

    /// Stations must not leak into the flooding graph: they hold no water, so
    /// reporting them as compartments would fabricate capacity and bulkhead
    /// edges for a gun mount.
    #[test]
    fn stations_are_not_treated_as_floodable_compartments() {
        let (s, station_id) = ship_with_station();
        let hits = DamageTarget::compartment_hits(&s);
        assert!(
            !hits.iter().any(|h| h.entity_id == station_id),
            "a station was reported as a water compartment"
        );
        // It must be reachable as a station, though.
        assert!(
            DamageTarget::station_hits(&s)
                .iter()
                .any(|h| h.entity_id == station_id),
            "the station is not in the blast path either, so it is now invulnerable"
        );
    }

    /// `apply_health_damage` is a trait method, so it is reached the same way
    /// the damage system reaches it.
    fn hurt(s: &Ship, amount: f32) {
        DamageTarget::apply_health_damage(s, amount);
    }

    /// Runs `n` ticks of one second, which is what drains `sink_timer`.
    ///
    /// `update` takes the state as a parameter rather than mutating the ship's
    /// own copy, so each tick is written back with `apply_state`. Without that,
    /// damage applied between ticks would read a timer that never advanced.
    fn tick(s: &Ship, n: usize) -> ShipState {
        let mut state = s.get_state();
        for _ in 0..n {
            s.update(1.0, &mut state);
            s.apply_state(state.clone());
        }
        state
    }

    /// Bug №244: compartment bounds can overlap, and the old lookup returned the
    /// first match out of a `HashMap`, so the result varied between runs. A
    /// point inside every compartment is the worst case, because every entry
    /// matches.
    #[test]
    fn an_overlapping_point_always_resolves_to_the_same_compartment() {
        let overlapping = CompartmentTemplate {
            name: "bridge".into(),
            // Every compartment covers the same box, so the point is ambiguous.
            local_bounds: CoreBounds::new(Vec3f::new(-5.0, -5.0, -5.0), Vec3f::new(5.0, 5.0, 5.0)),
            max_water_level: 100.0,
            pump_capacity: 5.0,
            connected_compartments: Vec::new(),
            bulkheads: Vec::new(),
            stations: Vec::new(),
        };
        let cfg = ShipConfig {
            max_health: 1000.0,
            compartments: vec![
                overlapping.clone(),
                overlapping.clone(),
                overlapping.clone(),
                overlapping.clone(),
            ],
            ..Default::default()
        };

        // Many independently built ships, so each gets its own hash seed and
        // its own iteration order.
        let mut answers = std::collections::HashSet::new();
        for _ in 0..64 {
            let s = Ship::new(cfg.clone(), EntityId::new(1), Arc::new(DamageSystem::new()));
            answers.insert(s.get_compartment_at(Vec3f::ZERO));
        }

        assert_eq!(
            answers.len(),
            1,
            "an ambiguous point resolved to different compartments across runs: {:?}",
            answers.len()
        );
    }

    /// The tie-break is the lowest id, which is template order — so the answer is
    /// also the one a player could predict, not merely a stable one.
    #[test]
    fn overlapping_compartments_resolve_to_the_first_in_template_order() {
        let overlapping = CompartmentTemplate {
            name: "same".into(),
            local_bounds: CoreBounds::new(Vec3f::new(-5.0, -5.0, -5.0), Vec3f::new(5.0, 5.0, 5.0)),
            max_water_level: 100.0,
            pump_capacity: 5.0,
            connected_compartments: Vec::new(),
            bulkheads: Vec::new(),
            stations: Vec::new(),
        };
        let cfg = ShipConfig {
            max_health: 1000.0,
            compartments: vec![overlapping.clone(), overlapping.clone(), overlapping],
            ..Default::default()
        };
        let s = Ship::new(cfg, EntityId::new(1), Arc::new(DamageSystem::new()));

        // Compartment ids start at ship_id + 1000 and follow template order.
        assert_eq!(
            s.get_compartment_at(Vec3f::ZERO),
            Some(EntityId::new(1001)),
            "an ambiguous point did not resolve to the first compartment"
        );
    }

    /// Sanity check on the fix itself: a point outside every box still finds
    /// nothing, so sorting did not turn "no match" into "some match".
    #[test]
    fn a_point_outside_every_compartment_still_finds_nothing() {
        let cfg = ShipConfig {
            max_health: 1000.0,
            compartments: vec![CompartmentTemplate {
                name: "bridge".into(),
                local_bounds: CoreBounds::new(
                    Vec3f::new(-5.0, -5.0, -5.0),
                    Vec3f::new(5.0, 5.0, 5.0),
                ),
                max_water_level: 100.0,
                pump_capacity: 5.0,
                connected_compartments: Vec::new(),
                bulkheads: Vec::new(),
                stations: Vec::new(),
            }],
            ..Default::default()
        };
        let s = Ship::new(cfg, EntityId::new(1), Arc::new(DamageSystem::new()));
        assert_eq!(s.get_compartment_at(Vec3f::new(500.0, 0.0, 0.0)), None);
    }

    /// Bug №191: every further hit on a sinking ship used to reset
    /// `sink_timer` to 30, so a ship that kept taking fire never finished
    /// sinking. `has_sunk` stayed false, the sim never despawned the wreck, and
    /// it went on colliding, replicating and holding its crew.
    #[test]
    fn more_damage_does_not_restart_the_sinking_timer() {
        let s = ship(100.0);
        hurt(&s, 500.0);

        let started = s.get_state();
        assert!(started.is_sinking, "the ship did not start sinking");
        assert!(
            (started.sink_timer - 30.0).abs() < 1e-4,
            "timer {}",
            started.sink_timer
        );

        // Ten seconds of sinking: ten seconds should be gone from the timer.
        let mid = tick(&s, 10);
        assert!(mid.is_sinking);
        assert!(
            (mid.sink_timer - 20.0).abs() < 1e-3,
            "expected 20s left after 10s, got {}",
            mid.sink_timer
        );

        // Another hit lands while it is still sinking. Health is already 0, so
        // there is nothing left to take, and the timer must not be rearmed.
        hurt(&s, 10.0);

        let after = s.get_state();
        assert!(
            (after.sink_timer - 20.0).abs() < 1e-3,
            "a hit rearmed the sinking timer to {}",
            after.sink_timer
        );
    }

    /// The end-to-end consequence: a sinking ship that keeps being hit must still
    /// reach `has_sunk`, so the sim can despawn it.
    ///
    /// The order matters. Each round advances a real tick first, then applies a
    /// hit, and reads the ship's own state back. An earlier version ticked a
    /// local copy and applied damage to the ship, so the write-back overwrote
    /// the rearm and the test passed even with the bug present — it was
    /// asserting nothing.
    #[test]
    fn a_sinking_ship_still_finishes_sinking_while_under_fire() {
        let s = ship(100.0);
        hurt(&s, 500.0);

        let mut sunk = false;
        // 35 seconds of fire. With the bug the timer is rearmed to 30 on every
        // hit and never reaches zero inside this window.
        for _ in 0..35 {
            tick(&s, 1);
            hurt(&s, 1.0);
            if s.get_state().has_sunk {
                sunk = true;
                break;
            }
        }

        assert!(
            sunk,
            "the wreck never finished sinking under continuous fire"
        );
    }

    /// A ship that is not sinking must be unaffected by the new guard, and a
    /// single killing blow must still arm the timer.
    #[test]
    fn the_first_killing_blow_arms_the_timer() {
        let s = ship(100.0);
        hurt(&s, 40.0);
        let alive = s.get_state();
        assert!(
            !alive.is_sinking,
            "a damaged but living ship started sinking"
        );
        assert_eq!(alive.sink_timer, 0.0);

        hurt(&s, 80.0);
        let dead = s.get_state();
        assert!(dead.is_sinking);
        assert!((dead.sink_timer - 30.0).abs() < 1e-4);
        assert!(dead.health <= 0.0);
    }

    /// Once sunk, the state must not be re-armed by later damage either.
    #[test]
    fn a_sunk_ship_is_not_resurrected_into_sinking() {
        let s = ship(100.0);
        hurt(&s, 500.0);
        let mut state = s.get_state();
        for _ in 0..31 {
            s.update(1.0, &mut state);
            s.apply_state(state.clone());
        }
        assert!(state.has_sunk, "the ship did not sink in 31 seconds");

        hurt(&s, 50.0);
        let after = s.get_state();
        assert!(after.has_sunk, "a late hit cleared has_sunk");
    }

    /// Bug №607: fire had no live source — `set_fire` was only ever called from
    /// tests, so `fire_intensity` stayed zero in a real match. This is that
    /// source. It reports a *new* fire: the first shell lights a compartment, a
    /// second into an existing fire does not report a fresh start.
    #[test]
    fn igniting_reports_only_a_new_fire() {
        let (s, _) = ship_with_station();
        let compartment = EntityId::new(1001);

        assert!(
            s.ignite_compartment(compartment, 0.5),
            "the first ignite reported nothing"
        );
        assert_eq!(
            s.get_state().compartment_states[&compartment].fire_intensity,
            0.5
        );
        assert!(
            !s.ignite_compartment(compartment, 0.5),
            "a second ignite on a burning compartment reported a new fire"
        );
    }

    /// The same shape as `set_fire`'s guard (Bug №198): garbage must neither
    /// light a fire nor be reported as one.
    #[test]
    fn igniting_refuses_non_finite_and_non_positive_intensity() {
        let (s, _) = ship_with_station();
        let compartment = EntityId::new(1001);

        for bad in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
            assert!(
                !s.ignite_compartment(compartment, bad),
                "{bad} reported a successful ignite"
            );
        }
        assert_eq!(
            s.get_state().compartment_states[&compartment].fire_intensity,
            0.0
        );
    }

    /// Bug №607: extinguish returned `true` for any existing compartment, so a
    /// cold one was acked as put out. It now reports the cold case — and must
    /// still clear a real fire.
    #[test]
    fn extinguish_reports_the_cold_case_and_clears_a_fire() {
        let (s, _) = ship_with_station();
        let compartment = EntityId::new(1001);

        assert!(
            !s.extinguish_compartment(compartment),
            "a cold compartment reported extinguished"
        );

        assert!(s.ignite_compartment(compartment, 0.5));
        assert!(
            s.extinguish_compartment(compartment),
            "a burning compartment was not extinguished"
        );
        assert_eq!(
            s.get_state().compartment_states[&compartment].fire_intensity,
            0.0
        );
        assert!(
            !s.extinguish_compartment(compartment),
            "extinguishing an already-cold compartment reported success"
        );
    }

    /// A compartment that is not there is refused by both, so a bad id cannot
    /// light or douse anything.
    #[test]
    fn ignite_and_extinguish_refuse_an_unknown_compartment() {
        let (s, _) = ship_with_station();
        let missing = EntityId::new(9999);
        assert!(!s.ignite_compartment(missing, 0.5));
        assert!(!s.extinguish_compartment(missing));
    }

    /// Bug №668: the compartment's hull-side seal — the flag a breached
    /// compartment checks before taking water at all — was unreachable from
    /// the command path, because the pump/bulkhead paths wrote the state map
    /// from the side. The command now drives `Compartment::seal`/`unseal`,
    /// an unknown compartment is the only refusal, and repeating a set is
    /// a success, not an error.
    #[test]
    fn a_compartment_seal_can_be_set_and_repeated() {
        let (s, _) = ship_with_station();
        let compartment = EntityId::new(1001);

        assert!(s.set_compartment_seal(compartment, true));
        assert!(s.get_state().compartment_states[&compartment].is_sealed);

        assert!(s.set_compartment_seal(compartment, true));
        assert!(s.get_state().compartment_states[&compartment].is_sealed);

        assert!(s.set_compartment_seal(compartment, false));
        assert!(!s.get_state().compartment_states[&compartment].is_sealed);

        assert!(
            !s.set_compartment_seal(EntityId::new(9999), true),
            "an unknown compartment was acked"
        );
    }
}
