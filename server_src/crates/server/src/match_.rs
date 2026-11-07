//! The match: ship IDs and their spawn line, world loading, the entity
//! snapshot of a ship (`TrackedShip`), and player spawn/despawn bookkeeping.
//!
//! Split out of the binary (plan H5) so tools can drive the same code the
//! server runs, instead of reimplementing what a "player" or a "tracked ship"
//! is.

use anyhow::{Context, Result};
use rfs_core::entity::{
    BulkheadEntity, CompartmentEntity, EntityFlags, EntityId, HitMark, PlayerEntity, ShipEntity,
    StationEntity,
};
use rfs_core::math::{Bounds, Quatf, Transform, Vec3f};
use rfs_core::packet::{CommandPacket, GameEvent as CoreGameEvent, PlayerPosture, ServerCommand};
use rfs_net::*;
use rfs_ship::loader::{create_default_destroyer, create_default_frigate, ShipLoader};
use rfs_ship::ship::{Ship, ShipState};
use rfs_sim::map::WorldMap;
use rfs_sim::tick::TickSystem;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

pub const SHIP_ID: u64 = 10;
pub const SHIP_ID_STRIDE: u64 = 10_000;
pub const SHIP_SPACING: f32 = 3000.0;
const PLAYER_ID_BASE: u64 = 1_000_000;
const PROJECTILE_ID_BASE: u64 = 5_000_000;

/// Name used in logs for a generated world, so a line in the log says which of
/// the two paths produced it.
const PROCEDURAL_MAP_NAME: &str = "open-sea";
const PROCEDURAL_MAP_SEED: u64 = 1;
/// Half-extent of a 50x50 km world — the size the architecture note names.
const DOCUMENTED_MAP_HALF_EXTENT: f32 = 25_000.0;
/// Room kept between the last spawn and the map face, so the boundary test in
/// the tick has something to work with instead of clamping on frame one.
const MAP_EDGE_MARGIN: f32 = 5_000.0;

/// Reads the value that follows a flag: `--map foo.json` gives `Some("foo.json")`.
///
/// Returns `None` for a flag at the end of the line with no value, which is a
/// typo rather than an absent flag, so the caller says so instead of quietly
/// using a default.
pub fn arg_value(args: &[String], flag: &str) -> Option<String> {
    let index = args.iter().position(|a| a == flag)?;
    args.get(index + 1).cloned()
}

/// How many ships a match runs with when `--ships` is not given.
///
/// Bug №609 (analysis 3.7.3/4.1): the default was 1. The declared scenario is
/// 15-40 ships, so running the server with no arguments produced a match that
/// the design never described, and the only way to reach the real one was to
/// know a flag. A default that does not reproduce the documented scenario makes
/// the documentation untested. The low end of the declared range is the honest
/// choice: it is a number the plan names, and `--ships` still moves it.
pub const DEFAULT_SHIP_COUNT: usize = 15;
/// The declared ceiling; also the clamp, so an absurd `--ships 1000` is refused
/// rather than silently generating a 3 000 km spawn line.
pub const MAX_SHIP_COUNT: usize = 64;

/// Resolves the ship count from the command line, or the default.
///
/// Bug №609: `--ships` with no value, or with junk, silently fell back to 1 —
/// the same number as "flag absent", so a typo looked exactly like the
/// default. Both are reported instead, because a match that runs one ship when
/// the operator asked for forty looks like the engine ignoring them.
pub fn resolve_ship_count(args: &[String]) -> Result<usize> {
    // The flag being present with nothing after it is a typo, not an absent
    // flag: `arg_value` cannot tell the two apart on its own, and treating the
    // typo as "use the default" is how a mistyped flag silently produces the
    // default match.
    if !args.iter().any(|a| a == "--ships") {
        return Ok(DEFAULT_SHIP_COUNT);
    }
    let raw = arg_value(args, "--ships").with_context(|| "`--ships` was given with no value")?;
    let parsed = raw
        .parse::<usize>()
        .with_context(|| format!("`--ships {raw}` is not a number"))?;
    if parsed == 0 || parsed > MAX_SHIP_COUNT {
        anyhow::bail!("`--ships {parsed}` is outside 1..={MAX_SHIP_COUNT}");
    }
    Ok(parsed)
}

