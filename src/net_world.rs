//! The server's world as the render loop sees it.
//!
//! The connection slice taught the client to *receive* the server's world:
//! [`NetBridge::take_latest_state`](crate::net::NetBridge::take_latest_state)
//! hands over the newest merged snapshot, and the loading screen treated its
//! mere presence as "the world arrived". Nothing else read it — the render
//! went on drawing the local showcase scene, so connecting changed nothing
//! on screen. This module is the missing half: it takes the snapshot and
//! puts the server's entities into the scene the geometry pass draws.
//!
//! ```text
//! loop frame: bridge.take_latest_state() -> ServerWorldView::apply()
//!                |  ships, players, projectiles mapped into a pre-built
//!                |  slot pool; drawn transforms computed here, in client
//!                |  space, against the same ocean the sea is drawn from
//!                v
//!              ServerWorldView::sync(&mut scene)
//!                |  the stored transforms written into the scene entities
//!                v
//!            geometry pass draws them (they are ordinary scene entities)
//! ```
//!
//! ## The slot pool
//!
//! The geometry pass builds its draw items once (one uniform buffer per
//! item), so the scene's entity *set* is fixed after the first frame. A pool
//! of entities is therefore built with the scene itself — before
//! `items_for_scene` runs — and an idle slot parks its transform at zero
//! scale: every vertex collapses onto one point, the triangles are
//! degenerate and nothing reaches the rasterizer. Activating a server
//! entity means writing a real transform into a parked slot; releasing one
//! parks it again. No GPU state is ever rebuilt mid-flight.
//!
//! One invariant makes the parked pool drawable at all: `items_for_scene`
//! frustum-culls at build time, and a parked slot's degenerate box sits at
//! the world origin — which the deterministic start camera
//! (`camera_start`, pinned under `RFS_CAMERA_LOCK`) is looking straight at.
//! The pool is therefore always inside the first item build; after that no
//! pass re-culls, so a slot can claim a transform anywhere in the server
//! world and be drawn there.
//!
//! Pool sizes follow the server's interest radii (a client only ever
//! receives what is within them): ships within 5 km of the player's hull on
//! a 3 km spawn line (a handful), players within 100 m (the crew of one
//! hull), projectiles within 3 km (shells in flight). Overflow is logged
//! once and the excess entity is honestly not drawn rather than silently
//! overwriting an occupied slot.
//!
//! ## Client space
//!
//! The server simulates in metres with its waterline at y = 0 and no ocean.
//! The client's sea is a Gerstner field whose calm surface sits at
//! `OCEAN_LEVEL`. One convention maps everything: a server entity at world
//! `p` is drawn at `p.xz` unchanged, with its height shifted so the
//! server's waterline (y = 0) lands on the drawn surface — hulls and the
//! crew riding them get the wave height at their spot from the same ocean
//! the mesh is drawn from (buoyancy and picture are one field, never two);
//! shells in flight skip the wave (they are above the sea) but keep the
//! level shift. This is the same single source of truth the local showcase
//! ship rides.
//!
//! Hull meshes: the server does not replicate hull dimensions, so a ship is
//! drawn as the client's own hull model fitted to a documented display
//! length (the frigate class's compartments span ~90 m; the constant keeps
//! that in metres). The mesh is centre-compensated the same way the local
//! ship is, so its centre lands on the server's transform.
//!
//! ## Identity
//!
//! The camera anchor is the hull our own crew member stands on. Who "ours"
//! is comes from the wire, not from a menu string: the server assigns a
//! connection id at ConnectAccept and echoes it in every player snapshot's
//! `player_id`, while naming every connection `player` — the display name
//! cannot tell crew members apart. [`ServerWorldView::set_connection_id`]
//! carries that id over (the main loop does it as soon as the link is up);
//! until it is, the case-blind display name is the fallback identity.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Quat, Vec3};

use rfs_core::packet::EntityType;
use rfs_net::snapshot::{EntitySnapshot, ProjectileSnapshot};
use rfs_net::Snapshot;

use crate::render::game_loop::{model_fit, ModelFit};
use crate::render::meshes::mesh::Mesh;
use crate::render::scene::{Entity, MeshComponent, Scene, Transform};
use crate::render::water::ocean::{Ocean, OCEAN_LEVEL};
use crate::rhi::{Device, RhiResult};

// ------------------------------------------------------------- constants

/// How many server ships the pool can draw at once. Interest keeps a client
/// to the ships within 5 km of its hull — two or three neighbours on the 3 km
/// spawn line plus its own; eight leaves headroom for drifting.
pub const SERVER_SHIP_POOL: usize = 8;
/// How many server players the pool can draw at once. Interest is 100 m:
/// the crew of the hull the client's player stands on.
pub const SERVER_PLAYER_POOL: usize = 8;
/// How many server projectiles the pool can draw at once. Interest is 3 km:
/// shells in flight around the hull.
pub const SERVER_PROJECTILE_POOL: usize = 16;

