use rfs_core::spatial::{SpatialGrid, EntityId, InterestMask, InterestLayer, InterestConfig};
use rfs_core::entity::{ShipEntity, PlayerEntity, StationEntity, ProjectileEntity, CompartmentEntity, SpatialEntity};
use rfs_core::math::Vec3f;
use parking_lot::RwLock;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

pub struct InterestManager {
    config: InterestConfig,
    ship_grid: RwLock<SpatialGrid<ShipEntity>>,
    player_grid: RwLock<SpatialGrid<PlayerEntity>>,
    station_grid: RwLock<SpatialGrid<StationEntity>>,
    projectile_grid: RwLock<SpatialGrid<ProjectileEntity>>,
    compartment_grid: RwLock<SpatialGrid<CompartmentEntity>>,
    player_ships: RwLock<HashMap<EntityId, EntityId>>,
    player_interests: RwLock<HashMap<u32, PlayerInterest>>,
    entity_layers: RwLock<HashMap<EntityId, InterestLayer>>,
}

#[derive(Debug, Clone)]
pub struct PlayerInterest {
    pub player_id: u32,
    pub ship_id: EntityId,
    pub position: Vec3f,
    pub visible_ships: HashSet<EntityId>,
    pub visible_stations: HashSet<EntityId>,
    pub visible_players: HashSet<EntityId>,
    pub visible_projectiles: HashSet<EntityId>,
    pub visible_compartments: HashSet<EntityId>,
    pub last_update: std::time::Instant,
}

impl Default for PlayerInterest {
    fn default() -> Self {
        Self {
            player_id: 0,
            ship_id: EntityId::nil(),
            position: Vec3f::ZERO,
            visible_ships: HashSet::new(),
            visible_stations: HashSet::new(),
            visible_players: HashSet::new(),
            visible_projectiles: HashSet::new(),
            visible_compartments: HashSet::new(),
            last_update: std::time::Instant::now(),
        }
    }
}

impl InterestManager {
    pub fn new(config: InterestConfig) -> Self {
        Self {
            config,
            ship_grid: RwLock::new(SpatialGrid::new()),
            player_grid: RwLock::new(SpatialGrid::new()),
            station_grid: RwLock::new(SpatialGrid::new()),
            projectile_grid: RwLock::new(SpatialGrid::new()),
            compartment_grid: RwLock::new(SpatialGrid::new()),
            player_ships: RwLock::new(HashMap::new()),
            player_interests: RwLock::new(HashMap::new()),
            entity_layers: RwLock::new(HashMap::new()),
        }
    }

    pub fn add_ship(&self, ship: ShipEntity) {
        let entity_id = ship.entity_id();
        self.entity_layers.write().insert(entity_id, InterestLayer::SHIP);
        self.ship_grid.write().insert(ship);
    }

    pub fn remove_ship(&self, entity_id: EntityId) {
        self.ship_grid.write().remove(entity_id);
        self.entity_layers.write().remove(&entity_id);
    }

    pub fn update_ship(&self, ship: ShipEntity) {
        self.ship_grid.write().insert(ship);
    }

    pub fn add_player(&self, player: PlayerEntity, ship_id: EntityId) {
        let entity_id = player.entity_id();
        let player_id = player.player_id;
        let position = player.position();
        self.entity_layers.write().insert(entity_id, InterestLayer::PLAYER);
        self.player_grid.write().insert(player);
        self.player_ships.write().insert(entity_id, ship_id);
        
        let mut interests = self.player_interests.write();
        interests.insert(player_id, PlayerInterest {
            player_id,
            ship_id,
            position,
            visible_ships: HashSet::new(),
            visible_stations: HashSet::new(),
            visible_players: HashSet::new(),
            visible_projectiles: HashSet::new(),
            visible_compartments: HashSet::new(),
            last_update: std::time::Instant::now(),
        });
    }

    pub fn remove_player(&self, player_id: u32, entity_id: EntityId) {
        self.player_grid.write().remove(entity_id);
        self.player_ships.write().remove(&entity_id);
        self.entity_layers.write().remove(&entity_id);
        self.player_interests.write().remove(&player_id);
    }

    pub fn update_player(&self, player: PlayerEntity) {
        let entity_id = player.entity_id();
        let player_id = player.player_id;
        let position = player.position();
        let ship_id = player.current_ship.unwrap_or(EntityId::nil());
        
        self.player_grid.write().insert(player.clone());
        
        if let Some(interests) = self.player_interests.write().get_mut(&player_id) {
            interests.position = position;
            if interests.ship_id != ship_id && !ship_id.is_nil() {
                interests.ship_id = ship_id;
            }
        }
        
        self.player_ships.write().insert(entity_id, ship_id);
    }

