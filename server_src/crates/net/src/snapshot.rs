use parking_lot::RwLock;
use rfs_core::entity::{
    CompartmentEntity, HitMark, PlayerEntity, ProjectileEntity, ShipEntity, StationEntity,
};
use rfs_core::math::{Transform, Vec3f};
use rfs_core::packet::*;
use rfs_core::time::Tick;
use smallvec::SmallVec;
use std::collections::HashMap;
use tracing::warn;

pub const MAX_SNAPSHOT_HISTORY: usize = 128;
pub const SNAPSHOT_INTERVAL_TICKS: u32 = 1;
/// Bug №24: per-delta budget. `created` beats `updated` beats projectiles;
/// `destroyed` is never cut (it is tiny and dropping it resurrects ghosts).
/// The layer base still advances to the full content, so anything cut here
/// converges on the next change or periodic resync.
pub const MAX_DELTA_CREATED: usize = 512;
pub const MAX_DELTA_UPDATED: usize = 512;
pub const MAX_DELTA_PROJECTILES: usize = 512;
/// Bug №20: hard caps so a corrupt/malicious snapshot cannot balloon client
/// memory through the interpolator. Normal snapshots are hundreds of entries.
pub const MAX_ENTITIES_PER_SNAPSHOT: usize = 4096;
pub const MAX_PROJECTILES_PER_SNAPSHOT: usize = 2048;

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub tick: Tick,
    pub time: f64,
    pub entities: Vec<EntitySnapshot>,
    pub projectiles: Vec<ProjectileSnapshot>,
    pub events: Vec<GameEvent>,
}

#[derive(Debug, Clone, PartialEq)]
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
    /// Bug №272: the most recent hit this ship took, so the hit effect does
    /// not depend on the `ShipHit` EVENT arriving.
    ///
    /// This exists because variant A of bug №272 makes events unreliable, and
    /// the impact normal plus the struck compartment were the ONLY parts of an
    /// event that no layer carried. Everything else an event reports — health,
    /// flooding, occupancy, projectile position — was already in a layer, so
    /// the loss was cosmetic. This makes the last one recoverable too.
    ///
    /// Deliberately "most recent", not "this tick": two shells can strike in
    /// one tick, and a layer delta is built by diffing two snapshots, so a
    /// per-tick hit list would have to accumulate a list to stay correct.
    /// Carrying one entry per ship keeps the layer a true diffable state and
    /// still guarantees the effect has something to draw.
    pub last_hit: Option<HitMark>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompartmentSnapshot {
    pub compartment_id: EntityId,
    pub water_level: f32,
    pub max_water_level: f32,
    pub is_sealed: bool,
    pub is_breached: bool,
    pub fire_intensity: f32,
    /// Bug №61: mirrors `CompartmentState.pump_active` instead of dropping it.
    pub pump_active: bool,
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
    // Bug №61: the station's full live suite makes it onto the wire now.
    pub ammo_count: u32,
    pub max_ammo: u32,
    pub health: f32,
    pub max_health: f32,
    pub cooldown: f32,
    pub max_cooldown: f32,
    pub is_operational: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSnapshotData {
    pub player_id: u32,
    pub name: String,
    pub team: u8,
    pub current_station: Option<EntityId>,
    /// Bug №274: which ship this player is on.
    ///
    /// The player's position is derived from the ship's transform
    /// (`local_to_world`), so without this a client cannot tell which ship it
    /// is riding — it has to guess by matching station lists. A bot that wants
    /// to occupy a gun or aim at an enemy needs it directly, and so does any
    /// real client drawing a HUD.
    pub current_ship: Option<EntityId>,
    pub posture: PlayerPosture,
    pub health: f32,
    pub stamina: f32,
}

#[derive(Debug, Clone, PartialEq)]
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
pub const LAYER_RESYNC_TICKS: [u64; LAYER_COUNT] = [15, 10, 15, 15];

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
    // Bug №21: this used to be three separate locks (snapshots/base/index)
    // taken in different orders by readers and writers — a lock-inversion
    // deadlock. One lock, one order, no cycle possible.
    inner: RwLock<SnapshotBufferInner>,
    max_history: usize,
}

struct SnapshotBufferInner {
    snapshots: Vec<Option<Snapshot>>,
    current_index: usize,
    base_tick: Tick,
}

impl SnapshotBuffer {
    pub fn new(max_history: usize) -> Self {
        // Bug №20: a zero window would `% 0` panic on every write/get. Clamp
        // to at least one slot so no valid call site can divide by zero.
        let max_history = max_history.max(1);
        let mut snapshots = Vec::with_capacity(max_history);
        snapshots.resize_with(max_history, || None);

        Self {
            inner: RwLock::new(SnapshotBufferInner {
                snapshots,
                current_index: 0,
                base_tick: Tick(0),
            }),
            max_history,
        }
    }

    pub fn write_snapshot(&self, snapshot: Snapshot) {
        let mut inner = self.inner.write();

        let tick = snapshot.tick;

        if tick < inner.base_tick {
            return;
        }

        let diff = (tick - inner.base_tick) as usize;
        if diff >= self.max_history {
            let advance = diff - self.max_history + 1;
            // Clear the slots that will fall out of the window.
            for i in 0..advance.min(self.max_history) {
                let clear_idx = (inner.current_index + i) % self.max_history;
                inner.snapshots[clear_idx] = None;
            }
            inner.base_tick = inner.base_tick + advance as u64;
            inner.current_index = (inner.current_index + advance) % self.max_history;
        }

        // Recompute diff against the (possibly) advanced base.
        let diff2 = (tick - inner.base_tick) as usize;
        let idx = (inner.current_index + diff2) % self.max_history;
        inner.snapshots[idx] = Some(snapshot);
    }