/// Half-extent of a generated world, derived from the spawn line rather than
/// fixed.
///
/// The spawn line is `ship_count * SHIP_SPACING` long, which reaches 189 km at
/// the `--ships 64` limit. A constant radius would leave the last ships outside
/// the world, where the tick's boundary clamp stacks them on the face — a bug
/// that looks like a physics problem and is not one. Small matches get the
/// documented 50x50 km instead, so the generated world matches the architecture
/// note whenever the layout allows it.
pub fn world_radius_for(ship_count: usize) -> f32 {
    let needed = ship_count as f32 * SHIP_SPACING * 0.5 + MAP_EDGE_MARGIN;
    needed.max(DOCUMENTED_MAP_HALF_EXTENT)
}

/// Loads the world the match runs in.
///
/// Two paths, and the log line at startup says which one produced the map:
/// an authored file (`--map <path>`) or a generated one from `--map-seed`.
/// There is no silent fallback. A match that runs in an empty universe because
/// a path was wrong looks exactly like a match where nothing happens, and the
/// first one costs an hour of looking at the wrong subsystem.
pub fn load_world(args: &[String], ship_count: usize) -> Result<WorldMap> {
    if let Some(path) = arg_value(args, "--map") {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read map file `{path}`"))?;
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        let world = WorldMap::from_json(&name, &text)
            .with_context(|| format!("map file `{path}` is not usable"))?;
        info!(
            "world: `{}` loaded from {}, obstacles={}, bounds={:?}..{:?}",
            world.name,
            path,
            world.obstacles.len(),
            world.bounds.min,
            world.bounds.max
        );
        return Ok(world);
    }

    let seed = match arg_value(args, "--map-seed") {
        Some(raw) => raw
            .parse::<u64>()
            .with_context(|| format!("`--map-seed {raw}` is not a number"))?,
        None => PROCEDURAL_MAP_SEED,
    };
    let radius = world_radius_for(ship_count);
    let world = WorldMap::open_sea(PROCEDURAL_MAP_NAME, seed, radius)
        .context("generated world failed its own validation")?;
    info!(
        "world: `{}` generated, seed={}, obstacles={}, bounds={:?}..{:?}",
        world.name,
        seed,
        world.obstacles.len(),
        world.bounds.min,
        world.bounds.max
    );
    Ok(world)
}

/// The class id every ship of the match is built from (the built-in frigate).
pub const DEFAULT_SHIP_CLASS_ID: u32 = 1;

/// Bug №667: classes are data, not code. The loader is the single place a
/// `ShipClass` becomes a ship. Disk-first: `ships/` next to the config (JSON
/// classes) is treated like `--map` — the directory exists only for classes
/// actually to load, a broken file is a hard stop, not a silently shipped
/// default. No usable ships directory (or one without class 1) registers the
/// built-in frigate/destroyer instead, so `create_ship` stays the one spawn
/// door and the log line says which side fed it.
pub fn load_ship_classes(config_dir: Option<&std::path::Path>) -> Result<ShipLoader> {
    let ships_dir = config_dir.map(|dir| dir.join("ships"));
    let mut loader = ShipLoader::new(
        &ships_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "ships".to_string()),
    );

    if let Some(dir) = ships_dir.as_deref() {
        if dir.exists() {
            loader
                .load_all()
                .with_context(|| format!("ship classes from `{}`", dir.display()))?;
            if loader.get_class(DEFAULT_SHIP_CLASS_ID).is_none() {
                warn!(
                    "`{}` has no class {DEFAULT_SHIP_CLASS_ID}; built-in frigate/destroyer registered too",
                    dir.display()
                );
                loader.register_class(create_default_frigate());
                loader.register_class(create_default_destroyer());
            }
            info!(
                "ship classes: {} registered from {}",
                loader.list_classes().len(),
                dir.display()
            );
            return Ok(loader);
        }
    }
    info!("no usable ships directory; built-in frigate/destroyer registered");
    loader.register_class(create_default_frigate());
    loader.register_class(create_default_destroyer());
    Ok(loader)
}

/// Whether a spawn point is inside a solid volume.
///
/// Reported, not repaired. Ships do not collide with map obstacles yet, so
/// moving the spawn would hide the fact rather than fix anything; the useful
/// output is the line saying the starting position is already inside a rock.
pub fn spawn_is_blocked(world: &WorldMap, point: Vec3f) -> Option<u64> {
    world
        .obstacles
        .iter()
        .find(|obstacle| obstacle.bounds.contains(point))
        .map(|obstacle| obstacle.id)
}

static NEXT_PROJECTILE_ID: AtomicU64 = AtomicU64::new(PROJECTILE_ID_BASE);

pub fn new_projectile_id() -> EntityId {
    EntityId::new(NEXT_PROJECTILE_ID.fetch_add(1, Ordering::Relaxed))
}