/// The length a server ship is drawn at, in world metres. The wire carries
/// no hull dimensions; the frigate class's compartments span roughly 90 m
/// (aft quarter to forecastle), so the drawn hull keeps to that scale.
pub const SERVER_SHIP_LENGTH: f32 = 100.0;
/// A player is a 1.8 m marker on the deck: the server simulates people, the
/// client has no person model yet.
pub const SERVER_PLAYER_SIZE: f32 = 1.8;
/// A projectile is a 0.6 m marker: a shell at server scale, visible in
/// flight, not a ship.
pub const SERVER_PROJECTILE_SIZE: f32 = 0.6;

/// The wake collar radius for a hull drawn at [`SERVER_SHIP_LENGTH`]: the
/// showcase collar (2.6 m around a 3 m model) scaled with the hull.
pub const SERVER_WAKE_RADIUS: f32 = 12.0;

/// How strongly a hull tilts with the wave normal — the same blend the local
/// showcase ship uses, so server hulls bob the way the sea under them does.
const HULL_TILT_BLEND: f32 = 0.55;

// ------------------------------------------------------------- slots

/// One computed transform, ready to be written into a scene entity.
#[derive(Debug, Clone, Copy)]
struct StoredTransform {
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
}

/// One pool slot: a scene entity that either draws a server entity or sits
/// parked at zero scale.
struct PoolSlot {
    scene_entity: usize,
    /// `None` is parked: `sync` keeps the entity collapsed at zero scale so
    /// the rasterizer discards every triangle of the slot.
    content: Option<StoredTransform>,
}

impl PoolSlot {
    /// The transform parked slots are kept at.
    fn parked_transform() -> StoredTransform {
        StoredTransform {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ZERO,
        }
    }
}

// ------------------------------------------------------------- view

/// The server's world, mapped into the client's scene.
pub struct ServerWorldView {
    /// Hull slots and the ship entity id -> slot index map.
    ship_slots: Vec<PoolSlot>,
    ships: HashMap<u64, usize>,
    /// Crew slots and the player entity id -> slot index map.
    player_slots: Vec<PoolSlot>,
    players: HashMap<u64, usize>,
    /// Shell slots and the projectile entity id -> slot index map.
    projectile_slots: Vec<PoolSlot>,
    projectiles: HashMap<u64, usize>,
    /// The hull mesh every ship slot draws, fitted to [`SERVER_SHIP_LENGTH`].
    ship_fit: ModelFit,
    /// The player name whose `current_ship` becomes the camera anchor. The
    /// fallback identity: the server names every connection `player`, so the
    /// name alone cannot tell crew members apart.
    our_name: String,
    /// Our client's connection id, the server-assigned identity. Every
    /// player snapshot carries it in `player_id`, so this — not the name —
    /// is what binds the camera anchor to our own hull.
    our_connection: Option<u32>,
    /// Our player's hull, once seen in a snapshot.
    our_ship: Option<u64>,
    /// Our hull's drawn position (buoyancy included), for the camera and the
    /// water shader's wake focus.
    anchor: Option<Vec3>,
    /// Our hull's server-side drive, as the snapshot reports it: throttle
    /// commanded and speed measured. The diagnostics line reads this —
    /// throttle near 1 with zero speed is a server-physics fact, throttle
    /// zero means our input never became thrust.
    hull_drive: Option<(f32, f32)>,
    /// Diagnostics for the stats line: snapshots accepted, entities drawn.
    applied_snapshots: u64,
    drawn_ships: usize,
    drawn_players: usize,
    drawn_projectiles: usize,
    /// Each pool kind reports a full pool once, then goes quiet.
    overflow_reported: [bool; 3],
}

impl ServerWorldView {
    /// Build the pool meshes and attach the pool entities to `scene`.
    ///
    /// Must run before the geometry pass builds its items — the pool
    /// entities have to be part of the first item build or they can never be
    /// drawn. The hull slots share the client's own hull mesh (the same
    /// `Arc` the showcase ship draws); the player and shell slots get small
    /// cubes with their own GPU buffers. A hull mesh that could not load
    /// leaves the ship slots empty — players and shells still draw, and the
    /// log says so.
    pub fn attach(
        scene: &mut Scene,
        device: &Device,
        ship_mesh: Option<&Arc<Mesh>>,
        our_name: &str,
    ) -> RhiResult<Self> {
        Self::attach_with(scene, Some(device), ship_mesh, our_name)
    }