    pub fn add_station(&self, station: StationEntity) {
        self.entity_layers.write().insert(station.entity_id(), InterestLayer::PLAYER);
        self.station_grid.write().insert(station);
    }

    pub fn remove_station(&self, entity_id: EntityId) {
        self.station_grid.write().remove(entity_id);
        self.entity_layers.write().remove(&entity_id);
    }

    pub fn update_station(&self, station: StationEntity) {
        self.station_grid.write().insert(station);
    }

    pub fn add_projectile(&self, projectile: ProjectileEntity) {
        self.entity_layers.write().insert(projectile.entity_id(), InterestLayer::PROJECTILES);
        self.projectile_grid.write().insert(projectile);
    }

    pub fn remove_projectile(&self, entity_id: EntityId) {
        self.projectile_grid.write().remove(entity_id);
        self.entity_layers.write().remove(&entity_id);
    }

    pub fn update_projectile(&self, projectile: ProjectileEntity) {
        self.projectile_grid.write().insert(projectile);
    }

    pub fn add_compartment(&self, compartment: CompartmentEntity) {
        self.entity_layers.write().insert(compartment.entity_id(), InterestLayer::COMPARTMENTS);
        self.compartment_grid.write().insert(compartment);
    }

    pub fn remove_compartment(&self, entity_id: EntityId) {
        self.compartment_grid.write().remove(entity_id);
        self.entity_layers.write().remove(&entity_id);
    }

    pub fn update_compartment(&self, compartment: CompartmentEntity) {
        self.compartment_grid.write().insert(compartment);
    }

    pub fn get_ships(&self) -> Vec<ShipEntity> {
        self.ship_grid.read().all_entities()
    }

    pub fn get_players(&self) -> Vec<PlayerEntity> {
        self.player_grid.read().all_entities()
    }

    pub fn get_stations(&self) -> Vec<StationEntity> {
        self.station_grid.read().all_entities()
    }

    pub fn get_compartments(&self) -> Vec<CompartmentEntity> {
        self.compartment_grid.read().all_entities()
    }

    pub fn get_projectiles(&self) -> Vec<ProjectileEntity> {
        self.projectile_grid.read().all_entities()
    }

    pub fn compute_interest(&self, player_id: u32) -> PlayerInterest {
        let mut interest = {
            let interests = self.player_interests.read();
            interests.get(&player_id).cloned().unwrap_or_default()
        };

        let ship_id = interest.ship_id;
        let position = interest.position;

        {
            let ship_grid = self.ship_grid.read();
            let ships = ship_grid.query_radius(position, self.config.ship_radius);
            let mut visible_ships = HashSet::new();
            for ship in ships {
                visible_ships.insert(ship.entity_id());
            }
            interest.visible_ships = visible_ships;
        }

        if !ship_id.is_nil() {
            {
                let station_grid = self.station_grid.read();
                let stations = station_grid.query_radius(position, self.config.player_radius);
                let mut visible_stations = HashSet::new();
                for station in stations {
                    if station.ship_id == ship_id {
                        visible_stations.insert(station.entity_id());
                    }
                }
                interest.visible_stations = visible_stations;
            }

            {
                let player_grid = self.player_grid.read();
                let players = player_grid.query_radius(position, self.config.player_radius);
                let mut visible_players = HashSet::new();
                for player in players {
                    if player.current_ship == Some(ship_id) {
                        visible_players.insert(player.entity_id());
                    }
                }
                interest.visible_players = visible_players;
            }

            {
                let compartment_grid = self.compartment_grid.read();
                let compartments = compartment_grid.query_radius(position, self.config.compartments_radius);
                let mut visible_compartments = HashSet::new();
                for comp in compartments {
                    if comp.ship_id == ship_id {
                        visible_compartments.insert(comp.entity_id());
                    }
                }
                interest.visible_compartments = visible_compartments;
            }
        }

        {
            let projectile_grid = self.projectile_grid.read();
            let projectiles = projectile_grid.query_radius(position, self.config.projectiles_radius);
            let mut visible_projectiles = HashSet::new();
            for proj in projectiles {
                visible_projectiles.insert(proj.entity_id());
            }
            interest.visible_projectiles = visible_projectiles;
        }

        interest.last_update = std::time::Instant::now();
        
        self.player_interests.write().insert(player_id, interest.clone());
        interest
    }