pub struct CompartmentStatic {
    name: String,
    local_bounds: Bounds,
    max_water_level: f32,
    pump_capacity: f32,
    connected_compartments: Vec<EntityId>,
    bulkheads: Vec<BulkheadEntity>,
}

pub struct StationStatic {
    station_type: rfs_core::entity::StationType,
    local_transform: Transform,
    compartment_id: EntityId,
    max_ammo: u32,
    max_health: f32,
    /// Bug №85: the real reload-cycle time, not `max_ammo`.
    reload_time: f32,
}

pub struct TrackedShip {
    entity_id: EntityId,
    class_id: u32,
    max_health: f32,
    max_fuel: f32,
    max_speed: f32,
    /// Static compartment tables, keyed by entity id; the per-tick values are
    /// filled from the live `ShipState`.
    pub compartments: HashMap<EntityId, CompartmentStatic>,
    pub stations: HashMap<EntityId, StationStatic>,
}

impl TrackedShip {
    pub fn capture(ship: &Ship) -> Self {
        let config = ship.config();
        let state = ship.get_state();
        let mut compartments = HashMap::new();
        for (cid, cstate) in &state.compartment_states {
            let compartment = ship.get_compartment(*cid);
            compartments.insert(
                *cid,
                CompartmentStatic {
                    name: compartment
                        .map(|c| c.config().name.clone())
                        .unwrap_or_default(),
                    local_bounds: compartment
                        .map(|c| c.bounds())
                        .unwrap_or_else(|| Bounds::new(Vec3f::ZERO, Vec3f::ZERO)),
                    max_water_level: cstate.max_water_level,
                    pump_capacity: cstate.pump_capacity,
                    connected_compartments: cstate.connected_compartments.clone(),
                    bulkheads: cstate
                        .bulkhead_states
                        .iter()
                        .map(|b| BulkheadEntity {
                            entity_id: b.entity_id,
                            compartment_a: *cid,
                            compartment_b: b.connects_to,
                            local_position: b.local_position,
                            is_sealed: b.is_sealed,
                            is_destroyed: b.is_destroyed,
                            seal_strength: b.seal_strength,
                        })
                        .collect(),
                },
            );
        }
        let mut stations = HashMap::new();
        for (sid, sstate) in &state.station_states {
            let station = ship.get_station(*sid);
            stations.insert(
                *sid,
                StationStatic {
                    station_type: sstate.station_type,
                    local_transform: station
                        .map(|s| s.local_transform())
                        .unwrap_or(Transform::IDENTITY),
                    compartment_id: station
                        .map(|s| s.compartment_id())
                        .unwrap_or(EntityId::nil()),
                    max_ammo: sstate.max_ammo,
                    max_health: sstate.max_health,
                    reload_time: station.map(|s| s.config().reload_time).unwrap_or(0.0),
                },
            );
        }
        Self {
            entity_id: ship.entity_id(),
            class_id: ship.class_id(),
            max_health: config.max_health,
            max_fuel: config.max_fuel,
            max_speed: config.max_speed,
            compartments,
            stations,
        }
    }

    pub fn ship_entity(&self, state: &ShipState) -> ShipEntity {
        ShipEntity {
            entity_id: self.entity_id,
            ship_class_id: self.class_id,
            transform: state.transform,
            velocity: state.velocity,
            angular_velocity: state.angular_velocity,
            health: state.health,
            max_health: self.max_health,
            flags: EntityFlags::NONE,
            fuel: state.fuel,
            max_fuel: self.max_fuel,
            speed: state.speed,
            max_speed: self.max_speed,
            heading: state.heading,
            rudder_angle: state.rudder_angle,
            throttle: state.throttle,
            half_size: Vec3f::new(50.0, 30.0, 150.0),
            compartments: self.compartment_entities(state),
            stations: self.station_entities(state),
            team: 0,
            // Bug №272: the pending hit mark rides in layer 0, so the impact
            // effect and the struck-compartment feedback survive a lost
            // `ShipHit` event (events are unreliable by design). `note_hit`
            // stores a ship::HitMark with a u64 tick; the wire type carries the
            // same value narrowed to u32, matching `spawn_tick`/`server_tick`
            // elsewhere in the protocol.
            last_hit: state.last_hit.map(|h| HitMark {
                hit_tick: h.tick as u32,
                position: h.position,
                normal: h.normal,
                compartment: h.compartment,
                damage: h.damage,
            }),
        }
    }