    /// [`Self::attach`] with the device optional: the CPU tests build pools
    /// without GPU buffers, because nothing draws there. Production always
    /// passes a device.
    fn attach_with(
        scene: &mut Scene,
        device: Option<&Device>,
        ship_mesh: Option<&Arc<Mesh>>,
        our_name: &str,
    ) -> RhiResult<Self> {
        // The hull fit is computed once from the mesh's own extent: the same
        // normalisation the showcase ship gets, at server hull length.
        let ship_fit = ship_mesh
            .map(|mesh| model_fit(mesh, SERVER_SHIP_LENGTH))
            .unwrap_or(ModelFit {
                center: Vec3::ZERO,
                scale: 1.0,
            });

        let ship_slots = Self::spawn_pool(
            scene,
            "server ship",
            SERVER_SHIP_POOL,
            ship_mesh.cloned(),
        );
        let player_mesh = Self::marker_mesh(device, "server-player", SERVER_PLAYER_SIZE)?;
        let projectile_mesh =
            Self::marker_mesh(device, "server-projectile", SERVER_PROJECTILE_SIZE)?;
        let player_slots = Self::spawn_pool(
            scene,
            "server player",
            SERVER_PLAYER_POOL,
            Some(player_mesh),
        );
        let projectile_slots = Self::spawn_pool(
            scene,
            "server projectile",
            SERVER_PROJECTILE_POOL,
            Some(projectile_mesh),
        );

        Ok(Self {
            ship_slots,
            ships: HashMap::new(),
            player_slots,
            players: HashMap::new(),
            projectile_slots,
            projectiles: HashMap::new(),
            ship_fit,
            our_name: our_name.to_string(),
            our_connection: None,
            our_ship: None,
            anchor: None,
            hull_drive: None,
            applied_snapshots: 0,
            drawn_ships: 0,
            drawn_players: 0,
            drawn_projectiles: 0,
            overflow_reported: [false; 3],
        })
    }

    /// A small centred cube with GPU buffers, for marker entities.
    fn marker_mesh(device: Option<&Device>, name: &str, size: f32) -> RhiResult<Arc<Mesh>> {
        let mut mesh = Mesh::cube(name, size);
        if let Some(device) = device {
            mesh.create_buffers(device)?;
        }
        Ok(Arc::new(mesh))
    }

    /// Create `count` parked entities drawing `mesh`, and return their slots.
    fn spawn_pool(
        scene: &mut Scene,
        label: &str,
        count: usize,
        mesh: Option<Arc<Mesh>>,
    ) -> Vec<PoolSlot> {
        let Some(mesh) = mesh else {
            // A missing hull mesh is not a failure of the whole view: the
            // caller's log already names it, and the other pools still work.
            return Vec::new();
        };
        (0..count)
            .map(|index| {
                let mut entity = Entity::new(&format!("{label} {index}"));
                entity.add_component(PoolSlot::parked_transform().into_scene_transform());
                // Every slot shares the one mesh through the `Arc`; the
                // geometry pass clones the same handle into each draw item.
                entity.add_component(MeshComponent::new(mesh.clone()));
                let scene_entity = scene.add_entity(entity);
                PoolSlot {
                    scene_entity,
                    content: None,
                }
            })
            .collect()
    }

    /// Feed one snapshot in: map new entities to free slots, release slots
    /// whose entities are no longer in the world, and compute the drawn
    /// transforms. Call `sync` afterwards to write them into the scene.
    ///
    /// `ocean` is the same field the drawn sea and the showcase ship ride:
    /// hulls and crew take their lift from it, so buoyancy and picture stay
    /// one source of truth.
    pub fn apply(&mut self, snapshot: &Snapshot, ocean: &Ocean) {
        self.applied_snapshots += 1;

        // Presence release: a slot whose entity is absent from this snapshot
        // is freed and parked. The merged snapshot the client builds is the
        // protocol's whole known world, so absence here is the protocol
        // telling us the entity is gone from view.
        Self::release_absent(&mut self.ships, &mut self.ship_slots, &snapshot.entities);
        Self::release_absent(&mut self.players, &mut self.player_slots, &snapshot.entities);
        self.release_projectiles_absent(&snapshot.projectiles);

        self.drawn_ships = 0;
        self.drawn_players = 0;
        self.drawn_projectiles = 0;
        // Players first: whose hull is ours must be known before the ships
        // are placed, or a snapshot that lists the player after its ship —
        // the wire makes no order promise — would report no anchor, and the
        // camera would wait a snapshot to move.
        for entity in &snapshot.entities {
            if entity.entity_type == EntityType::Player {
                self.take_player(entity, ocean);
            }
        }
        for entity in &snapshot.entities {
            match entity.entity_type {
                EntityType::Ship => self.take_ship(entity, ocean),
                EntityType::Player => {}
                // Stations and compartments are gameplay state inside the
                // hull: they are replicated for interaction, not for the
                // picture, and drawing them would put cubes inside the hull
                // mesh. They stay undrawn by design.
                EntityType::Station | EntityType::Compartment | EntityType::Projectile => {}
            }
        }
        self.take_projectiles(&snapshot.projectiles);

        // The anchor is strictly this snapshot's answer. If our hull is not
        // in the view — interest never drops our own hull, but a kick, a
        // layer stall or a bug must not leave the camera chasing a ghost —
        // it clears until the hull is seen again.
        if !self.our_ship.map_or(false, |ship| {
            snapshot
                .entities
                .iter()
                .any(|entity| {
                    entity.entity_type == EntityType::Ship && entity.entity_id.0 == ship
                })
        }) {
            self.anchor = None;
            self.hull_drive = None;
        }
    }