    pub fn get_interest(&self, player_id: u32) -> Option<PlayerInterest> {
        self.player_interests.read().get(&player_id).cloned()
    }

    pub fn should_replicate_entity(&self, viewer_player_id: u32, target_entity_id: EntityId) -> bool {
        let interests = self.player_interests.read();
        if let Some(interest) = interests.get(&viewer_player_id) {
            let layers = self.entity_layers.read();
            if let Some(&layer) = layers.get(&target_entity_id) {
                match layer {
                    InterestLayer::SHIP => interest.visible_ships.contains(&target_entity_id),
                    InterestLayer::COMPARTMENTS => interest.visible_compartments.contains(&target_entity_id),
                    InterestLayer::PLAYER => interest.visible_players.contains(&target_entity_id) || interest.visible_stations.contains(&target_entity_id),
                    InterestLayer::PROJECTILES => interest.visible_projectiles.contains(&target_entity_id),
                    InterestLayer::ALL => true,
                    InterestLayer(_) => false,
                }
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn get_replication_mask(&self, viewer_player_id: u32, target_entity_id: EntityId) -> InterestMask {
        let interests = self.player_interests.read();
        if let Some(interest) = interests.get(&viewer_player_id) {
            let layers = self.entity_layers.read();
            if let Some(&layer) = layers.get(&target_entity_id) {
                let mut mask = InterestMask::empty();
                match layer {
                    InterestLayer::SHIP => {
                        if interest.visible_ships.contains(&target_entity_id) {
                            mask |= InterestMask::SHIP;
                        }
                    }
                    InterestLayer::COMPARTMENTS => {
                        if interest.visible_compartments.contains(&target_entity_id) {
                            mask |= InterestMask::COMPARTMENTS;
                        }
                    }
                    InterestLayer::PLAYER => {
                        if interest.visible_players.contains(&target_entity_id) || interest.visible_stations.contains(&target_entity_id) {
                            mask |= InterestMask::PLAYER;
                        }
                    }
                    InterestLayer::PROJECTILES => {
                        if interest.visible_projectiles.contains(&target_entity_id) {
                            mask |= InterestMask::PROJECTILES;
                        }
                    }
                    InterestLayer::ALL => {
                        mask = InterestMask::ALL;
                    }
                    InterestLayer(_) => {}
                }
                mask
            } else {
                InterestMask::empty()
            }
        } else {
            InterestMask::empty()
        }
    }

    pub fn get_visible_entities(&self, player_id: u32, layer: InterestLayer) -> SmallVec<[EntityId; 32]> {
        let interests = self.player_interests.read();
        if let Some(interest) = interests.get(&player_id) {
            match layer {
                InterestLayer::SHIP => interest.visible_ships.iter().copied().collect(),
                InterestLayer::COMPARTMENTS => interest.visible_compartments.iter().copied().collect(),
                InterestLayer::PLAYER => {
                    let mut result = SmallVec::new();
                    result.extend(interest.visible_players.iter().copied());
                    result.extend(interest.visible_stations.iter().copied());
                    result
                }
                InterestLayer::PROJECTILES => interest.visible_projectiles.iter().copied().collect(),
InterestLayer::ALL => {
                        let mut result = SmallVec::new();
                        result.extend(interest.visible_ships.iter().copied());
                        result.extend(interest.visible_compartments.iter().copied());
                        result.extend(interest.visible_players.iter().copied());
                        result.extend(interest.visible_stations.iter().copied());
                        result.extend(interest.visible_projectiles.iter().copied());
                        result
                    }
                    InterestLayer(_) => SmallVec::new(),
                }
        } else {
            SmallVec::new()
        }
    }

    pub fn update_all_interests(&self) {
        let player_ids: Vec<u32> = self.player_interests.read().keys().copied().collect();
        for player_id in player_ids {
            self.compute_interest(player_id);
        }
    }

    pub fn stats(&self) -> InterestStats {
        let ships = self.ship_grid.read().entity_count();
        let players = self.player_grid.read().entity_count();
        let stations = self.station_grid.read().entity_count();
        let projectiles = self.projectile_grid.read().entity_count();
        let compartments = self.compartment_grid.read().entity_count();
        let tracked_players = self.player_interests.read().len();

        InterestStats {
            ships,
            players,
            stations,
            projectiles,
            compartments,
            tracked_players,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InterestStats {
    pub ships: usize,
    pub players: usize,
    pub stations: usize,
    pub projectiles: usize,
    pub compartments: usize,
    pub tracked_players: usize,
}