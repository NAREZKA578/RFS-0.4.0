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
        // Bug №174: `current_ship` is an `Option`, so a player who left their
        // ship reports `None` — reading only the `Some(..)` case meant the
        // stored `ship_id` kept pointing at the ship they just left, and they
        // went on seeing that ship's compartments, stations and crew forever.
        let ship_id = player.current_ship.unwrap_or(EntityId::nil());
        
        self.player_grid.write().insert(player.clone());
        
        if let Some(interests) = self.player_interests.write().get_mut(&player_id) {
            interests.position = position;
            // Assign unconditionally, including the nil case: leaving a ship
            // must clear the interest, not preserve it.
            interests.ship_id = ship_id;
        }
        
        self.player_ships.write().insert(entity_id, ship_id);
    }

    pub fn add_station(&self, station: StationEntity) {
        // Bug №174: stations were filed under `InterestLayer::PLAYER`, so
        // `should_replicate_entity` only ever matched them through
        // `visible_players` — and `compute_interest` populates `visible_players`
        // from `current_ship == ship_id` while filling `visible_stations` from
        // `station.ship_id == ship_id`. A station was therefore replicated only
        // by accident, whenever a player happened to be a member of that ship,
        // and `get_replication_mask` reported the wrong layer to the rest of
        // the stack. Stations have their own layer.
        self.entity_layers.write().insert(station.entity_id(), InterestLayer::COMPARTMENTS);
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

    /// Recompute a player's visible sets in place, without cloning.
    ///
    /// This is the per-tick hot path (`update_all_interests` calls it for every
    /// player every tick). The previous body cloned the whole `PlayerInterest`
    /// out of the map, built five fresh `HashSet`s, then cloned the whole
    /// structure again on write-back — two full clones plus five allocations per
    /// player per tick. At 200 players that is thousands of HashSet
    /// allocations per tick, which is the measured `sim+sync` cost during a
    /// connect flood. This version reads only the small scalars up front, does
    /// the grid queries lock-free, and then clears+refills the stored sets
    /// under a single write lock. Same result, no clones.
    fn recompute_interest_in_place(&self, player_id: u32) {
        use smallvec::SmallVec;
        type Ids = SmallVec<[EntityId; 24]>;

        // Read only the scalars we need. An unknown id must not be created
        // (Bug №19: untracked ids otherwise pollute the map forever).
        let (ship_id, position) = {
            let interests = self.player_interests.read();
            match interests.get(&player_id) {
                Some(i) => (i.ship_id, i.position),
                None => return,
            }
        };

        let mut visible_ships: Ids = SmallVec::new();
        {
            let ship_grid = self.ship_grid.read();
            for ship in ship_grid.query_radius(position, self.config.ship_radius) {
                visible_ships.push(ship.entity_id());
            }
        }

        let mut visible_stations: Ids = SmallVec::new();
        let mut visible_players: Ids = SmallVec::new();
        let mut visible_compartments: Ids = SmallVec::new();
        if !ship_id.is_nil() {
            {
                let station_grid = self.station_grid.read();
                for station in station_grid.query_radius(position, self.config.player_radius) {
                    if station.ship_id == ship_id {
                        visible_stations.push(station.entity_id());
                    }
                }
            }
            {
                let player_grid = self.player_grid.read();
                for player in player_grid.query_radius(position, self.config.player_radius) {
                    if player.current_ship == Some(ship_id) {
                        visible_players.push(player.entity_id());
                    }
                }
            }
            {
                let compartment_grid = self.compartment_grid.read();
                for comp in compartment_grid.query_radius(position, self.config.compartments_radius) {
                    if comp.ship_id == ship_id {
                        visible_compartments.push(comp.entity_id());
                    }
                }
            }
        }

        let mut visible_projectiles: Ids = SmallVec::new();
        {
            let projectile_grid = self.projectile_grid.read();
            for proj in projectile_grid.query_radius(position, self.config.projectiles_radius) {
                visible_projectiles.push(proj.entity_id());
            }
        }

        // One write lock; clear+refill the existing sets in place.
        let mut interests = self.player_interests.write();
        if let Some(interest) = interests.get_mut(&player_id) {
            interest.visible_ships.clear();
            interest.visible_ships.extend(visible_ships);
            interest.visible_stations.clear();
            interest.visible_stations.extend(visible_stations);
            interest.visible_players.clear();
            interest.visible_players.extend(visible_players);
            interest.visible_projectiles.clear();
            interest.visible_projectiles.extend(visible_projectiles);
            interest.visible_compartments.clear();
            interest.visible_compartments.extend(visible_compartments);
            interest.last_update = std::time::Instant::now();
        }
    }

    pub fn compute_interest(&self, player_id: u32) -> PlayerInterest {
        // In-place recompute, then clone only the final structure for the
        // caller (tests and one-off callers). The hot path uses
        // `recompute_interest_in_place` directly and pays no clone.
        self.recompute_interest_in_place(player_id);
        self.player_interests
            .read()
            .get(&player_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_interest(&self, player_id: u32) -> Option<PlayerInterest> {
        self.player_interests.read().get(&player_id).cloned()
    }

    /// Is this player tracked yet?
    ///
    /// Bug №164: the server sent a connection's first full state before the
    /// game layer had registered the player, and `should_replicate_entity`
    /// answers `false` for an unknown viewer. The caller uses this to hold the
    /// first full back until replication can actually produce content.
    pub fn has_player(&self, player_id: u32) -> bool {
        self.player_interests.read().contains_key(&player_id)
    }

    pub fn should_replicate_entity(&self, viewer_player_id: u32, target_entity_id: EntityId) -> bool {
        let interests = self.player_interests.read();
        if let Some(interest) = interests.get(&viewer_player_id) {
            let layers = self.entity_layers.read();
            if let Some(&layer) = layers.get(&target_entity_id) {
                match layer {
                    InterestLayer::SHIP => interest.visible_ships.contains(&target_entity_id),
                    // Bug №174: stations are filed under COMPARTMENTS, and
                    // `compute_interest` fills `visible_stations` from
                    // `station.ship_id == ship_id`. Check both sets, otherwise
                    // stations never replicate.
                    InterestLayer::COMPARTMENTS => {
                        interest.visible_compartments.contains(&target_entity_id)
                            || interest.visible_stations.contains(&target_entity_id)
                    }
                    InterestLayer::PLAYER => interest.visible_players.contains(&target_entity_id),
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
                        // Bug №174: stations share this layer.
                        if interest.visible_compartments.contains(&target_entity_id)
                            || interest.visible_stations.contains(&target_entity_id)
                        {
                            mask |= InterestMask::COMPARTMENTS;
                        }
                    }
                    InterestLayer::PLAYER => {
                        // Bug №174: players only — stations moved out of this
                        // layer, so do not leak them into the player mask.
                        if interest.visible_players.contains(&target_entity_id) {
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
                // Bug №174: compartments and stations share this layer.
                InterestLayer::COMPARTMENTS => {
                    let mut result = SmallVec::new();
                    result.extend(interest.visible_compartments.iter().copied());
                    result.extend(interest.visible_stations.iter().copied());
                    result
                }
                InterestLayer::PLAYER => {
                    // Bug №174: stations moved to COMPARTMENTS.
                    let mut result = SmallVec::new();
                    result.extend(interest.visible_players.iter().copied());
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

    /// Per-tick refresh for every tracked player.
    ///
    /// Uses the in-place recompute so the hot path allocates nothing per
    /// player (see `recompute_interest_in_place`). The previous version called
    /// `compute_interest`, which cloned the whole `PlayerInterest` twice per
    /// player per tick; at 200 players that dominated the tick budget during a
    /// connect flood.
    pub fn update_all_interests(&self) {
        let player_ids: Vec<u32> = self.player_interests.read().keys().copied().collect();
        for player_id in player_ids {
            self.recompute_interest_in_place(player_id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use rfs_core::entity::{PlayerPosture, StationType};
    use rfs_core::math::Transform;
    use rfs_core::packet::EntityFlags;

    const SHIP_ID: u64 = 10;
    const VIEWER: u32 = 1;

    fn make_ship() -> ShipEntity {
        ShipEntity {
            entity_id: EntityId::new(SHIP_ID),
            ship_class_id: 0,
            transform: Transform::IDENTITY,
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: 100.0,
            max_health: 100.0,
            flags: EntityFlags::empty(),
            fuel: 50.0,
            max_fuel: 50.0,
            speed: 0.0,
            max_speed: 10.0,
            heading: 0.0,
            rudder_angle: 0.0,
            throttle: 0.0,
            // Bug №272: layer-0 hit mark; irrelevant to interest filtering.
            last_hit: None,
            compartments: Vec::new(),
            stations: Vec::new(),
            team: 0,
        }
    }

    fn make_player() -> PlayerEntity {
        PlayerEntity {
            entity_id: EntityId::new(20),
            player_id: VIEWER,
            name: "viewer".to_string(),
            team: 0,
            transform: Transform::IDENTITY,
            velocity: Vec3f::ZERO,
            posture: PlayerPosture::Standing,
            health: 100.0,
            max_health: 100.0,
            stamina: 100.0,
            max_stamina: 100.0,
            current_station: None,
            current_ship: Some(EntityId::new(SHIP_ID)),
            input_sequence: 0,
            last_acknowledged_tick: 0,
        }
    }

    /// Bug №164, root cause: an unregistered viewer replicates nothing, so a
    /// full state sent at that moment is empty. The server now holds the first
    /// full back until `has_player` is true, and refreshes interest first.
    #[test]
    fn an_unregistered_viewer_replicates_nothing() {
        let mgr = InterestManager::new(InterestConfig::default());
        mgr.add_ship(make_ship());

        assert!(
            !mgr.has_player(VIEWER),
            "a player must not be tracked before the game layer registers it"
        );
        assert!(
            !mgr.should_replicate_entity(VIEWER, EntityId::new(SHIP_ID)),
            "this is exactly why the first full used to be empty"
        );
    }

    /// The other half of the fix: registration alone is not enough. The visible
    /// sets are only filled by `compute_interest`, which is why the server
    /// calls it before building the first full.
    #[test]
    fn registration_alone_does_not_make_a_ship_visible() {
        let mgr = InterestManager::new(InterestConfig::default());
        mgr.add_ship(make_ship());
        mgr.add_player(make_player(), EntityId::new(SHIP_ID));

        assert!(mgr.has_player(VIEWER), "now the player is tracked");
        assert!(
            !mgr.should_replicate_entity(VIEWER, EntityId::new(SHIP_ID)),
            "visible sets are still empty before compute_interest"
        );

        mgr.compute_interest(VIEWER);

        assert!(
            mgr.should_replicate_entity(VIEWER, EntityId::new(SHIP_ID)),
            "after compute_interest the first full can carry real content"
        );
    }

    #[test]
    fn a_full_built_the_way_the_server_builds_it_is_not_empty() {
        // Mirrors the ordering in `send_snapshots`: gate on has_player, refresh
        // interest, then filter. Before the fix the gate was missing.
        let mgr = InterestManager::new(InterestConfig::default());
        mgr.add_ship(make_ship());

        assert!(!mgr.has_player(VIEWER), "gate holds the first full back");
        mgr.add_player(make_player(), EntityId::new(SHIP_ID));
        assert!(mgr.has_player(VIEWER), "gate opens");
        mgr.compute_interest(VIEWER);

        let visible: Vec<EntityId> = mgr
            .get_visible_entities(VIEWER, InterestLayer::SHIP)
            .into_iter()
            .collect();
        assert_eq!(visible, vec![EntityId::new(SHIP_ID)], "the first full must not be empty");
    }

    fn make_station(id: u64) -> StationEntity {
        StationEntity {
            entity_id: EntityId::new(id),
            ship_id: EntityId::new(SHIP_ID),
            compartment_id: EntityId::nil(),
            station_type: StationType::Gun,
            local_transform: Transform::IDENTITY,
            world_transform: Transform::IDENTITY,
            occupant: None,
            yaw: 0.0,
            pitch: 0.0,
            reload_progress: 0.0,
            ammo_type: 0,
            ammo_count: 0,
            max_ammo: 0,
            is_operational: true,
            health: 100.0,
            max_health: 100.0,
            cooldown: 0.0,
            max_cooldown: 0.0,
        }
    }

    /// Bug №174: a player who left their ship kept the old `ship_id`, so they
    /// went on seeing that ship's interior forever.
    #[test]
    fn leaving_a_ship_clears_the_interest() {
        let mgr = InterestManager::new(InterestConfig::default());
        mgr.add_ship(make_ship());
        mgr.add_player(make_player(), EntityId::new(SHIP_ID));
        mgr.compute_interest(VIEWER);
        assert_eq!(mgr.get_interest(VIEWER).unwrap().ship_id, EntityId::new(SHIP_ID));

        // Player boards nothing: `current_ship` is None.
        let mut detached = make_player();
        detached.current_ship = None;
        mgr.update_player(detached);
        mgr.compute_interest(VIEWER);

        assert_eq!(
            mgr.get_interest(VIEWER).unwrap().ship_id,
            EntityId::nil(),
            "leaving the ship must clear ship_id, not preserve it"
        );
    }

    /// Switching to a different ship must move the interest, not be ignored.
    #[test]
    fn switching_ships_moves_the_interest() {
        let mgr = InterestManager::new(InterestConfig::default());
        let other = EntityId::new(20);
        mgr.add_player(make_player(), EntityId::new(SHIP_ID));

        let mut moved = make_player();
        moved.current_ship = Some(other);
        mgr.update_player(moved);

        assert_eq!(mgr.get_interest(VIEWER).unwrap().ship_id, other);
    }

    /// Bug №174: stations were filed under the PLAYER layer, so they replicated
    /// only when some player happened to be a member of the ship.
    #[test]
    fn stations_replicate_to_their_own_ship() {
        let mgr = InterestManager::new(InterestConfig::default());
        let station_id = 500;
        mgr.add_ship(make_ship());
        mgr.add_station(make_station(station_id));
        mgr.add_player(make_player(), EntityId::new(SHIP_ID));
        mgr.compute_interest(VIEWER);

        assert!(
            mgr.should_replicate_entity(VIEWER, EntityId::new(station_id)),
            "a station on the viewer's ship must replicate"
        );

        let compartments =
            mgr.get_visible_entities(VIEWER, InterestLayer::COMPARTMENTS).into_iter().collect::<Vec<_>>();
        assert!(
            compartments.contains(&EntityId::new(station_id)),
            "stations belong to the COMPARTMENTS layer, got {compartments:?}"
        );
    }

    /// A player with no ship must not see that ship's stations.
    #[test]
    fn stations_do_not_leak_to_a_shipless_player() {
        let mgr = InterestManager::new(InterestConfig::default());
        let station_id = 501;
        mgr.add_station(make_station(station_id));

        let mut detached = make_player();
        detached.current_ship = None;
        mgr.add_player(detached, EntityId::nil());
        mgr.compute_interest(VIEWER);

        assert!(
            !mgr.should_replicate_entity(VIEWER, EntityId::new(station_id)),
            "a player without a ship must not receive its stations"
        );
    }
}