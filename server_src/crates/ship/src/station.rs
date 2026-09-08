use rfs_core::entity::{StationTemplate, StationType, EntityId, Transform, Bounds};
use rfs_core::math::Vec3f;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationConfig {
    pub station_type: StationType,
    pub compartment_name: String,
    pub local_transform: Transform,
    pub max_ammo: u32,
    pub default_ammo_type: u8,
    pub reload_time: f32,
    pub max_health: f32,
    pub yaw_range: (f32, f32),
    pub pitch_range: (f32, f32),
    pub turn_speed: f32,
    pub fire_rate: f32,
}

impl Default for StationConfig {
    fn default() -> Self {
        Self {
            station_type: StationType::Gun,
            compartment_name: "main_deck".to_string(),
            local_transform: Transform::IDENTITY,
            max_ammo: 100,
            default_ammo_type: 0,
            reload_time: 10.0,
            max_health: 500.0,
            yaw_range: (-90.0_f32.to_radians(), 90.0_f32.to_radians()),
            pitch_range: (-30.0_f32.to_radians(), 45.0_f32.to_radians()),
            turn_speed: 1.0,
            fire_rate: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationState {
    pub station_type: StationType,
    pub occupant: Option<EntityId>,
    pub yaw: f32,
    pub pitch: f32,
    pub target_yaw: f32,
    pub target_pitch: f32,
    pub reload_progress: f32,
    pub ammo_type: u8,
    pub ammo_count: u32,
    pub max_ammo: u32,
    pub is_operational: bool,
    pub health: f32,
    pub max_health: f32,
    pub cooldown: f32,
    pub last_fire_time: f32,
}

impl Default for StationState {
    fn default() -> Self {
        Self {
            station_type: StationType::Gun,
            occupant: None,
            yaw: 0.0,
            pitch: 0.0,
            target_yaw: 0.0,
            target_pitch: 0.0,
            reload_progress: 1.0,
            ammo_type: 0,
            ammo_count: 100,
            max_ammo: 100,
            is_operational: true,
            health: 500.0,
            max_health: 500.0,
            cooldown: 0.0,
            last_fire_time: 0.0,
        }
    }
}

/// A station is a config holder. All mutable state lives in
/// `ShipState::station_states` (single source of truth, plan §3.x / bug №53):
/// the tick copy is what the physics tick mutates, what `fire`/`reload`/
/// occupancy mutate, and what gets replicated.
pub struct Station {
    config: StationConfig,
    compartment_id: EntityId,
    entity_id: EntityId,
    ship_id: EntityId,
}

impl Station {
    pub fn new(entity_id: EntityId, ship_id: EntityId, template: StationTemplate) -> Self {
        let config = StationConfig {
            station_type: template.station_type,
            compartment_name: template.compartment_name,
            local_transform: template.local_transform,
            max_ammo: template.max_ammo,
            default_ammo_type: template.default_ammo_type,
            reload_time: template.cooldown,
            max_health: template.max_health,
            yaw_range: (-template.yaw_range.1, template.yaw_range.1),
            pitch_range: (-template.pitch_range.1, template.pitch_range.1),
            turn_speed: 2.0,
            fire_rate: 1.0 / template.cooldown.max(0.1),
        };

        Self {
            config,
            compartment_id: EntityId::new(0),
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

    pub fn compartment_id(&self) -> EntityId {
        self.compartment_id
    }

    pub fn set_compartment_id(&mut self, id: EntityId) {
        self.compartment_id = id;
    }

    pub fn config(&self) -> &StationConfig {
        &self.config
    }

    pub fn update(&self, dt: f32, state: &mut StationState, compartment_state: Option<&crate::compartment::CompartmentState>) {
        if !state.is_operational || state.health <= 0.0 {
            return;
        }

        if let Some(comp) = compartment_state {
            if comp.is_flooded() || comp.fire_intensity > 0.5 {
                state.is_operational = false;
                return;
            }
        }

        state.cooldown = (state.cooldown - dt).max(0.0);

        let yaw_diff = state.target_yaw - state.yaw;
        let pitch_diff = state.target_pitch - state.pitch;
        
        state.yaw += yaw_diff.signum() * self.config.turn_speed * dt;
        state.pitch += pitch_diff.signum() * self.config.turn_speed * dt;
        
        state.yaw = state.yaw.clamp(self.config.yaw_range.0, self.config.yaw_range.1);
        state.pitch = state.pitch.clamp(self.config.pitch_range.0, self.config.pitch_range.1);

        if state.reload_progress < 1.0 {
            state.reload_progress += dt / self.config.reload_time;
            state.reload_progress = state.reload_progress.min(1.0);
        }
    }

    pub fn set_target_angles(&self, state: &mut StationState, yaw: f32, pitch: f32) {
        state.target_yaw = yaw.clamp(self.config.yaw_range.0, self.config.yaw_range.1);
        state.target_pitch = pitch.clamp(self.config.pitch_range.0, self.config.pitch_range.1);
    }

    pub fn get_angles(&self, state: &StationState) -> (f32, f32) {
        (state.yaw, state.pitch)
    }

    pub fn can_fire(&self, state: &StationState) -> bool {
        state.is_operational &&
        state.health > 0.0 &&
        state.cooldown <= 0.0 &&
        state.reload_progress >= 1.0 &&
        state.ammo_count > 0
    }

    pub fn fire(&self, state: &mut StationState, ammo_type: Option<u8>) -> Option<FireResult> {
        if !self.can_fire(state) {
            return None;
        }

        if let Some(at) = ammo_type {
            state.ammo_type = at;
        }
        
        state.ammo_count = state.ammo_count.saturating_sub(1);
        state.reload_progress = 0.0;
        state.cooldown = 1.0 / self.config.fire_rate;
        state.last_fire_time = 0.0;
        
        let direction = self.calculate_fire_direction(state.yaw, state.pitch);
        
        Some(FireResult {
            station_id: self.entity_id,
            position: self.local_transform().position,
            direction,
            velocity: direction * self.get_muzzle_velocity(),
            projectile_type: self.ammo_type_to_projectile(state.ammo_type),
            damage: self.get_damage(),
            penetration: self.get_penetration(),
        })
    }

    fn calculate_fire_direction(&self, yaw: f32, pitch: f32) -> Vec3f {
        let cp = pitch.cos();
        Vec3f::new(
            cp * yaw.sin(),
            pitch.sin(),
            cp * yaw.cos(),
        )
    }

    fn get_muzzle_velocity(&self) -> f32 {
        match self.config.station_type {
            StationType::Gun => 400.0,
            StationType::Engine => 0.0,
            StationType::Pump => 0.0,
            StationType::Helm => 0.0,
            _ => 200.0,
        }
    }

    fn get_damage(&self) -> f32 {
        match self.config.station_type {
            StationType::Gun => 100.0,
            _ => 0.0,
        }
    }

    fn get_penetration(&self) -> f32 {
        match self.config.station_type {
            StationType::Gun => 50.0,
            _ => 0.0,
        }
    }

    fn ammo_type_to_projectile(&self, ammo_type: u8) -> rfs_core::packet::ProjectileType {
        match ammo_type {
            0 => rfs_core::packet::ProjectileType::Cannonball,
            1 => rfs_core::packet::ProjectileType::ExplosiveShell,
            2 => rfs_core::packet::ProjectileType::ArmorPiercing,
            3 => rfs_core::packet::ProjectileType::ChainShot,
            4 => rfs_core::packet::ProjectileType::GrapeShot,
            _ => rfs_core::packet::ProjectileType::Cannonball,
        }
    }

    pub fn reload(&self, state: &mut StationState, ammo_type: u8) {
        if state.reload_progress >= 1.0 && state.ammo_count < state.max_ammo {
            state.ammo_type = ammo_type;
            state.reload_progress = 0.0;
        }
    }

    pub fn repair(&self, state: &mut StationState, amount: f32) {
        // Negative repairs must not damage the station.
        state.health = (state.health + amount.max(0.0)).min(state.max_health);
        if state.health > 0.0 {
            state.is_operational = true;
        }
    }

    pub fn apply_damage(&self, state: &mut StationState, damage: f32) {
        // Clamped both ways: negative damage must not overheal above max.
        state.health = (state.health - damage).clamp(0.0, state.max_health);
        if state.health <= 0.0 {
            state.is_operational = false;
            state.occupant = None;
        }
    }

    pub fn can_occupy(&self, state: &StationState) -> bool {
        state.is_operational && state.health > 0.0 && state.occupant.is_none()
    }

    pub fn get_occupant(&self, state: &StationState) -> Option<EntityId> {
        state.occupant
    }

    pub fn local_transform(&self) -> Transform {
        self.config.local_transform
    }

    pub fn bounds(&self) -> Bounds {
        let half_size = Vec3f::new(2.0, 2.0, 2.0);
        let pos = self.local_transform().position;
        Bounds::new(pos - half_size, pos + half_size)
    }

    pub fn station_type(&self) -> StationType {
        self.config.station_type
    }

    pub fn set_default_behavior(&self, state: &mut StationState, behavior: DefaultBehavior) {
        state.is_operational = matches!(behavior, DefaultBehavior::Active);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireResult {
    pub station_id: EntityId,
    pub position: Vec3f,
    pub direction: Vec3f,
    pub velocity: Vec3f,
    pub projectile_type: rfs_core::packet::ProjectileType,
    pub damage: f32,
    pub penetration: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefaultBehavior {
    Active,
    Passive,
    Disabled,
}