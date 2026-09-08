pub use crate::math::{Vec3f, Bounds, Transform};
pub use crate::packet::{EntityId, EntityType, EntityFlags, StationType, PlayerPosture, ProjectileType};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

pub trait SpatialEntity {
    fn entity_id(&self) -> EntityId;
    fn position(&self) -> Vec3f;
    fn bounds(&self) -> Bounds;
    fn entity_type(&self) -> EntityType;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShipEntity {
    pub entity_id: EntityId,
    pub ship_class_id: u32,
    pub transform: Transform,
    pub velocity: Vec3f,
    pub angular_velocity: Vec3f,
    pub health: f32,
    pub max_health: f32,
    pub flags: EntityFlags,
    pub fuel: f32,
    pub max_fuel: f32,
    pub speed: f32,
    pub max_speed: f32,
    pub heading: f32,
    pub rudder_angle: f32,
    pub throttle: f32,
    pub compartments: Vec<CompartmentEntity>,
    pub stations: Vec<StationEntity>,
    pub team: u8,
}

impl SpatialEntity for ShipEntity {
    fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    fn position(&self) -> Vec3f {
        self.transform.position
    }

    fn bounds(&self) -> Bounds {
        let half_size = Vec3f::new(50.0, 30.0, 150.0);
        Bounds::new(
            self.transform.position - half_size,
            self.transform.position + half_size,
        )
    }

    fn entity_type(&self) -> EntityType {
        EntityType::Ship
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompartmentEntity {
    pub entity_id: EntityId,
    pub ship_id: EntityId,
    pub name: String,
    /// Truth in the ship-local frame (plan §8.1); never moved by ship motion.
    pub local_bounds: Bounds,
    /// Derived once per tick: local bounds translated to the world frame.
    /// Rotation is intentionally not baked into the extents (ships turn slowly;
    /// interest radii have margin) — only the center follows the ship exactly.
    pub world_bounds: Bounds,
    pub water_level: f32,
    pub max_water_level: f32,
    pub is_sealed: bool,
    pub is_breached: bool,
    pub fire_intensity: f32,
    pub connected_compartments: SmallVec<[EntityId; 4]>,
    pub stations: SmallVec<[EntityId; 8]>,
    pub pump_capacity: f32,
    pub bulkheads: SmallVec<[BulkheadEntity; 4]>,
}

impl SpatialEntity for CompartmentEntity {
    fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    fn position(&self) -> Vec3f {
        self.world_bounds.center()
    }

    fn bounds(&self) -> Bounds {
        self.world_bounds
    }

    fn entity_type(&self) -> EntityType {
        EntityType::Compartment
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BulkheadEntity {
    pub entity_id: EntityId,
    pub compartment_a: EntityId,
    pub compartment_b: EntityId,
    pub local_position: Vec3f,
    pub is_sealed: bool,
    pub is_destroyed: bool,
    pub seal_strength: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StationEntity {
    pub entity_id: EntityId,
    pub ship_id: EntityId,
    pub compartment_id: EntityId,
    pub station_type: StationType,
    /// Truth in the ship-local frame (plan §8.1).
    pub local_transform: Transform,
    /// Derived once per tick from the ship transform + local_transform.
    /// Interest and snapshots must use the world frame.
    pub world_transform: Transform,
    pub occupant: Option<EntityId>,
    pub yaw: f32,
    pub pitch: f32,
    pub reload_progress: f32,
    pub ammo_type: u8,
    pub ammo_count: u32,
    pub max_ammo: u32,
    pub is_operational: bool,
    pub health: f32,
    pub max_health: f32,
    pub cooldown: f32,
    pub max_cooldown: f32,
}

impl SpatialEntity for StationEntity {
    fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    fn position(&self) -> Vec3f {
        self.world_transform.position
    }

    fn bounds(&self) -> Bounds {
        let half_size = Vec3f::new(2.0, 2.0, 2.0);
        Bounds::new(
            self.world_transform.position - half_size,
            self.world_transform.position + half_size,
        )
    }

    fn entity_type(&self) -> EntityType {
        EntityType::Station
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerEntity {
    pub entity_id: EntityId,
    pub player_id: u32,
    pub name: String,
    pub team: u8,
    pub transform: Transform,
    pub velocity: Vec3f,
    pub posture: PlayerPosture,
    pub health: f32,
    pub max_health: f32,
    pub stamina: f32,
    pub max_stamina: f32,
    pub current_station: Option<EntityId>,
    pub current_ship: Option<EntityId>,
    pub input_sequence: u32,
    pub last_acknowledged_tick: u32,
}

impl SpatialEntity for PlayerEntity {
    fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    fn position(&self) -> Vec3f {
        self.transform.position
    }

    fn bounds(&self) -> Bounds {
        let half_size = Vec3f::new(0.5, 0.9, 0.5);
        Bounds::new(
            self.transform.position - half_size,
            self.transform.position + half_size,
        )
    }

    fn entity_type(&self) -> EntityType {
        EntityType::Player
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectileEntity {
    pub entity_id: EntityId,
    pub projectile_type: ProjectileType,
    pub position: Vec3f,
    pub velocity: Vec3f,
    pub spawn_tick: u32,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub owner: EntityId,
    pub weapon: EntityId,
    pub damage: f32,
    pub penetration: f32,
    pub explosion_radius: f32,
    pub has_exploded: bool,
}

impl SpatialEntity for ProjectileEntity {
    fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    fn position(&self) -> Vec3f {
        self.position
    }

    fn bounds(&self) -> Bounds {
        let radius = 0.25;
        let half = Vec3f::new(radius, radius, radius);
        Bounds::new(self.position - half, self.position + half)
    }

    fn entity_type(&self) -> EntityType {
        EntityType::Projectile
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShipClass {
    pub class_id: u32,
    pub name: String,
    pub description: String,
    pub hull_model: String,
    pub displacement: f32,
    pub max_speed: f32,
    pub acceleration: f32,
    pub turn_rate: f32,
    pub max_health: f32,
    pub max_fuel: f32,
    pub armor_thickness: f32,
    pub compartments: Vec<CompartmentTemplate>,
    pub stations: Vec<StationTemplate>,
    pub crew_min: u32,
    pub crew_max: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompartmentTemplate {
    pub name: String,
    pub local_bounds: Bounds,
    pub max_water_level: f32,
    pub pump_capacity: f32,
    pub connected_compartments: Vec<String>,
    pub bulkheads: Vec<BulkheadTemplate>,
    pub stations: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BulkheadTemplate {
    pub name: String,
    pub local_position: Vec3f,
    pub connects_to: String,
    pub seal_strength: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StationTemplate {
    pub name: String,
    pub station_type: StationType,
    pub compartment_name: String,
    pub local_transform: Transform,
    pub max_ammo: u32,
    pub default_ammo_type: u8,
    pub cooldown: f32,
    pub max_health: f32,
    pub yaw_range: (f32, f32),
    pub pitch_range: (f32, f32),
}