    /// Release every slot whose id is absent from this snapshot's entities.
    fn release_absent(
        map: &mut HashMap<u64, usize>,
        slots: &mut [PoolSlot],
        entities: &[EntitySnapshot],
    ) {
        let absent: Vec<u64> = map
            .keys()
            .filter(|id| !entities.iter().any(|entity| entity.entity_id.0 == **id))
            .copied()
            .collect();
        for id in absent {
            let slot = map.remove(&id).expect("the map just listed it");
            if let Some(entry) = slots.get_mut(slot) {
                entry.content = None;
            }
        }
    }

    /// Release shell slots whose shells are absent from this snapshot.
    fn release_projectiles_absent(&mut self, shells: &[ProjectileSnapshot]) {
        let absent: Vec<u64> = self
            .projectiles
            .keys()
            .filter(|id| !shells.iter().any(|shell| shell.entity_id.0 == **id))
            .copied()
            .collect();
        for id in absent {
            let slot = self.projectiles.remove(&id).expect("the map just listed it");
            if let Some(entry) = self.projectile_slots.get_mut(slot) {
                entry.content = None;
            }
        }
    }

    /// Claim a free slot of the given pool.
    fn free_slot(slots: &mut [PoolSlot]) -> Option<usize> {
        slots
            .iter()
            .position(|slot| slot.content.is_none())
            .inspect(|index| slots[*index].content = Some(PoolSlot::parked_transform()))
    }

    /// Report a full pool once: the honest "this entity exists but the pool
    /// is exhausted" line instead of a silent drop or a slot overwrite.
    fn report_overflow(&mut self, kind: usize, label: &str) {
        if !self.overflow_reported[kind] {
            self.overflow_reported[kind] = true;
            eprintln!(
                "[net_world] {label} pool full: further entities this session are not drawn"
            );
        }
    }

    /// Map one ship onto a slot and compute its drawn transform.
    fn take_ship(&mut self, entity: &EntitySnapshot, ocean: &Ocean) {
        if self.ship_slots.is_empty() {
            self.report_overflow(0, "ship");
            return;
        }
        let slot = match self.ships.get(&entity.entity_id.0) {
            Some(&slot) => slot,
            None => match Self::free_slot(&mut self.ship_slots) {
                Some(slot) => {
                    self.ships.insert(entity.entity_id.0, slot);
                    slot
                }
                None => {
                    self.report_overflow(0, "ship");
                    return;
                }
            },
        };
        let (target, rotation, scale) = Self::hull_transform(&entity.transform, &self.ship_fit, ocean);
        self.ship_slots[slot].content = Some(StoredTransform {
            position: Self::hull_mesh_position(target, rotation, scale, &self.ship_fit),
            rotation,
            scale,
        });
        self.drawn_ships += 1;
        // The hull our player stands on is the anchor the camera teleports
        // to and the wake draws around: the hull's world position, buoyancy
        // included, without the mesh's own centre compensation.
        if self.our_ship == Some(entity.entity_id.0) {
            self.anchor = Some(target);
            // The snapshot carries our hull's own drive report; the station
            // and compartment details stay undrawn, but the throttle and the
            // speed they measure are the diagnostics line's facts.
            self.hull_drive = entity.ship_data.as_ref().map(|data| (data.throttle, data.speed));
        }
    }

    /// Map one player onto a slot; remember whose hull is ours.
    fn take_player(&mut self, entity: &EntitySnapshot, ocean: &Ocean) {
        let player_data = match &entity.player_data {
            Some(data) => data,
            None => return,
        };
        // Whose hull is ours: the server-assigned connection id when it is
        // known (every player snapshot carries it in `player_id`; the name
        // is `player` for every connection, so it cannot tell crew members
        // apart), and the display name — case-blind, the server lowercases
        // — until it is.
        let ours = match self.our_connection {
            Some(id) => player_data.player_id == id,
            None => player_data.name.eq_ignore_ascii_case(&self.our_name),
        };
        if ours {
            if let Some(ship) = player_data.current_ship {
                self.our_ship = Some(ship.0);
            }
        }
        if self.player_slots.is_empty() {
            self.report_overflow(1, "player");
            return;
        }
        let slot = match self.players.get(&entity.entity_id.0) {
            Some(&slot) => slot,
            None => match Self::free_slot(&mut self.player_slots) {
                Some(slot) => {
                    self.players.insert(entity.entity_id.0, slot);
                    slot
                }
                None => {
                    self.report_overflow(1, "player");
                    return;
                }
            },
        };
        // A crew member rides the hull: the server already places them in
        // world space on the deck, so the mapping is the plain waterline
        // shift plus the wave under their feet.
        let position = Self::crew_position(&entity.transform, ocean);
        let rotation = Quat::from_xyzw(
            entity.transform.rotation.x,
            entity.transform.rotation.y,
            entity.transform.rotation.z,
            entity.transform.rotation.w,
        );
        self.player_slots[slot].content = Some(StoredTransform {
            position,
            rotation,
            scale: Vec3::ONE,
        });
        self.drawn_players += 1;
    }

