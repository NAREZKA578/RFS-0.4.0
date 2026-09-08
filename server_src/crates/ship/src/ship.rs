use rfs_core::entity::{ShipClass, CompartmentTemplate, StationTemplate, StationType, EntityId};
use rfs_core::math::{Vec3f, Transform, Quatf, Bounds as CoreBounds};
use rfs_core::time::FIXED_DT;
use crate::station::{FireResult, Station, StationState};
use crate::compartment::{Compartment, CompartmentState};
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
        
        ship
    }

    fn initialize_compartments(&mut self) {
        // Ship-local names resolve to ids once here so the state copy is
        // born complete (bug №52 id scheme + bug №54 empty bulkheads).
        let name_to_id: HashMap<String, EntityId> = self
            .config
            .compartments
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name.clone(), EntityId::new(self.entity_id.0 + 1000 + i as u64)))
            .collect();

        let mut states = self.state.write();
        for (i, template) in self.config.compartments.iter().enumerate() {
            let comp_id = EntityId::new(self.entity_id.0 + 1000 + i as u64);

            let connected: Vec<EntityId> = template
                .connected_compartments
                .iter()
                .filter_map(|name| name_to_id.get(name).copied())
                .collect();

            // bug №52: bulkheads live in their own region (3000 + comp_rank*8 + bh_rank)
            // so they can never collide with another ship's compartments/stations.
            let bulkheads: Vec<crate::compartment::BulkheadState> = template
                .bulkheads
                .iter()
                .enumerate()
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

        for (i, template) in self.config.stations.iter().enumerate() {
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
        self.check_sinking(state);
    }

    fn update_physics(&self, dt: f32, state: &mut ShipState) {
        let throttle = state.throttle.clamp(-1.0, 1.0);
        // turn_rate comes from JSON unvalidated; a negative value must not
        // turn the clamp into a panic (min > max).
        let turn_limit = self.config.turn_rate.abs();
        let rudder = state.rudder_angle.clamp(-turn_limit, turn_limit);
        
        let forward = state.transform.rotation.mul_vec3(Vec3f::FORWARD);
        
        let thrust_force = throttle * self.config.acceleration * self.config.mass;
        let drag_force = -state.velocity.length() * state.velocity * 0.1;
        
        let acceleration = (forward * thrust_force + drag_force) / self.config.mass;
        state.velocity += acceleration * dt;
        state.transform.position += state.velocity * dt;
        
        state.speed = state.velocity.length();
        
        let turn_torque = rudder * self.config.turn_rate * state.speed;
        state.angular_velocity.y += turn_torque * dt;
        state.angular_velocity *= 0.98;
        
        let yaw_change = state.angular_velocity.y * dt;
        state.heading += yaw_change;
        // from_euler(yaw, pitch, roll): heading spins around Y, so it goes first.
        state.transform.rotation = Quatf::from_euler(state.heading, 0.0, 0.0);
    }

    fn update_compartments(&self, dt: f32, state: &mut ShipState) {
        let compartment_states = state.compartment_states.clone();
        for (comp_id, compartment) in &self.compartments {
            if let Some(comp_state) = state.compartment_states.get_mut(comp_id) {
                compartment.update(dt, comp_state, &compartment_states);
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

    fn check_sinking(&self, state: &mut ShipState) {
        if state.health <= 0.0 && !state.is_sinking {
            state.is_sinking = true;
            state.sink_timer = 30.0;
        }
        
        if state.is_sinking {
            state.sink_timer -= FIXED_DT;
            state.transform.position.y -= 0.1 * FIXED_DT;
            
            if state.sink_timer <= 0.0 {
            }
        }
    }

    pub fn set_throttle(&self, throttle: f32) {
        self.state.write().throttle = throttle.clamp(-1.0, 1.0);
    }

    pub fn set_rudder(&self, angle: f32) {
        let turn_limit = self.config.turn_rate.abs();
        self.state.write().rudder_angle = angle.clamp(-turn_limit, turn_limit);
    }

    pub fn apply_damage(&self, damage: f32, position: Vec3f) {
        let local_pos = {
            let state = self.state.read();
            state.transform.inverse().transform_point(position)
        };
        self.damage_system.apply_damage_to_ship(self, local_pos, damage);
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
        let Some(station_state) = state.station_states.get_mut(&station_id) else {
            return false;
        };
        if !station.can_occupy(station_state) {
            return false;
        }
        station_state.occupant = Some(player_id);
        station_state.is_operational = true;
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
            let occupant = station_state.occupant.take();
            occupant
        } else {
            None
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
        let mut state = self.state.write();
        state.health = (state.health - amount).clamp(0.0, self.config.max_health);
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