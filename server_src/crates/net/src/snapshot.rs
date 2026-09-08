use rfs_core::entity::{ShipEntity, PlayerEntity, StationEntity, CompartmentEntity, ProjectileEntity};
use rfs_core::packet::*;
use rfs_core::math::{Vec3f, Transform};
use rfs_core::time::Tick;
use smallvec::SmallVec;
use std::collections::HashMap;
use parking_lot::RwLock;

pub const MAX_SNAPSHOT_HISTORY: usize = 128;
pub const SNAPSHOT_INTERVAL_TICKS: u32 = 1;

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub tick: Tick,
    pub time: f64,
    pub entities: Vec<EntitySnapshot>,
    pub projectiles: Vec<ProjectileSnapshot>,
    pub events: Vec<GameEvent>,
}

#[derive(Debug, Clone)]
pub struct EntitySnapshot {
    pub entity_id: EntityId,
    pub entity_type: EntityType,
    pub transform: Transform,
    pub velocity: Vec3f,
    pub angular_velocity: Vec3f,
    pub health: f32,
    pub max_health: f32,
    pub flags: EntityFlags,
    pub ship_data: Option<ShipSnapshotData>,
    pub station_data: Option<StationSnapshotData>,
    pub player_data: Option<PlayerSnapshotData>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShipSnapshotData {
    pub ship_class_id: u32,
    pub compartments: Vec<CompartmentSnapshot>,
    pub stations: Vec<EntityId>,
    pub fuel: f32,
    pub max_fuel: f32,
    pub speed: f32,
    pub max_speed: f32,
    pub heading: f32,
    pub rudder_angle: f32,
    pub throttle: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompartmentSnapshot {
    pub compartment_id: EntityId,
    pub water_level: f32,
    pub max_water_level: f32,
    pub is_sealed: bool,
    pub is_breached: bool,
    pub fire_intensity: f32,
    pub connected_compartments: SmallVec<[EntityId; 4]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StationSnapshotData {
    pub station_type: StationType,
    pub occupant: Option<EntityId>,
    pub yaw: f32,
    pub pitch: f32,
    pub reload_progress: f32,
    pub ammo_type: u8,
    pub is_operational: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSnapshotData {
    pub player_id: u32,
    pub name: String,
    pub team: u8,
    pub current_station: Option<EntityId>,
    pub posture: PlayerPosture,
    pub health: f32,
    pub stamina: f32,
}

#[derive(Debug, Clone)]
pub struct ProjectileSnapshot {
    pub entity_id: EntityId,
    pub projectile_type: ProjectileType,
    pub position: Vec3f,
    pub velocity: Vec3f,
    pub spawn_tick: u32,
    pub lifetime: f32,
    pub owner: EntityId,
    pub damage: f32,
    pub penetration: f32,
}

// Replication layers (see plan §3.2/§3.4): each layer is an independent delta
// stream with its own send schedule and its own base snapshot per client.
pub const LAYER_SHIP: u8 = 0;
pub const LAYER_COMPARTMENT: u8 = 1;
pub const LAYER_PLAYER: u8 = 2;
pub const LAYER_PROJECTILE: u8 = 3;
pub const LAYER_COUNT: usize = 4;

/// How often each layer is sent, in 30 Hz ticks. Layer 1 has no fixed rate:
/// it goes out on change plus a periodic full resync (see below).
pub const LAYER_INTERVAL_TICKS: [u64; LAYER_COUNT] = [2, u64::MAX, 1, 1];
/// Force a full layer state this often (repairs state lost on the unreliable
/// channel). Layer 1 resyncs more often per the plan ("full check every few seconds").
pub const LAYER_RESYNC_TICKS: [u64; LAYER_COUNT] = [150, 90, 150, 150];

pub fn entity_layer(entity_type: EntityType) -> u8 {
    match entity_type {
        EntityType::Ship => LAYER_SHIP,
        EntityType::Station | EntityType::Compartment => LAYER_COMPARTMENT,
        EntityType::Player => LAYER_PLAYER,
        EntityType::Projectile => LAYER_PROJECTILE,
    }
}

/// What a client was last sent for one layer: the layer content plus the tick
/// it went out. The next delta for that layer diffs against this content only,
// so layers never interfere with each other.
#[derive(Debug, Clone, Default)]
pub struct LayerBase {
    pub entities: Vec<EntitySnapshot>,
    pub projectiles: Vec<ProjectileSnapshot>,
    pub last_sent_tick: u64,
}

impl LayerBase {
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.projectiles.is_empty()
    }
}

pub struct SnapshotBuffer {
    snapshots: RwLock<Vec<Option<Snapshot>>>,
    current_index: RwLock<usize>,
    base_tick: RwLock<Tick>,
    max_history: usize,
}

impl SnapshotBuffer {
    pub fn new(max_history: usize) -> Self {
        let mut snapshots = Vec::with_capacity(max_history);
        snapshots.resize_with(max_history, || None);
        
        Self {
            snapshots: RwLock::new(snapshots),
            current_index: RwLock::new(0),
            base_tick: RwLock::new(Tick(0)),
            max_history,
        }
    }

    pub fn write_snapshot(&self, snapshot: Snapshot) {
        let mut snapshots = self.snapshots.write();
        let mut base = self.base_tick.write();
        let mut index = self.current_index.write();

        let tick = snapshot.tick;

        if tick < *base {
            return;
        }

        let diff = (tick - *base) as usize;
        if diff >= self.max_history {
            let advance = diff - self.max_history + 1;
            // Clear the slots that will fall out of the window.
            for i in 0..advance.min(self.max_history) {
                let clear_idx = (*index + i) % self.max_history;
                snapshots[clear_idx] = None;
            }
            *base = *base + advance as u64;
            *index = (*index + advance) % self.max_history;
        }

        // Recompute diff against the (possibly) advanced base.
        let diff2 = (tick - *base) as usize;
        let idx = (*index + diff2) % self.max_history;
        snapshots[idx] = Some(snapshot);
    }

    pub fn get_snapshot(&self, tick: Tick) -> Option<Snapshot> {
        let snapshots = self.snapshots.read();
        let base = *self.base_tick.read();
        
        if tick < base {
            return None;
        }
        
        let diff = (tick - base) as usize;
        if diff >= self.max_history {
            return None;
        }
        
        let index = *self.current_index.read();
        let idx = (index + diff) % self.max_history;
        snapshots[idx].clone()
    }

    pub fn get_latest(&self) -> Option<Snapshot> {
        let snapshots = self.snapshots.read();
        // Scan for the snapshot with the greatest tick — `current_index` points at `base_tick`,
        // not at the newest entry, so the old `snapshots[index]` was always stale/empty.
        let mut best: Option<Snapshot> = None;
        let mut best_tick = Tick(0);
        for slot in snapshots.iter().flatten() {
            if best.is_none() || slot.tick > best_tick {
                best_tick = slot.tick;
                best = Some(slot.clone());
            }
        }
        best
    }

    pub fn get_base_tick(&self) -> Tick {
        *self.base_tick.read()
    }

    pub fn get_latest_tick(&self) -> Tick {
        let base = *self.base_tick.read();
        let index = *self.current_index.read();
        let snapshots = self.snapshots.read();
        
        for i in (0..self.max_history).rev() {
            let idx = (index + i) % self.max_history;
            if let Some(snap) = snapshots[idx].as_ref() {
                return snap.tick;
            }
        }
        base
    }
}

#[derive(Debug, Clone)]
pub struct DeltaSnapshot {
    pub layer: u8,
    pub base_tick: Tick,
    pub target_tick: Tick,
    pub target_time: f64,
    pub created: Vec<EntitySnapshot>,
    pub updated: Vec<EntityUpdate>,
    pub destroyed: Vec<EntityId>,
    pub projectile_created: Vec<ProjectileSnapshot>,
    pub projectile_updated: Vec<ProjectileUpdate>,
    pub projectile_destroyed: Vec<EntityId>,
}

impl DeltaSnapshot {
    pub fn is_empty(&self) -> bool {
        self.created.is_empty()
            && self.updated.is_empty()
            && self.destroyed.is_empty()
            && self.projectile_created.is_empty()
            && self.projectile_updated.is_empty()
            && self.projectile_destroyed.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct EntityUpdate {
    pub entity_id: EntityId,
    pub transform: Option<Transform>,
    pub velocity: Option<Vec3f>,
    pub angular_velocity: Option<Vec3f>,
    pub health: Option<f32>,
    pub flags: Option<EntityFlags>,
    pub ship_data: Option<ShipSnapshotData>,
    pub station_data: Option<StationSnapshotData>,
    pub player_data: Option<PlayerSnapshotData>,
}

#[derive(Debug, Clone)]
pub struct ProjectileUpdate {
    pub entity_id: EntityId,
    pub position: Option<Vec3f>,
    pub velocity: Option<Vec3f>,
    pub lifetime: Option<f32>,
}

pub struct DeltaCompressor {
    last_snapshots: RwLock<HashMap<(u32, u8), Tick>>,
}

impl DeltaCompressor {
    pub fn new() -> Self {
        Self {
            last_snapshots: RwLock::new(HashMap::new()),
        }
    }

    pub fn create_delta(
        &self,
        client_id: u32,
        layer: u8,
        base_snapshot: &Snapshot,
        target_snapshot: &Snapshot,
    ) -> DeltaSnapshot {
        let mut last = self.last_snapshots.write();
        let key = (client_id, layer);
        let base_tick = last.get(&key).copied().unwrap_or(base_snapshot.tick);
        
        if target_snapshot.tick <= base_tick {
            *last.entry(key).or_insert(target_snapshot.tick) = target_snapshot.tick;
            return DeltaSnapshot {
                layer,
                base_tick: target_snapshot.tick,
                target_tick: target_snapshot.tick,
                target_time: target_snapshot.time,
                created: Vec::new(),
                updated: Vec::new(),
                destroyed: Vec::new(),
                projectile_created: Vec::new(),
                projectile_updated: Vec::new(),
                projectile_destroyed: Vec::new(),
            };
        }

        let base_entities: HashMap<_, _> = base_snapshot.entities.iter()
            .map(|e| (e.entity_id, e))
            .collect();
        let target_entities: HashMap<_, _> = target_snapshot.entities.iter()
            .map(|e| (e.entity_id, e))
            .collect();

        let mut created = Vec::new();
        let mut updated = Vec::new();
        let mut destroyed = Vec::new();

        for (id, entity) in &target_entities {
            if let Some(base_entity) = base_entities.get(id) {
                let update = self.compute_entity_update(base_entity, entity);
                if update.has_changes() {
                    updated.push(update);
                }
            } else {
                created.push((*entity).clone());
            }
        }

        for (id, _) in &base_entities {
            if !target_entities.contains_key(id) {
                destroyed.push(*id);
            }
        }

        let base_projectiles: HashMap<_, _> = base_snapshot.projectiles.iter()
            .map(|p| (p.entity_id, p))
            .collect();
        let target_projectiles: HashMap<_, _> = target_snapshot.projectiles.iter()
            .map(|p| (p.entity_id, p))
            .collect();

        let mut projectile_created = Vec::new();
        let mut projectile_updated = Vec::new();
        let mut projectile_destroyed = Vec::new();

        for (id, proj) in &target_projectiles {
            if let Some(base_proj) = base_projectiles.get(id) {
                let update = self.compute_projectile_update(base_proj, proj);
                if update.has_changes() {
                    projectile_updated.push(update);
                }
            } else {
                projectile_created.push((*proj).clone());
            }
        }

        for (id, _) in &base_projectiles {
            if !target_projectiles.contains_key(id) {
                projectile_destroyed.push(*id);
            }
        }

        *last.entry(key).or_insert(target_snapshot.tick) = target_snapshot.tick;

        DeltaSnapshot {
            layer,
            base_tick,
            target_tick: target_snapshot.tick,
            target_time: target_snapshot.time,
            created,
            updated,
            destroyed,
            projectile_created,
            projectile_updated,
            projectile_destroyed,
        }
    }

    fn compute_entity_update(&self, base: &EntitySnapshot, target: &EntitySnapshot) -> EntityUpdate {
        let mut update = EntityUpdate {
            entity_id: base.entity_id,
            transform: None,
            velocity: None,
            angular_velocity: None,
            health: None,
            flags: None,
            ship_data: None,
            station_data: None,
            player_data: None,
        };

        if base.transform.position.distance(target.transform.position) > 0.01 ||
           (base.transform.rotation.x - target.transform.rotation.x).abs() > 0.001 ||
           (base.transform.rotation.y - target.transform.rotation.y).abs() > 0.001 ||
           (base.transform.rotation.z - target.transform.rotation.z).abs() > 0.001 ||
           (base.transform.rotation.w - target.transform.rotation.w).abs() > 0.001 {
            update.transform = Some(target.transform);
        }

        if (base.velocity - target.velocity).length_squared() > 0.0001 {
            update.velocity = Some(target.velocity);
        }

        if (base.angular_velocity - target.angular_velocity).length_squared() > 0.0001 {
            update.angular_velocity = Some(target.angular_velocity);
        }

        if (base.health - target.health).abs() > 0.01 {
            update.health = Some(target.health);
        }

        if base.flags != target.flags {
            update.flags = Some(target.flags);
        }

        if base.ship_data != target.ship_data {
            update.ship_data = target.ship_data.clone();
        }

        if base.station_data != target.station_data {
            update.station_data = target.station_data.clone();
        }

        if base.player_data != target.player_data {
            update.player_data = target.player_data.clone();
        }

        update
    }

    fn compute_projectile_update(&self, base: &ProjectileSnapshot, target: &ProjectileSnapshot) -> ProjectileUpdate {
        let mut update = ProjectileUpdate {
            entity_id: base.entity_id,
            position: None,
            velocity: None,
            lifetime: None,
        };

        if (base.position - target.position).length_squared() > 0.01 {
            update.position = Some(target.position);
        }

        if (base.velocity - target.velocity).length_squared() > 0.001 {
            update.velocity = Some(target.velocity);
        }

        if (base.lifetime - target.lifetime).abs() > 0.01 {
            update.lifetime = Some(target.lifetime);
        }

        update
    }

    pub fn remove_client(&self, client_id: u32) {
        self.last_snapshots.write().retain(|(id, _), _| *id != client_id);
    }
}

impl EntityUpdate {
    pub fn has_changes(&self) -> bool {
        self.transform.is_some() ||
        self.velocity.is_some() ||
        self.angular_velocity.is_some() ||
        self.health.is_some() ||
        self.flags.is_some() ||
        self.ship_data.is_some() ||
        self.station_data.is_some() ||
        self.player_data.is_some()
    }
}

impl ProjectileUpdate {
    pub fn has_changes(&self) -> bool {
        self.position.is_some() ||
        self.velocity.is_some() ||
        self.lifetime.is_some()
    }
}

pub struct SnapshotInterpolator {
    snapshots: Vec<Snapshot>,
    max_snapshots: usize,
}

impl SnapshotInterpolator {
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            snapshots: Vec::with_capacity(max_snapshots),
            max_snapshots,
        }
    }

    pub fn add_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshots.push(snapshot);
        if self.snapshots.len() > self.max_snapshots {
            self.snapshots.remove(0);
        }
    }

    pub fn interpolate(&self, target_time: f64) -> Option<Snapshot> {
        if self.snapshots.len() < 2 {
            return self.snapshots.last().cloned();
        }

        let mut before = None;
        let mut after = None;

        for snap in &self.snapshots {
            if snap.time <= target_time {
                before = Some(snap);
            } else {
                after = Some(snap);
                break;
            }
        }

        match (before, after) {
            (Some(b), Some(a)) => {
                let alpha = ((target_time - b.time) / (a.time - b.time)) as f32;
                Some(self.interpolate_snapshots(b, a, alpha))
            }
            (Some(b), None) => Some(b.clone()),
            (None, Some(a)) => Some(a.clone()),
            _ => None,
        }
    }

    fn interpolate_snapshots(&self, before: &Snapshot, after: &Snapshot, alpha: f32) -> Snapshot {
        let mut result = before.clone();
        result.time = before.time + (after.time - before.time) * alpha as f64;

        let before_entities: HashMap<_, _> = before.entities.iter().map(|e| (e.entity_id, e)).collect();
        let after_entities: HashMap<_, _> = after.entities.iter().map(|e| (e.entity_id, e)).collect();

        result.entities = before_entities.iter()
            .filter_map(|(id, b_entity)| {
                if let Some(a_entity) = after_entities.get(id) {
                    Some(self.interpolate_entity(b_entity, a_entity, alpha))
                } else {
                    Some((*b_entity).clone())
                }
            })
            .collect();

        let before_projs: HashMap<_, _> = before.projectiles.iter().map(|p| (p.entity_id, p)).collect();
        let after_projs: HashMap<_, _> = after.projectiles.iter().map(|p| (p.entity_id, p)).collect();

        result.projectiles = before_projs.iter()
            .filter_map(|(id, b_proj)| {
                if let Some(a_proj) = after_projs.get(id) {
                    Some(self.interpolate_projectile(b_proj, a_proj, alpha))
                } else {
                    Some((*b_proj).clone())
                }
            })
            .collect();

        result
    }

    fn interpolate_entity(&self, before: &EntitySnapshot, after: &EntitySnapshot, alpha: f32) -> EntitySnapshot {
        EntitySnapshot {
            entity_id: before.entity_id,
            entity_type: before.entity_type,
            transform: Transform {
                position: before.transform.position.lerp(after.transform.position, alpha),
                rotation: before.transform.rotation.slerp(after.transform.rotation, alpha),
                scale: before.transform.scale.lerp(after.transform.scale, alpha),
            },
            velocity: before.velocity.lerp(after.velocity, alpha),
            angular_velocity: before.angular_velocity.lerp(after.angular_velocity, alpha),
            health: before.health + (after.health - before.health) * alpha,
            max_health: after.max_health,
            flags: after.flags,
            ship_data: after.ship_data.clone(),
            station_data: after.station_data.clone(),
            player_data: after.player_data.clone(),
        }
    }

    fn interpolate_projectile(&self, before: &ProjectileSnapshot, after: &ProjectileSnapshot, alpha: f32) -> ProjectileSnapshot {
        ProjectileSnapshot {
            entity_id: before.entity_id,
            projectile_type: after.projectile_type,
            position: before.position.lerp(after.position, alpha),
            velocity: before.velocity.lerp(after.velocity, alpha),
            spawn_tick: after.spawn_tick,
            lifetime: before.lifetime + (after.lifetime - before.lifetime) * alpha,
            owner: after.owner,
            damage: after.damage,
            penetration: after.penetration,
        }
    }
}

impl From<&ShipEntity> for EntitySnapshot {
    fn from(ship: &ShipEntity) -> Self {
        EntitySnapshot {
            entity_id: ship.entity_id,
            entity_type: EntityType::Ship,
            transform: ship.transform,
            velocity: ship.velocity,
            angular_velocity: ship.angular_velocity,
            health: ship.health,
            max_health: ship.max_health,
            flags: ship.flags,
            ship_data: Some(ShipSnapshotData {
                ship_class_id: ship.ship_class_id,
                compartments: ship.compartments.iter().map(|c| CompartmentSnapshot {
                    compartment_id: c.entity_id,
                    water_level: c.water_level,
                    max_water_level: c.max_water_level,
                    is_sealed: c.is_sealed,
                    is_breached: c.is_breached,
                    fire_intensity: c.fire_intensity,
                    connected_compartments: c.connected_compartments.clone(),
                }).collect(),
                stations: ship.stations.iter().map(|s| s.entity_id).collect(),
                fuel: ship.fuel,
                max_fuel: ship.max_fuel,
                speed: ship.speed,
                max_speed: ship.max_speed,
                heading: ship.heading,
                rudder_angle: ship.rudder_angle,
                throttle: ship.throttle,
            }),
            station_data: None,
            player_data: None,
        }
    }
}

impl From<&PlayerEntity> for EntitySnapshot {
    fn from(player: &PlayerEntity) -> Self {
        EntitySnapshot {
            entity_id: player.entity_id,
            entity_type: EntityType::Player,
            transform: player.transform,
            velocity: player.velocity,
            angular_velocity: Vec3f::ZERO,
            health: player.health,
            max_health: player.max_health,
            flags: EntityFlags::NONE,
            ship_data: None,
            station_data: None,
            player_data: Some(PlayerSnapshotData {
                player_id: player.player_id,
                name: player.name.clone(),
                team: player.team,
                current_station: player.current_station,
                posture: player.posture,
                health: player.health,
                stamina: player.stamina,
            }),
        }
    }
}

impl From<&StationEntity> for EntitySnapshot {
    fn from(station: &StationEntity) -> Self {
        EntitySnapshot {
            entity_id: station.entity_id,
            entity_type: EntityType::Station,
            transform: station.world_transform,
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: station.health,
            max_health: station.max_health,
            flags: EntityFlags::NONE,
            ship_data: None,
            station_data: Some(StationSnapshotData {
                station_type: station.station_type,
                occupant: station.occupant,
                yaw: station.yaw,
                pitch: station.pitch,
                reload_progress: station.reload_progress,
                ammo_type: station.ammo_type,
                is_operational: station.is_operational,
            }),
            player_data: None,
        }
    }
}

impl From<&CompartmentEntity> for EntitySnapshot {
    fn from(compartment: &CompartmentEntity) -> Self {
        EntitySnapshot {
            entity_id: compartment.entity_id,
            entity_type: EntityType::Compartment,
            transform: Transform::from_position(compartment.world_bounds.center()),
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: 100.0,
            max_health: 100.0,
            flags: EntityFlags::NONE,
            ship_data: None,
            station_data: None,
            player_data: None,
        }
    }
}

impl From<&ProjectileEntity> for ProjectileSnapshot {
    fn from(projectile: &ProjectileEntity) -> Self {
        ProjectileSnapshot {
            entity_id: projectile.entity_id,
            projectile_type: projectile.projectile_type,
            position: projectile.position,
            velocity: projectile.velocity,
            spawn_tick: projectile.spawn_tick,
            lifetime: projectile.lifetime,
            owner: projectile.owner,
            damage: projectile.damage,
            penetration: projectile.penetration,
        }
    }
}

impl From<&EntityState> for EntitySnapshot {
    fn from(s: &EntityState) -> Self {
        EntitySnapshot {
            entity_id: s.entity_id,
            entity_type: s.entity_type,
            transform: s.transform,
            velocity: s.velocity,
            angular_velocity: s.angular_velocity,
            health: s.health,
            max_health: s.max_health,
            flags: s.flags,
            ship_data: s.ship_data.as_ref().map(ShipSnapshotData::from),
            station_data: s.station_data.as_ref().map(StationSnapshotData::from),
            player_data: s.player_data.as_ref().map(PlayerSnapshotData::from),
        }
    }
}

impl From<&ShipStateData> for ShipSnapshotData {
    fn from(s: &ShipStateData) -> Self {
        ShipSnapshotData {
            ship_class_id: s.ship_class_id,
            compartments: s.compartments.iter().map(CompartmentSnapshot::from).collect(),
            stations: s.stations.clone(),
            fuel: s.fuel,
            max_fuel: s.max_fuel,
            speed: s.speed,
            max_speed: s.max_speed,
            heading: s.heading,
            rudder_angle: s.rudder_angle,
            throttle: s.throttle,
        }
    }
}

impl From<&CompartmentState> for CompartmentSnapshot {
    fn from(s: &CompartmentState) -> Self {
        CompartmentSnapshot {
            compartment_id: s.compartment_id,
            water_level: s.water_level,
            max_water_level: s.max_water_level,
            is_sealed: s.is_sealed,
            is_breached: s.is_breached,
            fire_intensity: s.fire_intensity,
            connected_compartments: s.connected_compartments.clone(),
        }
    }
}

impl From<&StationStateData> for StationSnapshotData {
    fn from(s: &StationStateData) -> Self {
        StationSnapshotData {
            station_type: s.station_type,
            occupant: s.occupant,
            yaw: s.yaw,
            pitch: s.pitch,
            reload_progress: s.reload_progress,
            ammo_type: s.ammo_type,
            is_operational: s.is_operational,
        }
    }
}

impl From<&PlayerStateData> for PlayerSnapshotData {
    fn from(s: &PlayerStateData) -> Self {
        PlayerSnapshotData {
            player_id: s.player_id,
            name: s.name.clone(),
            team: s.team,
            current_station: s.current_station,
            posture: s.posture,
            health: s.health,
            stamina: s.stamina,
        }
    }
}

impl From<&ProjectileState> for ProjectileSnapshot {
    fn from(s: &ProjectileState) -> Self {
        ProjectileSnapshot {
            entity_id: s.entity_id,
            projectile_type: s.projectile_type,
            position: s.position,
            velocity: s.velocity,
            spawn_tick: s.spawn_tick,
            lifetime: s.lifetime,
            owner: s.owner,
            damage: s.damage,
            penetration: s.penetration,
        }
    }
}

impl From<&StatePacket> for Snapshot {
    fn from(s: &StatePacket) -> Self {
        Snapshot {
            tick: Tick(s.server_tick as u64),
            time: s.server_time,
            entities: s.entities.iter().map(EntitySnapshot::from).collect(),
            projectiles: s.projectiles.iter().map(ProjectileSnapshot::from).collect(),
            events: Vec::new(),
        }
    }
}

impl Snapshot {
    pub fn apply_delta(&self, delta: &StateDeltaPacket) -> Snapshot {
        // Layered deltas carry only their own layer, so `created` is an upsert:
        // the entity may already be present from an earlier full state or resync.
        let mut entities = self.entities.clone();
        for created in &delta.created {
            let snapshot = EntitySnapshot::from(created);
            if let Some(slot) = entities.iter_mut().find(|e| e.entity_id == snapshot.entity_id) {
                *slot = snapshot;
            } else {
                entities.push(snapshot);
            }
        }
        for update in &delta.updated {
            if let Some(entity) = entities.iter_mut().find(|e| e.entity_id == update.entity_id) {
                entity.apply_state_update(update);
            }
        }
        entities.retain(|e| !delta.destroyed.contains(&e.entity_id));

        let mut projectiles = self.projectiles.clone();
        for created in &delta.projectile_created {
            let snapshot = ProjectileSnapshot::from(created);
            if let Some(slot) = projectiles.iter_mut().find(|p| p.entity_id == snapshot.entity_id) {
                *slot = snapshot;
            } else {
                projectiles.push(snapshot);
            }
        }
        for update in &delta.projectile_updated {
            if let Some(proj) = projectiles.iter_mut().find(|p| p.entity_id == update.entity_id) {
                proj.apply_state_update(update);
            }
        }
        projectiles.retain(|p| !delta.projectile_destroyed.contains(&p.entity_id));

        Snapshot {
            tick: Tick(delta.server_tick as u64),
            time: delta.server_time,
            entities,
            projectiles,
            events: Vec::new(),
        }
    }

    pub fn to_state_packet(&self) -> StatePacket {
        StatePacket {
            server_tick: self.tick.value() as u32,
            server_time: self.time,
            entities: self.entities.iter().map(EntitySnapshot::to_state).collect(),
            projectiles: self.projectiles.iter().map(ProjectileSnapshot::to_state).collect(),
        }
    }
}

impl ShipSnapshotData {
    pub fn to_state(&self) -> ShipStateData {
        ShipStateData {
            ship_class_id: self.ship_class_id,
            compartments: self.compartments.iter().map(CompartmentSnapshot::to_state).collect(),
            stations: self.stations.clone(),
            fuel: self.fuel,
            max_fuel: self.max_fuel,
            speed: self.speed,
            max_speed: self.max_speed,
            heading: self.heading,
            rudder_angle: self.rudder_angle,
            throttle: self.throttle,
        }
    }
}

impl CompartmentSnapshot {
    pub fn to_state(&self) -> CompartmentState {
        CompartmentState {
            compartment_id: self.compartment_id,
            water_level: self.water_level,
            max_water_level: self.max_water_level,
            is_sealed: self.is_sealed,
            is_breached: self.is_breached,
            fire_intensity: self.fire_intensity,
            connected_compartments: self.connected_compartments.clone(),
        }
    }
}

impl StationSnapshotData {
    pub fn to_state(&self) -> StationStateData {
        StationStateData {
            station_type: self.station_type,
            occupant: self.occupant,
            yaw: self.yaw,
            pitch: self.pitch,
            reload_progress: self.reload_progress,
            ammo_type: self.ammo_type,
            is_operational: self.is_operational,
        }
    }
}

impl PlayerSnapshotData {
    pub fn to_state(&self) -> PlayerStateData {
        PlayerStateData {
            player_id: self.player_id,
            name: self.name.clone(),
            team: self.team,
            current_station: self.current_station,
            posture: self.posture,
            health: self.health,
            stamina: self.stamina,
        }
    }
}

impl ProjectileSnapshot {
    pub fn to_state(&self) -> ProjectileState {
        ProjectileState {
            entity_id: self.entity_id,
            projectile_type: self.projectile_type,
            position: self.position,
            velocity: self.velocity,
            spawn_tick: self.spawn_tick,
            lifetime: self.lifetime,
            owner: self.owner,
            damage: self.damage,
            penetration: self.penetration,
        }
    }

    pub fn apply_state_update(&mut self, update: &ProjectileStateUpdate) {
        self.position = update.position;
        self.velocity = update.velocity;
        self.lifetime = update.lifetime;
    }
}

impl EntitySnapshot {
    pub fn to_state(&self) -> EntityState {
        EntityState {
            entity_id: self.entity_id,
            entity_type: self.entity_type,
            transform: self.transform,
            velocity: self.velocity,
            angular_velocity: self.angular_velocity,
            health: self.health,
            max_health: self.max_health,
            flags: self.flags,
            ship_data: self.ship_data.as_ref().map(ShipSnapshotData::to_state),
            station_data: self.station_data.as_ref().map(StationSnapshotData::to_state),
            player_data: self.player_data.as_ref().map(PlayerSnapshotData::to_state),
        }
    }

    pub fn apply_state_update(&mut self, update: &EntityStateUpdate) {
        if let Some(t) = update.transform {
            self.transform = t;
        }
        if let Some(v) = update.velocity {
            self.velocity = v;
        }
        if let Some(av) = update.angular_velocity {
            self.angular_velocity = av;
        }
        if let Some(h) = update.health {
            self.health = h;
        }
        if let Some(f) = update.flags {
            self.flags = f;
        }
        if let Some(sd) = &update.ship_data {
            self.ship_data = Some(ShipSnapshotData::from(sd));
        }
        if let Some(sd) = &update.station_data {
            self.station_data = Some(StationSnapshotData::from(sd));
        }
        if let Some(pd) = &update.player_data {
            self.player_data = Some(PlayerSnapshotData::from(pd));
        }
    }
}

impl EntityUpdate {
    pub fn to_state_update(&self) -> EntityStateUpdate {
        EntityStateUpdate {
            entity_id: self.entity_id,
            transform: self.transform,
            velocity: self.velocity,
            angular_velocity: self.angular_velocity,
            health: self.health,
            flags: self.flags,
            ship_data: self.ship_data.as_ref().map(ShipSnapshotData::to_state),
            station_data: self.station_data.as_ref().map(StationSnapshotData::to_state),
            player_data: self.player_data.as_ref().map(PlayerSnapshotData::to_state),
        }
    }
}

impl ProjectileUpdate {
    pub fn to_state_update(&self) -> ProjectileStateUpdate {
        ProjectileStateUpdate {
            entity_id: self.entity_id,
            position: self.position.unwrap_or_default(),
            velocity: self.velocity.unwrap_or_default(),
            lifetime: self.lifetime.unwrap_or_default(),
        }
    }
}

impl DeltaSnapshot {
    pub fn to_state_delta(&self) -> StateDeltaPacket {
        StateDeltaPacket {
            layer: self.layer,
            base_tick: self.base_tick.value() as u32,
            server_tick: self.target_tick.value() as u32,
            server_time: self.target_time,
            created: self.created.iter().map(EntitySnapshot::to_state).collect(),
            updated: self.updated.iter().map(EntityUpdate::to_state_update).collect(),
            destroyed: self.destroyed.clone(),
            projectile_created: self.projectile_created.iter().map(ProjectileSnapshot::to_state).collect(),
            projectile_updated: self.projectile_updated.iter().map(ProjectileUpdate::to_state_update).collect(),
            projectile_destroyed: self.projectile_destroyed.clone(),
        }
    }
}