    fn compartment_entities(&self, state: &ShipState) -> Vec<CompartmentEntity> {
        self.compartments
            .iter()
            .map(|(cid, cs)| {
                let live = state.compartment_states.get(cid);
                let station_ids: Vec<EntityId> = self
                    .stations
                    .iter()
                    .filter(|(_, ss)| ss.compartment_id == *cid)
                    .map(|(sid, _)| *sid)
                    .collect();
                // World frame, derived once per tick from the ship transform
                // (plan §8.1). Extents stay axis-aligned (see world_bounds docs).
                let world_center = state.transform.transform_point(cs.local_bounds.center());
                let world_bounds = Bounds::new(
                    world_center + (cs.local_bounds.min - cs.local_bounds.center()),
                    world_center + (cs.local_bounds.max - cs.local_bounds.center()),
                );
                CompartmentEntity {
                    entity_id: *cid,
                    ship_id: self.entity_id,
                    name: cs.name.clone(),
                    local_bounds: cs.local_bounds,
                    world_bounds,
                    water_level: live.map_or(0.0, |c| c.water_level),
                    max_water_level: cs.max_water_level,
                    is_sealed: live.is_some_and(|c| c.is_sealed),
                    is_breached: live.is_some_and(|c| c.is_breached),
                    fire_intensity: live.map_or(0.0, |c| c.fire_intensity),
                    // Bug №61: expose pump activity to replication.
                    pump_active: live.is_some_and(|c| c.pump_active),
                    connected_compartments: cs.connected_compartments.iter().copied().collect(),
                    stations: station_ids.into(),
                    pump_capacity: cs.pump_capacity,
                    bulkheads: cs.bulkheads.clone().into(),
                }
            })
            .collect()
    }