    /// Map this snapshot's shells onto slots.
    fn take_projectiles(&mut self, shells: &[ProjectileSnapshot]) {
        for shell in shells {
            if self.projectile_slots.is_empty() {
                self.report_overflow(2, "projectile");
                break;
            }
            let slot = match self.projectiles.get(&shell.entity_id.0) {
                Some(&slot) => slot,
                None => match Self::free_slot(&mut self.projectile_slots) {
                    Some(slot) => {
                        self.projectiles.insert(shell.entity_id.0, slot);
                        slot
                    }
                    None => {
                        self.report_overflow(2, "projectile");
                        break;
                    }
                },
            };
            // A shell flies above the sea: the level shift aligns it with
            // everything else, the wave does not chase it.
            self.projectile_slots[slot].content = Some(StoredTransform {
                position: Vec3::new(
                    shell.position.x,
                    shell.position.y + OCEAN_LEVEL,
                    shell.position.z,
                ),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            });
            self.drawn_projectiles += 1;
        }
    }

    /// The drawn transform of a hull at server scale: the server transform,
    /// lifted and tilted by the wave field the drawn sea uses.
    ///
    /// Returns the hull's *world* position (the point its mesh centre must
    /// land on), the drawn rotation and the drawn scale. The mesh's own
    /// centre compensation is applied separately in
    /// [`Self::hull_mesh_position`], because it must rotate with the hull.
    fn hull_transform(
        server: &rfs_core::math::Transform,
        fit: &ModelFit,
        ocean: &Ocean,
    ) -> (Vec3, Quat, Vec3) {
        let position = Vec3::from(server.position);
        let rotation = Quat::from_xyzw(
            server.rotation.x,
            server.rotation.y,
            server.rotation.z,
            server.rotation.w,
        );
        let server_scale = Vec3::from(server.scale);
        let surface = ocean.height_at(position.x, position.z);
        let normal = ocean.normal_at(position.x, position.z);
        let tilt = Quat::IDENTITY.slerp(
            Quat::from_rotation_arc(Vec3::Y, Vec3::new(normal.x, normal.y, normal.z)),
            HULL_TILT_BLEND,
        );
        let drawn_rotation = rotation * tilt;
        // The server's waterline (y = 0) lands on the drawn surface, so the
        // hull rises and falls with the same field the picture shows.
        let target = Vec3::new(position.x, position.y + surface, position.z);
        (target, drawn_rotation, server_scale * fit.scale)
    }

    /// Where the hull *entity's* origin must sit so the mesh's centre lands
    /// on `target`: the centre compensation, rotated with the hull.
    fn hull_mesh_position(target: Vec3, rotation: Quat, scale: Vec3, fit: &ModelFit) -> Vec3 {
        // The mesh vertex v maps to `position + R * (scale * v)`. The mesh
        // centre `fit.center` must land on `target`, so
        // `position = target - R * (scale * fit.center)`.
        target - rotation * (scale * fit.center)
    }

    /// Where a crew member (or any deck-level server entity) is drawn: the
    /// server's world position with the waterline shifted onto the surface,
    /// riding the same wave the hull under them does.
    fn crew_position(server: &rfs_core::math::Transform, ocean: &Ocean) -> Vec3 {
        let position = Vec3::from(server.position);
        let surface = ocean.height_at(position.x, position.z);
        Vec3::new(position.x, position.y + surface, position.z)
    }

    /// Write the computed server world into the scene: every claimed slot
    /// gets its transform, parked slots stay collapsed. Idempotent — the
    /// loop calls it every frame with whatever `apply` last computed.
    pub fn sync(&mut self, scene: &mut Scene) {
        for slot in self
            .ship_slots
            .iter()
            .chain(self.player_slots.iter())
            .chain(self.projectile_slots.iter())
        {
            let transform = slot.content.unwrap_or_else(PoolSlot::parked_transform);
            if let Some(entity) = scene.get_entity_mut(slot.scene_entity) {
                if let Some(entity_transform) = entity.transform_mut() {
                    entity_transform.set_position(transform.position);
                    entity_transform.set_rotation(transform.rotation);
                    entity_transform.set_scale(transform.scale);
                }
            }
        }
    }

    /// Our hull's drawn position, once seen. The camera teleport and the
    /// water shader's wake focus both read this.
    pub fn anchor(&self) -> Option<Vec3> {
        self.anchor
    }

    /// Tell the view who we are on the wire: the server-assigned connection
    /// id the client learned from ConnectAccept. Call it once the link is up
    /// (the id is constant for the session; re-sending it is harmless).
    /// With this set, the anchor follows the player whose snapshot
    /// `player_id` is ours, regardless of the display name.
    pub fn set_connection_id(&mut self, id: u32) {
        self.our_connection = Some(id);
    }

