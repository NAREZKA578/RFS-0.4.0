use rfs_core::entity::{ShipClass, CompartmentTemplate, BulkheadTemplate, StationTemplate, StationType, EntityId, Bounds, Vec3f, Transform};
use rfs_core::math::Quatf;
use crate::ship::{Ship, ShipConfig};
use rfs_damage::damage::DamageSystem;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

pub struct ShipLoader {
    ship_classes: HashMap<u32, ShipClass>,
    data_path: String,
}

impl ShipLoader {
    pub fn new(data_path: &str) -> Self {
        Self {
            ship_classes: HashMap::new(),
            data_path: data_path.to_string(),
        }
    }

    pub fn load_all(&mut self) -> Result<(), ShipLoadError> {
        let path = Path::new(&self.data_path);
        if !path.exists() {
            return Err(ShipLoadError::PathNotFound(self.data_path.clone()));
        }

        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                self.load_ship_class(&path)?;
            }
        }

        Ok(())
    }

    pub fn load_ship_class(&mut self, path: &Path) -> Result<(), ShipLoadError> {
        let content = fs::read_to_string(path)?;
        let class: ShipClass = serde_json::from_str(&content)?;
        
        self.ship_classes.insert(class.class_id, class);
        Ok(())
    }

    pub fn get_class(&self, class_id: u32) -> Option<&ShipClass> {
        self.ship_classes.get(&class_id)
    }

    pub fn create_ship(&self, class_id: u32, entity_id: EntityId, damage_system: Arc<DamageSystem>) -> Result<Ship, ShipLoadError> {
        let class = self.get_class(class_id)
            .ok_or(ShipLoadError::ClassNotFound(class_id))?;

        let config: ShipConfig = class.clone().into();
        // Ship::new already links compartments, bulkheads and stations by name
        // (single source of truth, bug №52/№53/№54), so nothing to patch here.
        Ok(Ship::new(config, entity_id, damage_system.clone()))
    }

    pub fn list_classes(&self) -> Vec<&ShipClass> {
        self.ship_classes.values().collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ShipLoadError {
    #[error("Data path not found: {0}")]
    PathNotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Ship class not found: {0}")]
    ClassNotFound(u32),
}

pub fn create_default_frigate() -> ShipClass {
    ShipClass {
        class_id: 1,
        name: "Frigate".to_string(),
        description: "Light warship, fast and maneuverable".to_string(),
        hull_model: "frigate.glb".to_string(),
        displacement: 1500.0,
        max_speed: 30.0,
        acceleration: 0.5,
        turn_rate: 1.5,
        max_health: 10000.0,
        max_fuel: 5000.0,
        armor_thickness: 50.0,
        compartments: vec![
            CompartmentTemplate {
                name: "forecastle".to_string(),
                local_bounds: Bounds::new(
                    Vec3f::new(-10.0, 0.0, 30.0),
                    Vec3f::new(10.0, 5.0, 50.0),
                ),
                max_water_level: 100.0,
                pump_capacity: 10.0,
                connected_compartments: vec!["forward_magazine".to_string(), "crew_quarters".to_string()],
                bulkheads: vec![
                    BulkheadTemplate {
                        name: "forecastle_bulkhead".to_string(),
                        local_position: Vec3f::new(0.0, 2.5, 25.0),
                        connects_to: "forward_magazine".to_string(),
                        seal_strength: 500.0,
                    },
                ],
                stations: vec!["anchor_winch".to_string()],
            },
            CompartmentTemplate {
                name: "forward_magazine".to_string(),
                local_bounds: Bounds::new(
                    Vec3f::new(-8.0, -2.0, 10.0),
                    Vec3f::new(8.0, 2.0, 30.0),
                ),
                max_water_level: 80.0,
                pump_capacity: 15.0,
                connected_compartments: vec!["forecastle".to_string(), "crew_quarters".to_string(), "engine_room".to_string()],
                bulkheads: vec![
                    BulkheadTemplate {
                        name: "magazine_fore".to_string(),
                        local_position: Vec3f::new(0.0, 0.0, 25.0),
                        connects_to: "forecastle".to_string(),
                        seal_strength: 800.0,
                    },
                    BulkheadTemplate {
                        name: "magazine_aft".to_string(),
                        local_position: Vec3f::new(0.0, 0.0, 5.0),
                        connects_to: "crew_quarters".to_string(),
                        seal_strength: 800.0,
                    },
                ],
                stations: vec!["gun_1_ammo".to_string(), "gun_2_ammo".to_string()],
            },
            CompartmentTemplate {
                name: "crew_quarters".to_string(),
                local_bounds: Bounds::new(
                    Vec3f::new(-12.0, 0.0, -10.0),
                    Vec3f::new(12.0, 6.0, 10.0),
                ),
                max_water_level: 200.0,
                pump_capacity: 20.0,
                connected_compartments: vec!["forward_magazine".to_string(), "engine_room".to_string(), "wardroom".to_string()],
                bulkheads: vec![
                    BulkheadTemplate {
                        name: "quarters_fore".to_string(),
                        local_position: Vec3f::new(0.0, 3.0, 5.0),
                        connects_to: "forward_magazine".to_string(),
                        seal_strength: 400.0,
                    },
                    BulkheadTemplate {
                        name: "quarters_aft".to_string(),
                        local_position: Vec3f::new(0.0, 3.0, -5.0),
                        connects_to: "engine_room".to_string(),
                        seal_strength: 400.0,
                    },
                ],
                stations: vec!["helm".to_string(), "nav_station".to_string()],
            },
            CompartmentTemplate {
                name: "engine_room".to_string(),
                local_bounds: Bounds::new(
                    Vec3f::new(-10.0, -5.0, -30.0),
                    Vec3f::new(10.0, 3.0, -10.0),
                ),
                max_water_level: 300.0,
                pump_capacity: 50.0,
                connected_compartments: vec!["crew_quarters".to_string(), "aft_magazine".to_string(), "steering_gear".to_string()],
                bulkheads: vec![
                    BulkheadTemplate {
                        name: "engine_fore".to_string(),
                        local_position: Vec3f::new(0.0, -1.0, -5.0),
                        connects_to: "crew_quarters".to_string(),
                        seal_strength: 600.0,
                    },
                    BulkheadTemplate {
                        name: "engine_aft".to_string(),
                        local_position: Vec3f::new(0.0, -1.0, -25.0),
                        connects_to: "aft_magazine".to_string(),
                        seal_strength: 600.0,
                    },
                ],
                stations: vec!["engine_control".to_string(), "throttle".to_string()],
            },
            CompartmentTemplate {
                name: "aft_magazine".to_string(),
                local_bounds: Bounds::new(
                    Vec3f::new(-8.0, -2.0, -50.0),
                    Vec3f::new(8.0, 2.0, -30.0),
                ),
                max_water_level: 80.0,
                pump_capacity: 15.0,
                connected_compartments: vec!["engine_room".to_string(), "steering_gear".to_string()],
                bulkheads: vec![
                    BulkheadTemplate {
                        name: "magazine_fore".to_string(),
                        local_position: Vec3f::new(0.0, 0.0, -25.0),
                        connects_to: "engine_room".to_string(),
                        seal_strength: 800.0,
                    },
                ],
                stations: vec!["gun_3_ammo".to_string(), "gun_4_ammo".to_string()],
            },
            CompartmentTemplate {
                name: "steering_gear".to_string(),
                local_bounds: Bounds::new(
                    Vec3f::new(-6.0, -3.0, -70.0),
                    Vec3f::new(6.0, 2.0, -50.0),
                ),
                max_water_level: 50.0,
                pump_capacity: 10.0,
                connected_compartments: vec!["engine_room".to_string(), "aft_magazine".to_string()],
                bulkheads: vec![
                    BulkheadTemplate {
                        name: "steering_fore".to_string(),
                        local_position: Vec3f::new(0.0, -0.5, -50.0),
                        connects_to: "engine_room".to_string(),
                        seal_strength: 300.0,
                    },
                ],
                stations: vec!["emergency_tiller".to_string()],
            },
            CompartmentTemplate {
                name: "wardroom".to_string(),
                local_bounds: Bounds::new(
                    Vec3f::new(-8.0, 2.0, -10.0),
                    Vec3f::new(8.0, 8.0, 10.0),
                ),
                max_water_level: 50.0,
                pump_capacity: 5.0,
                connected_compartments: vec!["crew_quarters".to_string()],
                bulkheads: vec![
                    BulkheadTemplate {
                        name: "wardroom_bulkhead".to_string(),
                        local_position: Vec3f::new(0.0, 5.0, -10.0),
                        connects_to: "crew_quarters".to_string(),
                        seal_strength: 200.0,
                    },
                ],
                stations: vec!["captain_chair".to_string(), "comms".to_string(), "radar".to_string()],
            },
        ],
        stations: vec![
            StationTemplate {
                name: "helm".to_string(),
                station_type: StationType::Helm,
                compartment_name: "crew_quarters".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(0.0, 1.5, 0.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 200.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "nav_station".to_string(),
                station_type: StationType::Radar,
                compartment_name: "crew_quarters".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(-3.0, 1.5, 2.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 150.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "engine_control".to_string(),
                station_type: StationType::Engine,
                compartment_name: "engine_room".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(0.0, 0.0, -20.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 300.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "throttle".to_string(),
                station_type: StationType::Engine,
                compartment_name: "engine_room".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(2.0, 0.0, -20.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 100.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "gun_1".to_string(),
                station_type: StationType::Gun,
                compartment_name: "forecastle".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(-6.0, 3.0, 40.0),
                    Quatf::from_euler(0.0, 0.0, 0.0),
                    Vec3f::ONE,
                ),
                max_ammo: 50,
                default_ammo_type: 0,
                cooldown: 10.0,
                max_health: 500.0,
                yaw_range: (-1.5, 1.5),
                pitch_range: (-0.5, 0.8),
            },
            StationTemplate {
                name: "gun_2".to_string(),
                station_type: StationType::Gun,
                compartment_name: "forecastle".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(6.0, 3.0, 40.0),
                    Quatf::from_euler(0.0, 0.0, 0.0),
                    Vec3f::ONE,
                ),
                max_ammo: 50,
                default_ammo_type: 0,
                cooldown: 10.0,
                max_health: 500.0,
                yaw_range: (-1.5, 1.5),
                pitch_range: (-0.5, 0.8),
            },
            StationTemplate {
                name: "gun_3".to_string(),
                station_type: StationType::Gun,
                compartment_name: "forecastle".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(-6.0, 3.0, -40.0),
                    Quatf::from_euler(0.0, 0.0, 0.0),
                    Vec3f::ONE,
                ),
                max_ammo: 50,
                default_ammo_type: 0,
                cooldown: 10.0,
                max_health: 500.0,
                yaw_range: (-1.5, 1.5),
                pitch_range: (-0.5, 0.8),
            },
            StationTemplate {
                name: "gun_4".to_string(),
                station_type: StationType::Gun,
                compartment_name: "forecastle".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(6.0, 3.0, -40.0),
                    Quatf::from_euler(0.0, 0.0, 0.0),
                    Vec3f::ONE,
                ),
                max_ammo: 50,
                default_ammo_type: 0,
                cooldown: 10.0,
                max_health: 500.0,
                yaw_range: (-1.5, 1.5),
                pitch_range: (-0.5, 0.8),
            },
            StationTemplate {
                name: "pump_1".to_string(),
                station_type: StationType::Pump,
                compartment_name: "engine_room".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(-4.0, 0.0, -15.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 200.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "pump_2".to_string(),
                station_type: StationType::Pump,
                compartment_name: "engine_room".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(4.0, 0.0, -15.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 200.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "damage_control".to_string(),
                station_type: StationType::DamageControl,
                compartment_name: "crew_quarters".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(4.0, 1.5, 0.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 100.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "captain_chair".to_string(),
                station_type: StationType::Captain,
                compartment_name: "wardroom".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(0.0, 1.5, 0.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 100.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "comms".to_string(),
                station_type: StationType::Comms,
                compartment_name: "wardroom".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(-3.0, 1.5, 2.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 100.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "radar".to_string(),
                station_type: StationType::Radar,
                compartment_name: "wardroom".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(3.0, 1.5, 2.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 150.0,
                yaw_range: (0.0, 0.0),
                pitch_range: (0.0, 0.0),
            },
            StationTemplate {
                name: "emergency_tiller".to_string(),
                station_type: StationType::Helm,
                compartment_name: "steering_gear".to_string(),
                local_transform: Transform::new(
                    Vec3f::new(0.0, 0.0, -60.0),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                ),
                max_ammo: 0,
                default_ammo_type: 0,
                cooldown: 0.0,
                max_health: 150.0,
                yaw_range: (-0.5, 0.5),
                pitch_range: (0.0, 0.0),
            },
        ],
        crew_min: 15,
        crew_max: 40,
    }
}

pub fn create_default_destroyer() -> ShipClass {
    let mut frigate = create_default_frigate();
    frigate.class_id = 2;
    frigate.name = "Destroyer".to_string();
    frigate.description = "Medium warship, balanced firepower and speed".to_string();
    frigate.hull_model = "destroyer.glb".to_string();
    frigate.displacement = 3500.0;
    frigate.max_speed = 35.0;
    frigate.acceleration = 0.4;
    frigate.turn_rate = 1.2;
    frigate.max_health = 25000.0;
    frigate.max_fuel = 10000.0;
    frigate.armor_thickness = 100.0;
    frigate.crew_min = 30;
    frigate.crew_max = 80;
    
    frigate
}