    fn station_entities(&self, state: &ShipState) -> Vec<StationEntity> {
        self.stations
            .iter()
            .map(|(sid, ss)| {
                let live = state.station_states.get(sid);
                // World frame, derived once per tick (plan §8.1).
                let world_transform = Transform::new(
                    state.transform.transform_point(ss.local_transform.position),
                    state.transform.rotation * ss.local_transform.rotation,
                    ss.local_transform.scale,
                );
                StationEntity {
                    entity_id: *sid,
                    ship_id: self.entity_id,
                    compartment_id: ss.compartment_id,
                    station_type: ss.station_type,
                    local_transform: ss.local_transform,
                    world_transform,
                    occupant: live.and_then(|s| s.occupant),
                    yaw: live.map_or(0.0, |s| s.yaw),
                    pitch: live.map_or(0.0, |s| s.pitch),
                    reload_progress: live.map_or(0.0, |s| s.reload_progress),
                    ammo_type: live.map_or(0, |s| s.ammo_type),
                    ammo_count: live.map_or(0, |s| s.ammo_count),
                    max_ammo: ss.max_ammo,
                    is_operational: live.is_some_and(|s| s.is_operational),
                    health: live.map_or(0.0, |s| s.health),
                    max_health: ss.max_health,
                    cooldown: live.map_or(0.0, |s| s.cooldown),
                    // Bug №85: the cooldown gauge reflects the real reload
                    // cycle (seconds), not the magazine size.
                    max_cooldown: ss.reload_time,
                }
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct PlayerMeta {
    pub player_entity_id: EntityId,
    pub ship_id: EntityId,
    pub name: String,
    pub spawn_local: Vec3f,
}

pub fn spawn_player(net: &NetServer, ship: &Ship, connection_id: u32, name: &str) -> PlayerMeta {
    let player_entity_id = EntityId::new(PLAYER_ID_BASE + connection_id as u64);
    let spawn_local = Vec3f::new(0.0, 4.0, -8.0);
    let player = PlayerEntity {
        entity_id: player_entity_id,
        player_id: connection_id,
        name: name.to_string(),
        team: 0,
        transform: Transform::new(
            ship.local_to_world(spawn_local),
            Quatf::IDENTITY,
            Vec3f::ONE,
        ),
        velocity: ship.velocity(),
        posture: PlayerPosture::Standing,
        health: 100.0,
        max_health: 100.0,
        stamina: 100.0,
        max_stamina: 100.0,
        current_station: None,
        current_ship: Some(ship.entity_id()),
        input_sequence: 0,
        last_acknowledged_tick: 0,
    };
    net.add_player(player.clone(), ship.entity_id());
    net.broadcast_event(CoreGameEvent::PlayerSpawned {
        player: player_entity_id,
        ship: ship.entity_id(),
        position: player.transform.position,
    });
    PlayerMeta {
        player_entity_id,
        ship_id: ship.entity_id(),
        name: name.to_string(),
        spawn_local,
    }
}

/// Bug №71: where a client's input applies — or `None` when it applies
/// nowhere.
///
/// The client cleans its pending-input map only on an ack, so an input that
/// the server drops in silence leaves the entry pending forever and the map
/// grows. The caller answers EVERY input; this function is what tells it
/// whether the input was applied or rejected, and the two cases must not
/// look alike.
pub fn resolve_input_target(
    players: &HashMap<u32, PlayerMeta>,
    sim: &TickSystem,
    connection_id: u32,
) -> Option<(EntityId, Arc<Ship>)> {
    let meta = players.get(&connection_id)?;
    let ship = sim.get_ship(meta.ship_id)?;
    Some((meta.player_entity_id, ship))
}

pub fn despawn_player(
    sim: &TickSystem,
    net: &NetServer,
    connection_id: u32,
    players: &mut HashMap<u32, PlayerMeta>,
) {
    if let Some(meta) = players.remove(&connection_id) {
        net.remove_player(connection_id, meta.player_entity_id);
        // Bug №58: vacate any station the leaving player still occupies —
        // otherwise the occupant dangles on a dead player forever, the
        // station is blocked and the ghost occupant gets replicated.
        if let Some(ship) = sim.get_ship(meta.ship_id) {
            // Vacate ALL stations held by the leaving player (one player
            // must never hold N stations, but stale states could).
            let occupied: Vec<EntityId> = ship
                .get_state()
                .station_states
                .iter()
                .filter(|(_, s)| s.occupant == Some(meta.player_entity_id))
                .map(|(id, _)| *id)
                .collect();
            for sid in occupied {
                ship.vacate_station(sid);
                // Bug №153: broadcast the real vacate event, not nothing.
                net.broadcast_event(CoreGameEvent::StationVacated {
                    station: sid,
                    player: meta.player_entity_id,
                });
            }
        }
    }
}

/// Bug №701: every member of a sinking crew is dead the moment the hull's
/// loss became certain — not when the wreck is finally removed (№57, up to
/// 30 s later).
///
/// The sim's event names only the ship (`ShipSinkingStarted`): players do not
/// exist in the sim, so it cannot name them. This is the one place that knows
/// the binding, and it fans out one honest death per member: `killer` stays
/// `None` — which hull or gun started the fire is a scoring question, and the
/// score ledger is separate.
///
/// Lives beside `handle_respawn`, not inside `core_game_event`'s pure
/// translation: the translator gets one event in and one event out, and this
/// is one-to-many.
pub fn broadcast_player_deaths(
    net: &NetServer,
    players: &HashMap<u32, PlayerMeta>,
    ship: EntityId,
) {
    for meta in players.values().filter(|meta| meta.ship_id == ship) {
        net.broadcast_event(CoreGameEvent::PlayerDied {
            player: meta.player_entity_id,
            killer: None,
        });
    }
}

/// Bug №701: what `Respawn` costs and where exactly the player returns is the
/// owner's to define later (plan §20.7 leaves it open); until then the
/// command can decide one thing honestly — a player whose binding is destroyed
/// is put back under a live hull.
///
/// Routes HERE, not through `translate_command`: a respawned player's ship is
/// by definition gone (№57 removed the wreck) or going under, so the per-ship
/// path would answer "ship not ready" before the request was even read. Like
/// VoteKick, this is a server-wide command and sits at this glue's level.
pub fn handle_respawn(
    net: &NetServer,
    sim: &TickSystem,
    connection_id: u32,
    command: &CommandPacket,
    players: &mut HashMap<u32, PlayerMeta>,
) {
    let acknowledge = |success: bool, reason: Option<String>| {
        net.send_command_ack(connection_id, command.command_id, success, reason);
    };

    let ServerCommand::Respawn { player: claimed } = &command.command else {
        acknowledge(
            false,
            Some("respawn glue received a non-respawn command".into()),
        );
        return;
    };

    let Some(meta) = players.get(&connection_id) else {
        acknowledge(false, Some("no player to respawn".into()));
        return;
    };
    // The command's `player` field is client-authored, like ChatMessage's was
    // (№634): the connection decides whose player it is, the payload does not.
    if *claimed != meta.player_entity_id {
        acknowledge(false, Some("the command names a different player".into()));
        return;
    }

    // Death is exact: the binding is gone (the №57 removal took the ship) or
    // the hull no longer keeps anyone afloat (`check_sinking` flipped the
    // flag; health ≤ 0 is the same statement earlier in the same update).
    // Either way the player is not steering anything, and no ping of the hull
    // matters — repairs cannot bring back what the point of no return already
    // fixed.
    let dead = sim
        .get_ship(meta.ship_id)
        .map(|ship| !ship.is_operational())
        .unwrap_or(true);
    if !dead {
        acknowledge(false, Some("player is not dead".into()));
        return;
    }

    let Some(target) = sim.first_alive_ship() else {
        acknowledge(false, Some("nothing to respawn to".into()));
        return;
    };

    // The old binding ends the way a disconnect ends it — the same despawn
    // path, so every rule it already owns (net cleanup, the №58 station
    // vacate, the broadcasts) holds without a second opinion about what
    // "leave" means. Then the same `spawn_player` that opened the session
    // rebinds: the crew broadcast the ServerSpawned, the players map is
    // rewritten, and the same entity id returns because it is derived from the
    // connection, not from the binding.
    let name = meta.name.clone();
    despawn_player(sim, net, connection_id, players);
    let fresh = spawn_player(net, &target, connection_id, &name);
    players.insert(connection_id, fresh);

    acknowledge(true, None);
}

/// Bug №59: the current helmsman — the player occupying a Helm station.
pub fn current_helm_occupant(ship: &Ship) -> Option<EntityId> {
    ship.get_state()
        .station_states
        .values()
        .find(|s| s.station_type == rfs_core::entity::StationType::Helm && s.occupant.is_some())
        .and_then(|s| s.occupant)
}

#[cfg(test)]
mod input_target_tests {
    //! Bug №71: an input that the server cannot apply must be REJECTED, not
    //! dropped in silence. The client frees its pending-input entry only on
    //! an ack, so a silent drop grows that map without bound.

    use super::*;
    use rfs_damage::damage::DamageSystem;
    use rfs_ship::ship::ShipConfig;

    fn meta(ship_id: EntityId) -> PlayerMeta {
        PlayerMeta {
            player_entity_id: EntityId::new(7),
            ship_id,
            name: "tester".into(),
            spawn_local: Vec3f::ZERO,
        }
    }

    fn sim_with_ship(entity_id: EntityId) -> TickSystem {
        let sim = TickSystem::new();
        sim.add_ship(Ship::new(
            ShipConfig::default(),
            entity_id,
            Arc::new(DamageSystem::new()),
        ));
        sim
    }

    #[test]
    fn an_unknown_client_has_no_input_target() {
        let sim = TickSystem::new();
        let mut players = HashMap::new();
        players.insert(1u32, meta(EntityId::new(99)));
        assert!(
            resolve_input_target(&players, &sim, 2).is_none(),
            "an unknown client must be rejected, not silently accepted"
        );
    }

    #[test]
    fn a_client_whose_ship_is_gone_has_no_input_target() {
        let sim = TickSystem::new();
        let mut players = HashMap::new();
        players.insert(1u32, meta(EntityId::new(99)));
        assert!(
            resolve_input_target(&players, &sim, 1).is_none(),
            "a client whose ship no longer exists must be rejected"
        );
    }

    #[test]
    fn a_known_client_with_a_ship_has_a_target() {
        let sim = sim_with_ship(EntityId::new(42));
        let mut players = HashMap::new();
        players.insert(1u32, meta(EntityId::new(42)));
        let (player_id, ship) = resolve_input_target(&players, &sim, 1)
            .expect("a known client with a live ship must have a target");
        assert_eq!(player_id, EntityId::new(7));
        assert_eq!(ship.entity_id(), EntityId::new(42));
    }
}

/// Bug №609 (analysis 3.7.3/4.1, plan step 3): the ship count used to be
/// `…unwrap_or(1)`, so the default match was one ship while the declared
/// scenario is 15-40. These pin both halves of the decision: the default has to
/// be a number the plan names, and a malformed flag must not be indistinguishable
/// from an absent one — the old code turned `--ships banana` into the same match
/// as no flag at all.
#[cfg(test)]
mod ship_count_args {
    use super::{
        resolve_ship_count, world_radius_for, DEFAULT_SHIP_COUNT, MAX_SHIP_COUNT, SHIP_SPACING,
    };

    fn args(parts: &[&str]) -> Vec<String> {
        std::iter::once("server".to_string())
            .chain(parts.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn no_flag_gives_a_count_the_plan_declares() {
        let count = resolve_ship_count(&args(&[])).expect("the default is a number");
        assert_eq!(count, DEFAULT_SHIP_COUNT);
        assert!(
            (15..=40).contains(&count),
            "the default {count} is outside the declared 15-40 range, so the \
             documented scenario is still not what a bare run produces"
        );
    }

    #[test]
    fn an_explicit_count_is_honoured() {
        assert_eq!(
            resolve_ship_count(&args(&["--ships", "40"])).expect("40 is valid"),
            40
        );
        assert_eq!(
            resolve_ship_count(&args(&["--ships", "1"])).expect("1 is valid"),
            1,
            "asking for one ship must still be possible"
        );
    }

    /// The flag is a claim about the match. Silently running a different match
    /// than the one that was asked for is the defect, so every unusable value is
    /// refused loudly.
    #[test]
    fn an_unusable_count_is_refused_instead_of_defaulted() {
        for bad in [
            args(&["--ships"]),
            args(&["--ships", "banana"]),
            args(&["--ships", "-3"]),
            args(&["--ships", "0"]),
            args(&["--ships", "1000"]),
        ] {
            assert!(
                resolve_ship_count(&bad).is_err(),
                "{bad:?} was accepted as a ship count"
            );
        }
    }

    /// The world is sized from the count, so a count the plan does not allow
    /// must not be able to generate a 3 000 km spawn line. This ties the two
    /// decisions together instead of trusting the clamp to still hold later.
    #[test]
    fn the_world_is_big_enough_for_the_accepted_range() {
        let radius = world_radius_for(MAX_SHIP_COUNT);
        assert!(
            radius >= MAX_SHIP_COUNT as f32 * SHIP_SPACING * 0.5,
            "at the ceiling the spawn line runs past the map face: {radius} m"
        );
    }
}

/// Bug №665: the world generator and the open_sea helper diverged — the helper
/// documented the plan's open sea and nobody called it, while the loader went
/// straight to the procedural layer with the same zero-obstacle count. Pinning
/// the loader to the helper keeps one voice for "open sea" and makes the
/// helper a live part of the path instead of a description nobody follows.
#[cfg(test)]
mod load_world_args {
    use super::load_world;

    fn args(parts: &[&str]) -> Vec<String> {
        std::iter::once("server".to_string())
            .chain(parts.iter().map(|s| s.to_string()))
            .collect()
    }

    /// The generated world is the open sea the plan names: no obstacles at
    /// the default seed, and the log name matches the world name.
    #[test]
    fn a_generated_world_is_open_sea() {
        let world = load_world(&args(&[]), 20).expect("the defaults build a world");
        assert_eq!(world.name, "open-sea");
        assert!(world.obstacles.is_empty(), "a generated world has land");
    }

    /// An explicit seed keeps working even though it changes nothing about the
    /// emptiness: the world stays deterministic and open at any seed.
    #[test]
    fn an_explicit_seed_builds_the_same_empty_world() {
        let world =
            load_world(&args(&["--map-seed", "12345"]), 20).expect("an explicit seed is honoured");
        assert!(world.obstacles.is_empty());
    }

    /// A seed that is not a number is refused loudly, never defaulted.
    #[test]
    fn a_non_numeric_seed_is_refused() {
        let bad = load_world(&args(&["--map-seed", "banana"]), 20);
        assert!(bad.is_err(), "a non-numeric seed was silently accepted");
    }
}

/// Bug №667: no ships directory is the "no flag" case — the built-in
/// frigate/destroyer are registered instead of failing the start, and the
/// registry still answers for both ids.
#[cfg(test)]
mod ship_classes_args {
    use super::{load_ship_classes, DEFAULT_SHIP_CLASS_ID};

    /// The bare-run path: no config dir at all, the built-ins answer.
    #[test]
    fn builtins_fill_in_when_the_ships_dir_is_missing() {
        let loader = load_ship_classes(None).expect("missing dir is the fallback path");
        assert!(
            loader.get_class(DEFAULT_SHIP_CLASS_ID).is_some(),
            "the frigate must answer without any ships directory"
        );
        assert!(
            loader.get_class(2).is_some(),
            "the destroyer fallback survived too"
        );
        assert_eq!(
            loader.list_classes().len(),
            2,
            "only the built-in pair may show up when no disk classes exist"
        );
    }

    /// A class the registry does not know cannot build a ship — this is the
    /// refusal `create_ship` makes, shown from the load path.
    #[test]
    fn an_unknown_class_cannot_build() {
        let loader = load_ship_classes(None).expect("missing dir is the fallback path");
        assert!(
            loader
                .create_ship(
                    99,
                    rfs_core::entity::EntityId::new(1),
                    std::sync::Arc::new(rfs_damage::damage::DamageSystem::new())
                )
                .is_err(),
            "an unknown class id must be refused"
        );
    }
}

/// Bug №701: respawn has to be honest in both directions — a dead binding
/// rebinds onto a floating hull, a live one refuses, a sea without hulls
/// refuses. The glue runs against a real `NetServer` on an OS-allocated port:
/// with no client attached, the acks and broadcasts fan out to nobody, which
/// is a no-op second opinion rather than a test double that could drift from
/// the production path.
#[cfg(test)]
mod respawn_tests {
    use super::*;

    use rfs_net::{NetServer, ServerConfig};

    fn meta(player: u64, ship_id: u64) -> PlayerMeta {
        PlayerMeta {
            player_entity_id: EntityId::new(player),
            ship_id: EntityId::new(ship_id),
            name: "tester".into(),
            spawn_local: Vec3f::ZERO,
        }
    }

    /// The entity id a real join gets: it is derived from the connection, so
    /// a respawned player's id is the one they had before the loss — the test
    /// meta must claim exactly that, not a convenient small number.
    fn real_player_id(connection_id: u32) -> EntityId {
        EntityId::new(PLAYER_ID_BASE + u64::from(connection_id))
    }

    fn respawn_packet(connection_id: u32) -> CommandPacket {
        CommandPacket {
            command_id: 7,
            command: ServerCommand::Respawn {
                player: real_player_id(connection_id),
            },
            // Bug №216: the server crate never opens command packets itself —
            // the session echo belongs to the transport, not to commands.
            session: 0,
        }
    }

    fn hittable_hull(entity_id: EntityId) -> Ship {
        Ship::new(
            rfs_ship::ship::ShipConfig {
                max_health: 1000.0,
                ..Default::default()
            },
            entity_id,
            Arc::new(rfs_damage::damage::DamageSystem::new()),
        )
    }

    async fn server() -> NetServer {
        NetServer::new(ServerConfig {
            bind_addr: "127.0.0.1:0".parse().expect("a loopback address"),
            ..ServerConfig::default()
        })
        .await
        .expect("server bind")
    }

    /// A dead binding (the hull no 57 took away) is rebindable: the old
    /// binding ends through the same despawn path as anything else, and the
    /// same spawn path that opened the session rebinds under the one hull
    /// still floating.
    #[tokio::test]
    async fn a_dead_player_rebinds_onto_the_floating_hull() {
        let net = server().await;
        let sim = TickSystem::new();
        let floating = EntityId::new(SHIP_ID);
        sim.add_ship(hittable_hull(floating));
        let mut players = HashMap::new();
        // The binding points at a hull the sim does not have: the dead half.
        players.insert(1, meta(real_player_id(1).0, 999));

        handle_respawn(&net, &sim, 1, &respawn_packet(1), &mut players);

        let rebound = players.get(&1).expect("the player survived the respawn");
        assert_eq!(
            rebound.ship_id, floating,
            "the only floating hull is the fallback target"
        );
        assert_eq!(
            rebound.player_entity_id,
            real_player_id(1),
            "the id belongs to the connection, not to the binding it lost"
        );
        assert_eq!(rebound.name, "tester", "the loss must not change the name");
    }

    /// A binding that is still steering an operational hull is NOT dead, and
    /// respawn has no reason to exist for it — the refusal must leave the
    /// binding untouched, because a silent rebind would be a kick, not a
    /// respawn.
    #[tokio::test]
    async fn a_live_player_is_refused_and_stays_bound() {
        let net = server().await;
        let sim = TickSystem::new();
        let hull = EntityId::new(SHIP_ID);
        sim.add_ship(hittable_hull(hull));
        let mut players = HashMap::new();
        players.insert(1, meta(real_player_id(1).0, hull.0));

        handle_respawn(&net, &sim, 1, &respawn_packet(1), &mut players);

        let started = players.get(&1).expect("the player stayed");
        assert_eq!(
            started.ship_id, hull,
            "a live player must not be silently rebound"
        );
        assert_eq!(started.name, "tester");
    }

    /// A sea without floating hulls cannot put anyone under any roof: the
    /// refusal names it, and nothing about the binding moves — the meta keeps
    /// its last known binding so a later respawn still resolves against it.
    #[tokio::test]
    async fn a_world_without_hulls_refuses() {
        let net = server().await;
        let sim = TickSystem::new();
        let mut players = HashMap::new();
        players.insert(1, meta(real_player_id(1).0, 999));

        handle_respawn(&net, &sim, 1, &respawn_packet(1), &mut players);

        let kept = players
            .get(&1)
            .expect("the binding is untouched on refusal");
        assert_eq!(
            kept.ship_id,
            EntityId::new(999),
            "a refused respawn must not invent a binding either"
        );
    }
}