    /// Bug №636: a restarted server is a new session with a fresh tick space.
    /// The old session's frames would keep the stale-guard alive forever (every
    /// low tick from the new session loses to the high ticks of the old one),
    /// so a new `match_id` must be able to empty this buffer.
    pub fn reset(&self) {
        let mut inner = self.inner.write();
        for slot in inner.snapshots.iter_mut() {
            *slot = None;
        }
        inner.current_index = 0;
        inner.base_tick = Tick(0);
    }

    pub fn get_snapshot(&self, tick: Tick) -> Option<Snapshot> {
        let inner = self.inner.read();

        if tick < inner.base_tick {
            return None;
        }

        let diff = (tick - inner.base_tick) as usize;
        if diff >= self.max_history {
            return None;
        }

        let idx = (inner.current_index + diff) % self.max_history;
        inner.snapshots[idx].clone()
    }

    pub fn get_latest(&self) -> Option<Snapshot> {
        let inner = self.inner.read();
        // Scan for the snapshot with the greatest tick — `current_index` points at `base_tick`,
        // not at the newest entry, so the old `snapshots[index]` was always stale/empty.
        let mut best: Option<Snapshot> = None;
        let mut best_tick = Tick(0);
        for slot in inner.snapshots.iter().flatten() {
            if best.is_none() || slot.tick > best_tick {
                best_tick = slot.tick;
                best = Some(slot.clone());
            }
        }
        best
    }

    pub fn get_base_tick(&self) -> Tick {
        self.inner.read().base_tick
    }