    /// Our hull's server-side drive report from the newest snapshot:
    /// `(throttle, speed)`, or `None` while no hull of ours is in view.
    pub fn hull_drive(&self) -> Option<(f32, f32)> {
        self.hull_drive
    }

    /// How many snapshots the view has applied, for the stats line.
    pub fn applied_snapshots(&self) -> u64 {
        self.applied_snapshots
    }

    /// The stats line: how much of the server world is currently drawn.
    pub fn drawn_summary(&self) -> String {
        format!(
            "{} ships, {} players, {} projectiles",
            self.drawn_ships, self.drawn_players, self.drawn_projectiles
        )
    }

    /// Drop the whole server world back to parked slots (the connection is
    /// gone; the client keeps drawing its own local scene again).
    pub fn reset(&mut self) {
        self.ships.clear();
        self.players.clear();
        self.projectiles.clear();
        for slot in self
            .ship_slots
            .iter_mut()
            .chain(self.player_slots.iter_mut())
            .chain(self.projectile_slots.iter_mut())
        {
            slot.content = None;
        }
        self.our_ship = None;
        self.our_connection = None;
        self.anchor = None;
        self.hull_drive = None;
        self.overflow_reported = [false; 3];
    }
}

impl StoredTransform {
    /// The scene-component form of a parked slot.
    fn into_scene_transform(self) -> Transform {
        let mut transform = Transform::new();
        transform.set_position(self.position);
        transform.set_rotation(self.rotation);
        transform.set_scale(self.scale);
        transform
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::scene::Scene as TestScene;
    use rfs_core::math::{Quatf, Transform as ServerTransform, Vec3f};
    use rfs_core::packet::{EntityFlags, PlayerPosture, ProjectileType};
    use rfs_core::spatial::EntityId;
    use rfs_core::time::Tick;
    use rfs_net::snapshot::{PlayerSnapshotData, Snapshot};

    fn scene_with_pool(our_name: &str) -> (TestScene, ServerWorldView) {
        let mut scene = TestScene::new("net_world test");
        let hull = Arc::new(Mesh::cube("test hull", 1.0));
        // No device: the CPU test never draws, so the meshes need no GPU
        // buffers.
        let view = ServerWorldView::attach_with(&mut scene, None, Some(&hull), our_name)
            .expect("the pool attaches without a device");
        (scene, view)
    }

    fn ship_entity(id: u64, position: Vec3f) -> EntitySnapshot {
        EntitySnapshot {
            entity_id: EntityId(id),
            entity_type: EntityType::Ship,
            transform: ServerTransform::new(position, Quatf::IDENTITY, Vec3f::ONE),
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: 1.0,
            max_health: 1.0,
            flags: EntityFlags::NONE,
            ship_data: None,
            station_data: None,
            player_data: None,
        }
    }

    fn player_entity(id: u64, name: &str, ship: Option<u64>) -> EntitySnapshot {
        player_entity_with(id as u32, id, name, ship)
    }

    /// A crew member with an explicit wire identity: `player_id` is the
    /// server-assigned connection id, `entity_id` is the scene id.
    fn player_entity_with(
        player_id: u32,
        entity_id: u64,
        name: &str,
        ship: Option<u64>,
    ) -> EntitySnapshot {
        EntitySnapshot {
            entity_id: EntityId(entity_id),
            entity_type: EntityType::Player,
            transform: ServerTransform::new(
                Vec3f::new(3002.0, 4.0, -8.0),
                Quatf::IDENTITY,
                Vec3f::ONE,
            ),
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: 100.0,
            max_health: 100.0,
            flags: EntityFlags::NONE,
            ship_data: None,
            station_data: None,
            player_data: Some(PlayerSnapshotData {
                player_id,
                name: name.to_string(),
                team: 0,
                current_station: None,
                current_ship: ship.map(EntityId),
                posture: PlayerPosture::Standing,
                health: 100.0,
                stamina: 100.0,
            }),
        }
    }

    fn shell_entity(id: u64, position: Vec3f) -> ProjectileSnapshot {
        ProjectileSnapshot {
            entity_id: EntityId(id),
            projectile_type: ProjectileType::ExplosiveShell,
            position,
            velocity: Vec3f::ZERO,
            spawn_tick: 0,
            lifetime: 10.0,
            owner: EntityId(10),
            damage: 1.0,
            penetration: 1.0,
        }
    }

    fn snapshot_at(
        entities: Vec<EntitySnapshot>,
        shells: Vec<ProjectileSnapshot>,
    ) -> Snapshot {
        Snapshot {
            tick: Tick(1),
            time: 0.0,
            entities,
            projectiles: shells,
            events: Vec::new(),
        }
    }

    /// The transform of the pool entity with the given name.
    fn pool_transform(scene: &TestScene, label: &str) -> (Vec3, Quat, Vec3) {
        let entity = scene
            .entities()
            .iter()
            .find(|entity| entity.name() == label)
            .unwrap_or_else(|| panic!("{label} is not in the scene"));
        let transform = entity.transform().expect("the slot has a transform");
        (
            transform.position,
            transform.rotation,
            transform.scale,
        )
    }

    #[test]
    fn a_hull_is_drawn_with_its_mesh_centre_on_the_server_transform() {
        // A hand-made fit: the mesh's centre sits at (0, 2, 5) in mesh
        // space, and the mesh is drawn 4x its authored size.
        let fit = ModelFit {
            center: Vec3::new(0.0, 2.0, 5.0),
            scale: 4.0,
        };
        let mut ocean = Ocean::new(40.0, 4, OCEAN_LEVEL);
        // A flat sea: the tilt is identity, so the compensation arithmetic
        // can be asserted by hand. The surface mapping itself is asserted
        // against the same field the sea is drawn from.
        ocean.waves_mut().set_height_scale(0.0);
        // Identity server rotation and scale, on the spawn line.
        let server = ServerTransform::new(
            Vec3f::new(3000.0, 0.0, 100.0),
            Quatf::IDENTITY,
            Vec3f::ONE,
        );
        let (target, rotation, scale) =
            ServerWorldView::hull_transform(&server, &fit, &ocean);

        // The hull's world anchor is the server position with the waterline
        // shifted onto the drawn surface — the same field the sea is drawn
        // from, not a second source of truth; the drawn scale carries the
        // fit.
        let surface = ocean.height_at(target.x, target.z);
        assert!((target.y - surface).abs() < 1e-4, "target {target:?} vs surface {surface}");
        assert!(target.x == 3000.0 && target.z == 100.0);
        assert!((scale.x - 4.0).abs() < 1e-5);

        // The compensation really moves the origin: with the centre at
        // (0, 2, 5), drawn at scale 4 under identity rotation, the drawn
        // origin sits exactly (0, 8, 20) short of the anchor.
        let position = ServerWorldView::hull_mesh_position(target, rotation, scale, &fit);
        let centre_drawn = position + rotation * (scale * fit.center);
        assert!(
            centre_drawn.abs_diff_eq(target, 1e-4),
            "the mesh centre drifted off the server transform: {centre_drawn:?} vs {target:?}"
        );
        let expected_origin = target - Vec3::new(0.0, 8.0, 20.0);
        assert!(
            position.abs_diff_eq(expected_origin, 1e-4),
            "the origin is not the compensated anchor: {position:?} vs {expected_origin:?}"
        );
    }

    #[test]
    fn an_idle_hull_rides_the_calm_surface_without_tilting() {
        let mut ocean = Ocean::new(40.0, 4, OCEAN_LEVEL);
        // A zero height scale is the documented flat sea: height is the
        // level everywhere and the normal is straight up.
        ocean.waves_mut().set_height_scale(0.0);
        let fit = ModelFit {
            center: Vec3::ZERO,
            scale: 1.0,
        };
        let server = ServerTransform::new(
            Vec3f::new(1.0, 0.0, 1.0),
            Quatf::IDENTITY,
            Vec3f::ONE,
        );
        let (target, rotation, _) = ServerWorldView::hull_transform(&server, &fit, &ocean);
        // A hull at the server's waterline (y = 0) rides the calm surface:
        // exactly the level.
        assert!((target.y - OCEAN_LEVEL).abs() < 1e-5);
        assert!(
            rotation.abs_diff_eq(Quat::IDENTITY, 1e-5),
            "a calm sea must not tilt the hull"
        );
    }

    #[test]
    fn the_pool_takes_claims_and_releases_absent_entities() {
        let (mut scene, mut view) = scene_with_pool("Player");
        let mut ocean = Ocean::new(40.0, 4, OCEAN_LEVEL);
        // A flat sea keeps every expected world Y exact: the server's
        // waterline (y = 0) must land on the drawn surface, whatever the
        // waves are doing, and the flat field makes that OCEAN_LEVEL.
        ocean.waves_mut().set_height_scale(0.0);

        // Two ships on the spawn line, our player on the first, one shell.
        let first = snapshot_at(
            vec![
                ship_entity(10, Vec3f::new(3000.0, 0.0, 0.0)),
                ship_entity(20, Vec3f::new(6000.0, 0.0, 0.0)),
                player_entity(1_000_001, "Player", Some(10)),
            ],
            vec![shell_entity(7, Vec3f::new(3100.0, 5.0, 10.0))],
        );
        view.apply(&first, &ocean);
        view.sync(&mut scene);

        assert_eq!(view.applied_snapshots(), 1);
        assert!(
            view.drawn_summary().contains("2 ships"),
            "both ships claimed a slot: {}",
            view.drawn_summary()
        );
        assert!(view.drawn_summary().contains("1 players"));
        assert!(view.drawn_summary().contains("1 projectiles"));
        // Our player's hull is the anchor: the first ship's drawn position.
        let anchor = view.anchor().expect("our hull is seen");
        assert!((anchor.x - 3000.0).abs() < 1e-4);

        // The claimed slots are un-collapsed and placed in client space.
        let (ship_position, _, ship_scale) = pool_transform(&scene, "server ship 0");
        assert!(ship_scale.x > 0.5, "a claimed slot is not parked");
        assert!((ship_position.x - 3000.0).abs() < 1e-3);
        assert!((ship_position.y - OCEAN_LEVEL).abs() < 1e-3);
        let (crew_position, _, crew_scale) = pool_transform(&scene, "server player 0");
        assert!(crew_scale.x > 0.5);
        // The server's waterline (deck y=4) lands on the drawn surface.
        assert!((crew_position.y - (4.0 + OCEAN_LEVEL)).abs() < 1e-3);
        let (shell_position, _, _) = pool_transform(&scene, "server projectile 0");
        assert!((shell_position.y - (5.0 + OCEAN_LEVEL)).abs() < 1e-3);

        // The second ship took the NEXT slot, not the same one.
        let (other_position, _, other_scale) = pool_transform(&scene, "server ship 1");
        assert!(other_scale.x > 0.5);
        assert!((other_position.x - 6000.0).abs() < 1e-3);

        // Both ships leave the world: their slots park again, and the crew
        // and shell slots park when their entities go too.
        let second = snapshot_at(
            vec![player_entity(1_000_001, "Player", Some(10))],
            Vec::new(),
        );
        view.apply(&second, &ocean);
        view.sync(&mut scene);
        let (_, _, parked) = pool_transform(&scene, "server ship 0");
        assert!(parked.x == 0.0, "a released slot parks at zero scale");
        let (_, _, parked_crew) = pool_transform(&scene, "server ship 1");
        assert!(parked_crew.x == 0.0);

        // The anchor goes with the hull: our ship is gone from view, so no
        // anchor is reported until the next snapshot places one.
        assert!(view.anchor().is_none());
    }

    #[test]
    fn the_connection_id_binds_the_anchor_even_when_names_collide() {
        let (mut scene, mut view) = scene_with_pool("Player");
        // The server answered ConnectAccept with id 7, and it names every
        // connection `player`: two crew members cannot be told apart by
        // name, so the wire id is the identity.
        view.set_connection_id(7);
        let mut ocean = Ocean::new(40.0, 4, OCEAN_LEVEL);
        ocean.waves_mut().set_height_scale(0.0);

        let first = snapshot_at(
            vec![
                ship_entity(10, Vec3f::new(3000.0, 0.0, 0.0)),
                ship_entity(20, Vec3f::new(6000.0, 0.0, 0.0)),
                player_entity_with(7, 1_000_001, "player", Some(10)),
                player_entity_with(8, 1_000_002, "player", Some(20)),
            ],
            Vec::new(),
        );
        view.apply(&first, &ocean);
        view.sync(&mut scene);

        // Ours is the hull our connection id rides — not the one a namesake
        // rides, however it is ordered in the snapshot.
        let anchor = view.anchor().expect("our hull is bound by connection id");
        assert!((anchor.x - 3000.0).abs() < 1e-4, "{anchor:?}");
    }

    #[test]
    fn without_a_connection_id_the_name_is_the_identity_and_case_blind() {
        let (mut scene, mut view) = scene_with_pool("Player");
        let mut ocean = Ocean::new(40.0, 4, OCEAN_LEVEL);
        ocean.waves_mut().set_height_scale(0.0);
        // The server lowercases its names: the menu's `Player` still binds.
        let first = snapshot_at(
            vec![
                ship_entity(10, Vec3f::new(3000.0, 0.0, 0.0)),
                player_entity_with(7, 1_000_001, "player", Some(10)),
            ],
            Vec::new(),
        );
        view.apply(&first, &ocean);
        view.sync(&mut scene);
        let anchor = view
            .anchor()
            .expect("the case-blind name binds our hull");
        assert!((anchor.x - 3000.0).abs() < 1e-4, "{anchor:?}");
    }

    #[test]
    fn a_hull_mesh_that_never_loaded_leaves_the_ship_pool_empty_but_the_rest_works() {
        let mut scene = TestScene::new("no hull");
        let mut view = ServerWorldView::attach_with(&mut scene, None, None, "Player")
            .expect("attaching without a hull mesh is not a failure");
        let ocean = Ocean::new(40.0, 4, OCEAN_LEVEL);
        let first = snapshot_at(
            vec![ship_entity(10, Vec3f::new(3000.0, 0.0, 0.0))],
            vec![shell_entity(7, Vec3f::new(3100.0, 5.0, 10.0))],
        );
        view.apply(&first, &ocean);
        // The honest summary: the ship exists but cannot be drawn; the shell
        // still can.
        assert!(
            view.drawn_summary().contains("0 ships"),
            "an empty pool must not claim a ship: {}",
            view.drawn_summary()
        );
        assert!(view.drawn_summary().contains("1 projectiles"));
    }
}
