use rfs_core::entity::*;
use rfs_core::math::{Vec3f, Bounds, Transform};
use rfs_core::packet::{EntityId, EntityType, StationType, PlayerPosture, ProjectileType};

fn make_ship() -> ShipEntity {
    ShipEntity {
        entity_id: EntityId::new(1),
        ship_class_id: 0,
        transform: Transform::from_position(Vec3f::new(10.0, 20.0, 30.0)),
        velocity: Vec3f::ZERO,
        angular_velocity: Vec3f::ZERO,
        health: 100.0,
        max_health: 100.0,
        flags: Default::default(),
        fuel: 50.0,
        max_fuel: 100.0,
        speed: 0.0,
        max_speed: 20.0,
        heading: 0.0,
        rudder_angle: 0.0,
        throttle: 0.0,
        compartments: vec![],
        stations: vec![],
        team: 0,
    }
}

fn make_station() -> StationEntity {
    StationEntity {
        entity_id: EntityId::new(2),
        ship_id: EntityId::new(1),
        compartment_id: EntityId::new(10),
        station_type: StationType::Helm,
        local_transform: Transform::IDENTITY,
        world_transform: Transform::from_position(Vec3f::new(5.0, 6.0, 7.0)),
        occupant: None,
        yaw: 0.0,
        pitch: 0.0,
        reload_progress: 0.0,
        ammo_type: 0,
        ammo_count: 0,
        max_ammo: 100,
        is_operational: true,
        health: 100.0,
        max_health: 100.0,
        cooldown: 0.0,
        max_cooldown: 1.0,
    }
}

fn make_player() -> PlayerEntity {
    PlayerEntity {
        entity_id: EntityId::new(3),
        player_id: 42,
        name: String::new(),
        team: 0,
        transform: Transform::from_position(Vec3f::new(1.0, 2.0, 3.0)),
        velocity: Vec3f::ZERO,
        posture: PlayerPosture::Standing,
        health: 100.0,
        max_health: 100.0,
        stamina: 100.0,
        max_stamina: 100.0,
        current_station: None,
        current_ship: None,
        input_sequence: 0,
        last_acknowledged_tick: 0,
    }
}

fn make_projectile() -> ProjectileEntity {
    ProjectileEntity {
        entity_id: EntityId::new(4),
        projectile_type: ProjectileType::Cannonball,
        position: Vec3f::new(100.0, 200.0, 300.0),
        velocity: Vec3f::new(0.0, 0.0, -50.0),
        spawn_tick: 0,
        lifetime: 5.0,
        max_lifetime: 5.0,
        owner: EntityId::new(1),
        weapon: EntityId::new(2),
        damage: 50.0,
        penetration: 0.5,
        explosion_radius: 0.0,
        has_exploded: false,
    }
}

fn make_ship_class() -> ShipClass {
    ShipClass {
        class_id: 1,
        name: String::new(),
        description: String::new(),
        hull_model: String::new(),
        displacement: 0.0,
        max_speed: 20.0,
        acceleration: 5.0,
        turn_rate: 1.0,
        max_health: 1000.0,
        max_fuel: 500.0,
        armor_thickness: 10.0,
        compartments: vec![],
        stations: vec![],
        crew_min: 0,
        crew_max: 0,
    }
}

fn make_compartment_template() -> CompartmentTemplate {
    CompartmentTemplate {
        name: String::new(),
        local_bounds: Bounds::new(Vec3f::ZERO, Vec3f::new(10.0, 5.0, 20.0)),
        max_water_level: 100.0,
        pump_capacity: 10.0,
        connected_compartments: vec![],
        bulkheads: vec![],
        stations: vec![],
    }
}

fn make_bulkhead_template() -> BulkheadTemplate {
    BulkheadTemplate {
        name: String::new(),
        local_position: Vec3f::ZERO,
        connects_to: String::new(),
        seal_strength: 1.0,
    }
}

fn make_station_template() -> StationTemplate {
    StationTemplate {
        name: String::new(),
        station_type: StationType::Gun,
        compartment_name: String::new(),
        local_transform: Transform::IDENTITY,
        max_ammo: 100,
        default_ammo_type: 0,
        cooldown: 1.0,
        max_health: 100.0,
        yaw_range: (-180.0, 180.0),
        pitch_range: (-45.0, 45.0),
    }
}

// ---------- Serialization roundtrips ----------

#[test]
fn ship_entity_roundtrip() {
    let e = make_ship();
    let json = serde_json::to_string(&e).unwrap();
    let back: ShipEntity = serde_json::from_str(&json).unwrap();
    assert_eq!(back.entity_id, e.entity_id);
    assert_eq!(back.health, e.health);
    assert_eq!(back.team, e.team);
}