    pub fn get_latest_tick(&self) -> Tick {
        let inner = self.inner.read();

        for i in (0..self.max_history).rev() {
            let idx = (inner.current_index + i) % self.max_history;
            if let Some(snap) = inner.snapshots[idx].as_ref() {
                return snap.tick;
            }
        }
        inner.base_tick
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

impl Default for DeltaCompressor {
    fn default() -> Self {
        Self::new()
    }
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

        // Bug №22/№78: the old code advanced the ring on this early return
        // even though nothing is sent (and the client applies nothing). That
        // raced the ring ahead of the server's LayerBase.last_sent_tick, which
        // only moves on real sends — the next non-empty delta declared a base
        // tick the client had never applied and was dropped, freezing the
        // layer forever. Keep the ring in lockstep: no send, no advance.
        if target_snapshot.tick <= base_tick {
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

        let base_entities: HashMap<_, _> = base_snapshot
            .entities
            .iter()
            .map(|e| (e.entity_id, e))
            .collect();
        let target_entities: HashMap<_, _> = target_snapshot
            .entities
            .iter()
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

        for id in base_entities.keys() {
            if !target_entities.contains_key(id) {
                destroyed.push(*id);
            }
        }

        let base_projectiles: HashMap<_, _> = base_snapshot
            .projectiles
            .iter()
            .map(|p| (p.entity_id, p))
            .collect();
        let target_projectiles: HashMap<_, _> = target_snapshot
            .projectiles
            .iter()
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

        for id in base_projectiles.keys() {
            if !target_projectiles.contains_key(id) {
                projectile_destroyed.push(*id);
            }
        }

        // Bug №24: bound the delta even if the world explodes in one tick.
        //
        // Bug №271: bounding is only safe as long as the client's base is NOT
        // advanced past what it received. Everything cut here would otherwise
        // be dropped on the floor for good: the delta still goes out, the base
        // still moves to `target_snapshot.tick`, and the entities behind the cut
        // are then diffed against a base the client never saw — which either
        // stalls the layer on a base mismatch (№267) or applies on top of wrong
        // state. There is no client-side trace of the loss: the warn below is
        // the only record. So a truncated delta is not sent at all, and the
        // next tick re-diffs from the same base and carries the full content.
        let mut truncated = false;
        if created.len() > MAX_DELTA_CREATED {
            warn!(
                "create_delta: {} created exceeds {}; withholding the delta for this tick",
                created.len(),
                MAX_DELTA_CREATED
            );
            truncated = true;
        }
        if updated.len() > MAX_DELTA_UPDATED {
            warn!(
                "create_delta: {} updated exceeds {}; withholding the delta for this tick",
                updated.len(),
                MAX_DELTA_UPDATED
            );
            truncated = true;
        }
        if projectile_created.len() + projectile_updated.len() > MAX_DELTA_PROJECTILES {
            warn!("create_delta: {} projectile changes exceed {}; withholding the delta for this tick", projectile_created.len() + projectile_updated.len(), MAX_DELTA_PROJECTILES);
            truncated = true;
        }
        if truncated {
            return Self::withheld_delta(layer, base_tick, target_snapshot);
        }

        let is_empty = created.is_empty()
            && updated.is_empty()
            && destroyed.is_empty()
            && projectile_created.is_empty()
            && projectile_updated.is_empty()
            && projectile_destroyed.is_empty();

        // Bug №22/№78: an empty delta is never sent, so the client base does
        // not advance. Advancing the ring here used to desync it from the
        // server's LayerBase and freeze replication for every client.
        if is_empty {
            return DeltaSnapshot {
                layer,
                base_tick,
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

    /// An empty delta that is deliberately not sent, leaving the client's base
    /// where it is.
    ///
    /// Bug №271: the caller sends nothing and the ring is not advanced, so the
    /// next tick diffs the same base again and the withheld content goes out in
    /// full. `base_tick` is the client's real base — using the target tick here
    /// (as the early return in `create_delta` does) would declare a base the
    /// client never applied, which is the same class of error this exists to
    /// avoid. It is never observed on the wire: the send path drops empty
    /// deltas, so the value exists only to be unambiguous if that changes.
    fn withheld_delta(layer: u8, base_tick: Tick, target: &Snapshot) -> DeltaSnapshot {
        DeltaSnapshot {
            layer,
            base_tick,
            target_tick: target.tick,
            target_time: target.time,
            created: Vec::new(),
            updated: Vec::new(),
            destroyed: Vec::new(),
            projectile_created: Vec::new(),
            projectile_updated: Vec::new(),
            projectile_destroyed: Vec::new(),
        }
    }

    /// Move a client's per-layer base tick forward without building a delta.
    ///
    /// The caller is about to send a delta that was serialized for a different
    /// client (identical layer content, identical base tick, identical target
    /// tick — that is exactly what the payload cache key guarantees), so the
    /// diff itself is not needed, but this client's base tick still has to move
    /// to the tick that was just sent.
    ///
    /// Skipping this is what the payload cache used to do, and it silently broke
    /// replication: the ring stayed behind while the server's own
    /// `LayerBase.last_sent_tick` moved on, so the next real diff declared a
    /// base tick the client had already applied and the client rejected it as a
    /// base mismatch — for the ship layer, whose content is identical for every
    /// client, that is almost every client, every tick.
    pub fn advance_base(&self, client_id: u32, layer: u8, target_tick: u64) {
        self.last_snapshots
            .write()
            .insert((client_id, layer), Tick(target_tick));
    }

    fn compute_entity_update(
        &self,
        base: &EntitySnapshot,
        target: &EntitySnapshot,
    ) -> EntityUpdate {
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

        if base.transform.position.distance(target.transform.position) > 0.01
            || (base.transform.rotation.x - target.transform.rotation.x).abs() > 0.001
            || (base.transform.rotation.y - target.transform.rotation.y).abs() > 0.001
            || (base.transform.rotation.z - target.transform.rotation.z).abs() > 0.001
            || (base.transform.rotation.w - target.transform.rotation.w).abs() > 0.001
        {
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

    fn compute_projectile_update(
        &self,
        base: &ProjectileSnapshot,
        target: &ProjectileSnapshot,
    ) -> ProjectileUpdate {
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
        self.last_snapshots
            .write()
            .retain(|(id, _), _| *id != client_id);
    }
}

impl EntityUpdate {
    pub fn has_changes(&self) -> bool {
        self.transform.is_some()
            || self.velocity.is_some()
            || self.angular_velocity.is_some()
            || self.health.is_some()
            || self.flags.is_some()
            || self.ship_data.is_some()
            || self.station_data.is_some()
            || self.player_data.is_some()
    }
}

impl ProjectileUpdate {
    pub fn has_changes(&self) -> bool {
        self.position.is_some() || self.velocity.is_some() || self.lifetime.is_some()
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

    /// Bug №636: a restarted server is a new session — old-session frames must
    /// not feed an interpolation between a dead world and the new one, so a
    /// new `match_id` clears the frame ring the same way the buffer resets
    /// (`SnapshotBuffer::reset`, Bug №636).
    pub fn reset(&mut self) {
        self.snapshots.clear();
    }

    pub fn add_snapshot(&mut self, mut snapshot: Snapshot) {
        // Bug №20: truncate oversized snapshots instead of buffering them whole.
        if snapshot.entities.len() > MAX_ENTITIES_PER_SNAPSHOT {
            warn!(
                "add_snapshot: truncating {} entities to {}",
                snapshot.entities.len(),
                MAX_ENTITIES_PER_SNAPSHOT
            );
            snapshot.entities.truncate(MAX_ENTITIES_PER_SNAPSHOT);
        }
        if snapshot.projectiles.len() > MAX_PROJECTILES_PER_SNAPSHOT {
            warn!(
                "add_snapshot: truncating {} projectiles to {}",
                snapshot.projectiles.len(),
                MAX_PROJECTILES_PER_SNAPSHOT
            );
            snapshot.projectiles.truncate(MAX_PROJECTILES_PER_SNAPSHOT);
        }
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
                // Bug №22: unclamped alpha extrapolated wildly past the
                // interval on unordered/duplicate timestamps.
                let alpha = if a.time > b.time {
                    ((target_time - b.time) / (a.time - b.time)).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
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

        let before_entities: HashMap<_, _> =
            before.entities.iter().map(|e| (e.entity_id, e)).collect();
        let after_entities: HashMap<_, _> =
            after.entities.iter().map(|e| (e.entity_id, e)).collect();

        result.entities = before_entities
            .iter()
            .map(|(id, b_entity)| match after_entities.get(id) {
                Some(a_entity) => self.interpolate_entity(b_entity, a_entity, alpha),
                None => (*b_entity).clone(),
            })
            .chain(
                // Bug №22: entities created in `after` used to be invisible
                // until the NEXT snapshot. Take them as-is.
                after_entities
                    .iter()
                    .filter(|(id, _)| !before_entities.contains_key(id))
                    .map(|(_, a_entity)| (*a_entity).clone()),
            )
            .collect();

        let before_projs: HashMap<_, _> = before
            .projectiles
            .iter()
            .map(|p| (p.entity_id, p))
            .collect();
        let after_projs: HashMap<_, _> =
            after.projectiles.iter().map(|p| (p.entity_id, p)).collect();

        result.projectiles = before_projs
            .iter()
            .map(|(id, b_proj)| match after_projs.get(id) {
                Some(a_proj) => self.interpolate_projectile(b_proj, a_proj, alpha),
                None => (*b_proj).clone(),
            })
            .chain(
                after_projs
                    .iter()
                    .filter(|(id, _)| !before_projs.contains_key(id))
                    .map(|(_, a_proj)| (*a_proj).clone()),
            )
            .collect();

        result
    }

    fn finite_rotation(q: rfs_core::math::Quatf) -> bool {
        (q.x + q.y + q.z + q.w).is_finite()
    }

    fn interpolate_entity(
        &self,
        before: &EntitySnapshot,
        after: &EntitySnapshot,
        alpha: f32,
    ) -> EntitySnapshot {
        // Bug №22: slerp with a non-finite endpoint yields NaN rotations
        // that poison everything downstream. Fall back to the finite side.
        let rotation = if Self::finite_rotation(before.transform.rotation)
            && Self::finite_rotation(after.transform.rotation)
        {
            before
                .transform
                .rotation
                .slerp(after.transform.rotation, alpha)
        } else if Self::finite_rotation(before.transform.rotation) {
            before.transform.rotation
        } else {
            after.transform.rotation
        };
        EntitySnapshot {
            entity_id: before.entity_id,
            entity_type: before.entity_type,
            transform: Transform {
                position: before
                    .transform
                    .position
                    .lerp(after.transform.position, alpha),
                rotation,
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

    fn interpolate_projectile(
        &self,
        before: &ProjectileSnapshot,
        after: &ProjectileSnapshot,
        alpha: f32,
    ) -> ProjectileSnapshot {
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
                compartments: ship
                    .compartments
                    .iter()
                    .map(|c| CompartmentSnapshot {
                        compartment_id: c.entity_id,
                        water_level: c.water_level,
                        max_water_level: c.max_water_level,
                        is_sealed: c.is_sealed,
                        is_breached: c.is_breached,
                        fire_intensity: c.fire_intensity,
                        pump_active: c.pump_active,
                        connected_compartments: c.connected_compartments.clone(),
                    })
                    .collect(),
                stations: ship.stations.iter().map(|s| s.entity_id).collect(),
                fuel: ship.fuel,
                max_fuel: ship.max_fuel,
                speed: ship.speed,
                max_speed: ship.max_speed,
                heading: ship.heading,
                rudder_angle: ship.rudder_angle,
                throttle: ship.throttle,
                last_hit: ship.last_hit,
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
                // Bug №274: the ship this player rides, so a client can tell
                // which ship it is on without reverse-engineering station lists.
                current_ship: player.current_ship,
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
                // Bug №61: carry the full station state the entity exposes.
                ammo_count: station.ammo_count,
                max_ammo: station.max_ammo,
                health: station.health,
                max_health: station.max_health,
                cooldown: station.cooldown,
                max_cooldown: station.max_cooldown,
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
            // Bug №606: a constant `100.0` claimed every compartment was at
            // full health no matter what layer 0 said, so layer 1 contradicted
            // the truth it is supposed to mirror. Compartments have no HP
            // model anywhere (no field in `CompartmentTemplate`, none in
            // `CompartmentState`), and the wire type carries a mandatory
            // `f32`, so the honest mapping is structural: intact = full,
            // breached = destroyed. Flood and fire stay layer 0's job —
            // this field no longer asserts anything those two contradict.
            health: if compartment.is_breached { 0.0 } else { 100.0 },
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
            compartments: s
                .compartments
                .iter()
                .map(CompartmentSnapshot::from)
                .collect(),
            stations: s.stations.clone(),
            fuel: s.fuel,
            max_fuel: s.max_fuel,
            speed: s.speed,
            max_speed: s.max_speed,
            heading: s.heading,
            rudder_angle: s.rudder_angle,
            throttle: s.throttle,
            // Bug §272: the wire type carries the hit, so it MUST survive the
            // conversion in both directions. Dropping it here broke the
            // `updated` path: `apply_state_update` rebuilds `ShipSnapshotData`
            // through this `From`, so a hit arriving in an update was silently
            // erased before the client could see it — which is the exact loss
            // the field exists to prevent. The regression test that caught it
            // is `a_client_recovers_the_hit_from_the_layer_alone`.
            last_hit: s.last_hit,
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
            pump_active: s.pump_active,
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
            ammo_count: s.ammo_count,
            max_ammo: s.max_ammo,
            health: s.health,
            max_health: s.max_health,
            cooldown: s.cooldown,
            max_cooldown: s.max_cooldown,
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
            // Bug №274: symmetric with `From<&PlayerEntity>`.
            current_ship: s.current_ship,
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
    /// Bug №637: this used to take `&self` and deep-copy both vectors
    /// (`iter().cloned().collect()`) into the result, on top of the caller
    /// already owning a base copy — four full entity/projectile copies per
    /// applied delta, each frame, layered. Taking `self` by value moves the
    /// vectors for free; the caller's base is a private clone or a boot
    /// snapshot, so nothing outside the call referenced it.
    pub fn apply_delta(self, delta: &StateDeltaPacket) -> Snapshot {
        // Layered deltas carry only their own layer, so `created` is an upsert:
        // the entity may already be present from an earlier full state or resync.
        let mut entities = self.entities;
        // Bug №637: the four `find` scans looked the whole vector up per
        // created/updated record — O(records × entities) each frame. One index
        // pass up front makes it O(entities + records); updated/created lookups
        // go through the index and pushes extend it, so the merge order of
        // records is preserved exactly as before.
        let mut index: HashMap<EntityId, usize> = entities
            .iter()
            .enumerate()
            .map(|(i, e)| (e.entity_id, i))
            .collect();
        for created in &delta.created {
            let snapshot = EntitySnapshot::from(created);
            if let Some(slot) = index.get(&snapshot.entity_id).copied() {
                entities[slot] = snapshot;
            } else {
                index.insert(snapshot.entity_id, entities.len());
                entities.push(snapshot);
            }
        }
        for update in &delta.updated {
            if let Some(&slot) = index.get(&update.entity_id) {
                entities[slot].apply_state_update(update);
            }
        }
        entities.retain(|e| !delta.destroyed.contains(&e.entity_id));

        let mut projectiles = self.projectiles;
        let mut pindex: HashMap<EntityId, usize> = projectiles
            .iter()
            .enumerate()
            .map(|(i, p)| (p.entity_id, i))
            .collect();
        for created in &delta.projectile_created {
            let snapshot = ProjectileSnapshot::from(created);
            if let Some(slot) = pindex.get(&snapshot.entity_id).copied() {
                projectiles[slot] = snapshot;
            } else {
                pindex.insert(snapshot.entity_id, projectiles.len());
                projectiles.push(snapshot);
            }
        }
        for update in &delta.projectile_updated {
            if let Some(&slot) = pindex.get(&update.entity_id) {
                projectiles[slot].apply_state_update(update);
            }
        }
        projectiles.retain(|p| !delta.projectile_destroyed.contains(&p.entity_id));

        Snapshot {
            tick: Tick(delta.server_tick as u64),
            time: delta.server_time,
            entities,
            projectiles,
            // The merged snapshot carries no events — same contract as before
            // №637 (`Vec::new()`), events travel via the event stream.
            events: Vec::new(),
        }
    }

    pub fn to_state_packet(&self) -> StatePacket {
        StatePacket {
            server_tick: self.tick.value() as u32,
            server_time: self.time,
            entities: self.entities.iter().map(EntitySnapshot::to_state).collect(),
            projectiles: self
                .projectiles
                .iter()
                .map(ProjectileSnapshot::to_state)
                .collect(),
        }
    }
}

impl ShipSnapshotData {
    pub fn to_state(&self) -> ShipStateData {
        ShipStateData {
            ship_class_id: self.ship_class_id,
            compartments: self
                .compartments
                .iter()
                .map(CompartmentSnapshot::to_state)
                .collect(),
            stations: self.stations.clone(),
            fuel: self.fuel,
            max_fuel: self.max_fuel,
            speed: self.speed,
            max_speed: self.max_speed,
            heading: self.heading,
            rudder_angle: self.rudder_angle,
            throttle: self.throttle,
            last_hit: self.last_hit,
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
            pump_active: self.pump_active,
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
            ammo_count: self.ammo_count,
            max_ammo: self.max_ammo,
            health: self.health,
            max_health: self.max_health,
            cooldown: self.cooldown,
            max_cooldown: self.max_cooldown,
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
            // Bug №274: symmetric with `From<&PlayerEntity>` — a field dropped
            // here would silently vanish on a full-state rebuild.
            current_ship: self.current_ship,
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
        // Bug №24: only overwrite fields the delta actually carried; None
        // means "unchanged", mirroring EntityStateUpdate semantics.
        if let Some(p) = update.position {
            self.position = p;
        }
        if let Some(v) = update.velocity {
            self.velocity = v;
        }
        if let Some(l) = update.lifetime {
            self.lifetime = l;
        }
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
            station_data: self
                .station_data
                .as_ref()
                .map(StationSnapshotData::to_state),
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
            station_data: self
                .station_data
                .as_ref()
                .map(StationSnapshotData::to_state),
            player_data: self.player_data.as_ref().map(PlayerSnapshotData::to_state),
        }
    }
}

impl ProjectileUpdate {
    pub fn to_state_update(&self) -> ProjectileStateUpdate {
        ProjectileStateUpdate {
            entity_id: self.entity_id,
            // Bug №24: keep None as None — no zeroing of unchanged fields.
            position: self.position,
            velocity: self.velocity,
            lifetime: self.lifetime,
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
            is_resync: false,
            created: self.created.iter().map(EntitySnapshot::to_state).collect(),
            updated: self
                .updated
                .iter()
                .map(EntityUpdate::to_state_update)
                .collect(),
            destroyed: self.destroyed.clone(),
            projectile_created: self
                .projectile_created
                .iter()
                .map(ProjectileSnapshot::to_state)
                .collect(),
            projectile_updated: self
                .projectile_updated
                .iter()
                .map(ProjectileUpdate::to_state_update)
                .collect(),
            projectile_destroyed: self.projectile_destroyed.clone(),
        }
    }
}

#[cfg(test)]
mod compartment_entity_snapshot_tests {
    use super::*;
    use rfs_core::entity::{Bounds, CompartmentEntity, EntityId};

    fn compartment(is_breached: bool) -> CompartmentEntity {
        let local = Bounds::new(Vec3f::new(-10.0, -5.0, -20.0), Vec3f::new(10.0, 5.0, 20.0));
        CompartmentEntity {
            entity_id: EntityId::new(7),
            ship_id: EntityId::new(1),
            name: "bow".into(),
            local_bounds: local,
            world_bounds: local,
            water_level: 0.0,
            max_water_level: 100.0,
            is_sealed: !is_breached,
            is_breached,
            fire_intensity: 0.0,
            pump_active: false,
            connected_compartments: SmallVec::new(),
            stations: SmallVec::new(),
            pump_capacity: 0.0,
            bulkheads: SmallVec::new(),
        }
    }

    #[test]
    fn an_intact_compartment_reports_full_health() {
        let snapshot = EntitySnapshot::from(&compartment(false));
        assert_eq!(snapshot.health, 100.0);
        assert_eq!(snapshot.max_health, 100.0);
    }

    /// Bug №606, negative control: with the old constant this passes 100.0 and
    /// the assertion below goes red, so the test measures the fix and not the
    /// fixture.
    #[test]
    fn a_breached_compartment_does_not_claim_full_health() {
        let snapshot = EntitySnapshot::from(&compartment(true));
        assert_eq!(
            snapshot.health, 0.0,
            "layer 1 reports full health for a compartment layer 0 marks breached"
        );
        assert_eq!(
            snapshot.max_health, 100.0,
            "the ceiling must stay for ratio math"
        );
    }
}

#[cfg(test)]
mod resync_cadence_tests {
    //! Bug №220: the record analysed a recovery ceiling of up to 5 s before
    //! destroyed entities are pruned from a client's layer. The resync
    //! cadence is the actual ceiling for how long a ghost can linger when a
    //! layer delta was itself lost on the unreliable channel, so it must
    //! never drift back to a seconds-scale value without an explicit
    //! "layer empty" signal in the protocol.

    use super::*;
    use rfs_core::time::TICK_DURATION;
    use std::time::Duration;

    #[test]
    fn the_resync_cadence_is_the_ghost_ceiling_and_stays_short() {
        let ceiling_ticks = *LAYER_RESYNC_TICKS.iter().max().unwrap();
        let ceiling = Duration::from_secs_f64(ceiling_ticks as f64 * TICK_DURATION.as_secs_f64());
        assert!(
            ceiling < Duration::from_millis(750),
            "resync ceiling {} ticks = {:.2} s — ghost recovery must stay sub-second",
            ceiling_ticks,
            ceiling.as_secs_f64()
        );
        assert!(
            LAYER_RESYNC_TICKS.iter().all(|&t| t > 0),
            "a zero tick cadence would resync every tick and swallow the delta stream"
        );
    }
}

#[cfg(test)]
mod apply_delta_tests {
    //! Behavioral coverage for the merge rewrite (Bug №637). The old code
    //! answered every created/updated record with a linear scan of the whole
    //! vector; the rewrite answers through an index. Same semantics must be
    //! pinned, because the index rewrite is exactly the kind of change that
    //! silently breaks record ordering or upsert handling.

    use super::*;
    use rfs_core::entity::EntityId;
    use rfs_core::math::Quatf;

    fn entity(id: u64, health: f32) -> EntitySnapshot {
        EntitySnapshot {
            entity_id: EntityId::new(id),
            entity_type: EntityType::Ship,
            transform: Transform::new(Vec3f::new(1.0, 2.0, 3.0), Quatf::IDENTITY, Vec3f::ONE),
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health,
            max_health: 100.0,
            flags: EntityFlags::NONE,
            ship_data: None,
            station_data: None,
            player_data: None,
        }
    }

    fn wire_entity(id: u64, health: f32) -> EntityState {
        EntityState {
            entity_id: EntityId::new(id),
            entity_type: EntityType::Ship,
            transform: Transform::new(Vec3f::new(9.0, 0.0, 0.0), Quatf::IDENTITY, Vec3f::ONE),
            velocity: Vec3f::new(5.0, 0.0, 0.0),
            angular_velocity: Vec3f::ZERO,
            health,
            max_health: 100.0,
            flags: EntityFlags::NONE,
            ship_data: None,
            station_data: None,
            player_data: None,
        }
    }

    fn projectile(id: u64, lifetime: f32) -> ProjectileSnapshot {
        ProjectileSnapshot {
            entity_id: EntityId::new(id),
            projectile_type: ProjectileType::Cannonball,
            position: Vec3f::new(1.0, 1.0, 1.0),
            velocity: Vec3f::new(10.0, 0.0, 0.0),
            spawn_tick: 1,
            lifetime,
            owner: EntityId::new(1),
            damage: 5.0,
            penetration: 0.0,
        }
    }

    fn wire_projectile(id: u64) -> ProjectileState {
        ProjectileState {
            entity_id: EntityId::new(id),
            projectile_type: ProjectileType::Cannonball,
            position: Vec3f::new(2.0, 2.0, 2.0),
            velocity: Vec3f::new(3.0, 0.0, 0.0),
            spawn_tick: 2,
            lifetime: 7.0,
            owner: EntityId::new(1),
            damage: 9.0,
            penetration: 0.0,
        }
    }

    fn base_snapshot() -> Snapshot {
        Snapshot {
            tick: Tick(10),
            time: 10.0 / 30.0,
            entities: vec![entity(1, 100.0), entity(2, 90.0)],
            projectiles: vec![projectile(500, 4.0)],
            events: Vec::new(),
        }
    }

    fn delta() -> StateDeltaPacket {
        StateDeltaPacket {
            layer: LAYER_SHIP,
            base_tick: 10,
            server_tick: 12,
            server_time: 12.0 / 30.0,
            is_resync: false,
            created: Vec::new(),
            updated: Vec::new(),
            destroyed: Vec::new(),
            projectile_created: Vec::new(),
            projectile_updated: Vec::new(),
            projectile_destroyed: Vec::new(),
        }
    }

    /// Layered deltas carry `created` as an upsert: an id already present from
    /// a full state or another layer's resync must be replaced in place, not
    /// duplicated — and the records not named by the delta must all survive.
    ///
    /// The `updated` record for 3 (arriving *created* in this very delta) is
    /// the index-regression guard: the lookup must see entities created by the
    /// same delta, not only those that came with the base.
    #[test]
    fn a_created_record_upserts_in_place_instead_of_duplicating() {
        let mut d = delta();
        d.created = vec![wire_entity(2, 80.0), wire_entity(3, 70.0)];
        d.updated.push(EntityStateUpdate {
            entity_id: EntityId::new(3),
            transform: None,
            velocity: None,
            angular_velocity: None,
            health: Some(42.0),
            flags: None,
            ship_data: None,
            station_data: None,
            player_data: None,
        });
        d.destroyed = vec![EntityId::new(1)];

        let merged = base_snapshot().apply_delta(&d);

        let ids: Vec<u64> = merged.entities.iter().map(|e| e.entity_id.0).collect();
        assert_eq!(
            ids,
            vec![2, 3],
            "existing id replaced in place, new id appended, destroyed dropped"
        );
        assert_eq!(
            merged.entities[0].health, 80.0,
            "the upsert replaces the stored record"
        );
        let created = merged
            .entities
            .iter()
            .find(|e| e.entity_id == EntityId::new(3))
            .expect("the created entity stays in the merged snapshot");
        assert_eq!(
            created.health, 42.0,
            "an update for an entity created by the same delta must reach it, not fall through the index"
        );
        assert_eq!(
            merged.tick,
            Tick(12),
            "the merged snapshot carries the delta's tick"
        );
    }

    /// A partial update name-touches only its fields: everything left None
    /// keeps the stored value.
    #[test]
    fn a_partial_update_touches_only_its_fields() {
        let mut d = delta();
        d.updated.push(EntityStateUpdate {
            entity_id: EntityId::new(1),
            transform: None,
            velocity: Some(Vec3f::new(2.0, 0.0, 0.0)),
            angular_velocity: None,
            health: Some(50.0),
            flags: None,
            ship_data: None,
            station_data: None,
            player_data: None,
        });

        let merged = base_snapshot().apply_delta(&d);

        let updated = &merged.entities[0];
        assert_eq!(updated.health, 50.0);
        assert_eq!(updated.velocity, Vec3f::new(2.0, 0.0, 0.0));
        assert_eq!(
            updated.transform.position,
            Vec3f::new(1.0, 2.0, 3.0),
            "fields the delta left None keep the stored value"
        );
        assert_eq!(
            merged.entities[1].health, 90.0,
            "records outside the delta stay untouched"
        );
    }

    /// Projectile side has its own three branches (upsert, partial update,
    /// destroy). None-fields of a partial update keep the stored values, so
    /// the update names only velocity/lifetime and the position must survive.
    #[test]
    fn a_projectile_delta_upserts_and_partially_updates() {
        let mut d = delta();
        d.projectile_created = vec![wire_projectile(600)];
        d.projectile_updated.push(ProjectileStateUpdate {
            entity_id: EntityId::new(500),
            position: None,
            velocity: Some(Vec3f::new(1.0, 1.0, 1.0)),
            lifetime: Some(2.0),
        });

        let merged = base_snapshot().apply_delta(&d);

        assert_eq!(
            merged.projectiles.len(),
            2,
            "created arrives alongside the base shell"
        );
        let updated = &merged.projectiles[0];
        assert_eq!(updated.entity_id, EntityId::new(500));
        // Bug №24 semantics: None keeps the stored value in place.
        assert_eq!(
            updated.position,
            Vec3f::new(1.0, 1.0, 1.0),
            "position the delta left None keeps the stored value"
        );
        assert_eq!(updated.velocity, Vec3f::new(1.0, 1.0, 1.0));
        assert_eq!(updated.lifetime, 2.0);
        assert_eq!(merged.projectiles[1].entity_id, EntityId::new(600));
        assert_eq!(merged.projectiles[1].position, Vec3f::new(2.0, 2.0, 2.0));
    }

    /// `projectile_destroyed` removes exactly the named ids and nothing else.
    #[test]
    fn a_projectile_destroy_removes_exactly_the_named_id() {
        let mut d = delta();
        d.projectile_created = vec![wire_projectile(600)];
        d.projectile_destroyed = vec![EntityId::new(500)];

        let merged = base_snapshot().apply_delta(&d);

        let ids: Vec<u64> = merged.projectiles.iter().map(|p| p.entity_id.0).collect();
        assert_eq!(ids, vec![600], "the named shell is gone, the rest stays");
    }

    /// Bug №672: the full-state branch — `Snapshot::to_state_packet`, fed by
    /// `EntitySnapshot::to_state` and the whole `to_state` family for
    /// ship/station/player/compartment/projectile — must carry every snapshot
    /// field through the wire round-trip. The recipient rebuilds its view with
    /// the symmetric `From<&EntityState>`s, so a field dropped on the way out
    /// shows up here as a changed snapshot, not as a silent gap.
    #[test]
    fn a_full_state_packet_carries_every_field_back() {
        let source = EntitySnapshot {
            entity_id: EntityId::new(2),
            entity_type: EntityType::Ship,
            transform: Transform::new(
                Vec3f::new(4.0, 5.0, 6.0),
                Quatf::from_euler(0.1, 0.2, 0.0),
                Vec3f::ONE,
            ),
            velocity: Vec3f::new(3.0, 0.0, 0.0),
            angular_velocity: Vec3f::new(0.0, 0.1, 0.0),
            health: 70.0,
            max_health: 100.0,
            // Anything but the NONE constant: a dropped field must change the
            // round-trip, so neither side may match a default here.
            flags: EntityFlags::ON_FIRE | EntityFlags::FLOODING,
            ship_data: Some(ShipSnapshotData {
                ship_class_id: 7,
                compartments: vec![CompartmentSnapshot {
                    compartment_id: EntityId::new(1001),
                    water_level: 30.0,
                    max_water_level: 100.0,
                    is_sealed: false,
                    is_breached: true,
                    fire_intensity: 0.0,
                    pump_active: true,
                    connected_compartments: SmallVec::from_slice(&[EntityId::new(1002)]),
                }],
                stations: vec![EntityId::new(2001)],
                fuel: 400.0,
                max_fuel: 500.0,
                speed: 12.0,
                max_speed: 30.0,
                heading: 1.5,
                rudder_angle: -0.2,
                throttle: 0.8,
                last_hit: Some(HitMark {
                    hit_tick: 55,
                    position: Vec3f::new(9.0, 0.0, 0.0),
                    normal: Vec3f::new(-1.0, 0.0, 0.0),
                    compartment: Some(EntityId::new(1001)),
                    damage: 42.0,
                }),
            }),
            station_data: Some(StationSnapshotData {
                station_type: StationType::Gun,
                occupant: Some(EntityId::new(42)),
                yaw: 0.5,
                pitch: -0.1,
                reload_progress: 0.25,
                ammo_type: 3,
                ammo_count: 20,
                max_ammo: 50,
                health: 400.0,
                max_health: 500.0,
                cooldown: 2.0,
                max_cooldown: 10.0,
                is_operational: true,
            }),
            player_data: Some(PlayerSnapshotData {
                player_id: 42,
                name: "fixture".into(),
                team: 1,
                current_station: Some(EntityId::new(2001)),
                current_ship: Some(EntityId::new(2)),
                posture: PlayerPosture::Seated,
                health: 90.0,
                stamina: 80.0,
            }),
        };

        let packet = Snapshot {
            tick: Tick(12),
            time: 0.4,
            entities: vec![source.clone()],
            projectiles: vec![],
            events: Vec::new(),
        }
        .to_state_packet();

        let restored = EntitySnapshot::from(&packet.entities[0]);
        assert_eq!(restored, source, "a wire field got dropped on the way out");
    }

    /// Bug №672 (delta branch): `DeltaSnapshot::to_state_delta` feeds both
    /// `to_state_update` converters. The packet must apply onto a base
    /// snapshot exactly like the records say — created upserts, the partial
    /// update names its fields, and a None stays None through the converter.
    #[test]
    fn a_state_delta_packet_applies_like_the_records_say() {
        let source = entity(4, 60.0);
        let update = EntityUpdate {
            entity_id: EntityId::new(1),
            transform: None,
            velocity: Some(Vec3f::new(6.0, 0.0, 0.0)),
            angular_velocity: None,
            health: Some(50.0),
            flags: None,
            ship_data: None,
            station_data: None,
            player_data: None,
        };
        let proj_update = ProjectileUpdate {
            entity_id: EntityId::new(500),
            position: Some(Vec3f::new(5.0, 5.0, 5.0)),
            velocity: None,
            lifetime: Some(9.0),
        };

        let packet = DeltaSnapshot {
            layer: LAYER_SHIP,
            base_tick: Tick(10),
            target_tick: Tick(12),
            target_time: 12.0 / 30.0,
            created: vec![source.clone()],
            updated: vec![update],
            destroyed: vec![EntityId::new(2)],
            projectile_created: vec![],
            projectile_updated: vec![proj_update],
            projectile_destroyed: vec![],
        }
        .to_state_delta();

        // The created record survived the converter: the recipient can
        // rebuild the source snapshot from exactly what arrived.
        assert_eq!(
            EntitySnapshot::from(&packet.created[0]),
            source,
            "the created record changed shape through to_state"
        );
        assert_eq!(
            packet.updated[0].entity_id,
            EntityId::new(1),
            "the update record keeps its subject"
        );
        assert_eq!(packet.updated[0].health, Some(50.0));
        assert_eq!(packet.updated[0].velocity, Some(Vec3f::new(6.0, 0.0, 0.0)));
        assert_eq!(
            packet.updated[0].angular_velocity, None,
            "None must stay None through the update converter (bug №24 semantics)"
        );
        assert_eq!(packet.destroyed, vec![EntityId::new(2)]);
        assert_eq!(
            packet.projectile_updated[0].position,
            Some(Vec3f::new(5.0, 5.0, 5.0))
        );
        assert_eq!(packet.projectile_updated[0].lifetime, Some(9.0));
        assert_eq!(
            packet.projectile_updated[0].velocity, None,
            "None must stay None through the update converter (bug №24 semantics)"
        );

        // And it applies: the merge is the consumer the packet is built for.
        let merged = base_snapshot().apply_delta(&packet);
        assert_eq!(
            merged
                .entities
                .iter()
                .map(|e| e.entity_id.0)
                .collect::<Vec<_>>(),
            vec![1, 4],
            "created became an entity, destroyed is gone"
        );
        assert_eq!(merged.entities[0].health, 50.0);
        assert_eq!(merged.entities[0].velocity, Vec3f::new(6.0, 0.0, 0.0));
        let restored = &merged.entities[1];
        assert_eq!(
            restored.health, 60.0,
            "the created record carries its own health through the full path"
        );
        assert_eq!(merged.projectiles[0].position, Vec3f::new(5.0, 5.0, 5.0));
        assert_eq!(merged.projectiles[0].lifetime, 9.0);
        assert_eq!(
            merged.projectiles[0].velocity,
            Vec3f::new(10.0, 0.0, 0.0),
            "fields the update left None keep the stored value"
        );
    }
}