#[test]
fn station_entity_roundtrip() {
    let e = make_station();
    let json = serde_json::to_string(&e).unwrap();
    let back: StationEntity = serde_json::from_str(&json).unwrap();
    assert_eq!(back.entity_id, e.entity_id);
    assert_eq!(back.station_type, StationType::Helm);
}

#[test]
fn player_entity_roundtrip() {
    let e = make_player();
    let json = serde_json::to_string(&e).unwrap();
    let back: PlayerEntity = serde_json::from_str(&json).unwrap();
    assert_eq!(back.entity_id, e.entity_id);
    assert_eq!(back.player_id, 42);
}

#[test]
fn projectile_entity_roundtrip() {
    let e = make_projectile();
    let json = serde_json::to_string(&e).unwrap();
    let back: ProjectileEntity = serde_json::from_str(&json).unwrap();
    assert_eq!(back.entity_id, e.entity_id);
    assert_eq!(back.projectile_type, ProjectileType::Cannonball);
}

#[test]
fn ship_class_roundtrip() {
    let e = make_ship_class();
    let json = serde_json::to_string(&e).unwrap();
    let back: ShipClass = serde_json::from_str(&json).unwrap();
    assert_eq!(back.class_id, 1);
}

#[test]
fn compartment_template_roundtrip() {
    let e = make_compartment_template();
    let json = serde_json::to_string(&e).unwrap();
    let back: CompartmentTemplate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_water_level, 100.0);
}

#[test]
fn bulkhead_template_roundtrip() {
    let e = make_bulkhead_template();
    let json = serde_json::to_string(&e).unwrap();
    let back: BulkheadTemplate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.seal_strength, 1.0);
}

#[test]
fn station_template_roundtrip() {
    let e = make_station_template();
    let json = serde_json::to_string(&e).unwrap();
    let back: StationTemplate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.station_type, StationType::Gun);
}

// ---------- SpatialEntity::bounds hardcoded half-sizes ----------

#[test]
fn ship_bounds_half_size() {
    let e = make_ship();
    let b = e.bounds();
    let size = b.size();
    assert!((size.x - 100.0).abs() < 0.001);
    assert!((size.y - 60.0).abs() < 0.001);
    assert!((size.z - 300.0).abs() < 0.001);
}

#[test]
fn station_bounds_half_size() {
    let e = make_station();
    let b = e.bounds();
    let size = b.size();
    assert!((size.x - 4.0).abs() < 0.001);
    assert!((size.y - 4.0).abs() < 0.001);
    assert!((size.z - 4.0).abs() < 0.001);
}

#[test]
fn player_bounds_half_size() {
    let e = make_player();
    let b = e.bounds();
    let size = b.size();
    assert!((size.x - 1.0).abs() < 0.001);
    assert!((size.y - 1.8).abs() < 0.001);
    assert!((size.z - 1.0).abs() < 0.001);
}

#[test]
fn projectile_bounds_half_size() {
    let e = make_projectile();
    let b = e.bounds();
    let size = b.size();
    assert!((size.x - 0.5).abs() < 0.001);
    assert!((size.y - 0.5).abs() < 0.001);
    assert!((size.z - 0.5).abs() < 0.001);
}

// ---------- SpatialEntity::position ----------

#[test]
fn ship_position() {
    let e = make_ship();
    let p = e.position();
    assert!((p.x - 10.0).abs() < 0.001);
    assert!((p.y - 20.0).abs() < 0.001);
    assert!((p.z - 30.0).abs() < 0.001);
}

#[test]
fn station_position() {
    let e = make_station();
    let p = e.position();
    assert!((p.x - 5.0).abs() < 0.001);
    assert!((p.y - 6.0).abs() < 0.001);
    assert!((p.z - 7.0).abs() < 0.001);
}

#[test]
fn player_position() {
    let e = make_player();
    let p = e.position();
    assert!((p.x - 1.0).abs() < 0.001);
    assert!((p.y - 2.0).abs() < 0.001);
    assert!((p.z - 3.0).abs() < 0.001);
}

#[test]
fn projectile_position() {
    let e = make_projectile();
    let p = e.position();
    assert!((p.x - 100.0).abs() < 0.001);
    assert!((p.y - 200.0).abs() < 0.001);
    assert!((p.z - 300.0).abs() < 0.001);
}

// ---------- SpatialEntity::entity_type ----------

#[test]
fn ship_entity_type() {
    assert_eq!(make_ship().entity_type(), EntityType::Ship);
}

#[test]
fn station_entity_type() {
    assert_eq!(make_station().entity_type(), EntityType::Station);
}

#[test]
fn player_entity_type() {
    assert_eq!(make_player().entity_type(), EntityType::Player);
}

#[test]
fn projectile_entity_type() {
    assert_eq!(make_projectile().entity_type(), EntityType::Projectile);
}
