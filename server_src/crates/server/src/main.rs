use anyhow::{Context, Result};
use rfs_core::entity::{
    BulkheadEntity, CompartmentEntity, EntityFlags, EntityId, HitMark, PlayerEntity, ProjectileEntity,
    ShipEntity, StationEntity, StationType,
};
use rfs_core::math::{Bounds, Quatf, Transform, Vec3f};
use rfs_core::packet::{
    CommandPacket, GameEvent as CoreGameEvent, InputAckPacket, InputPacket, PlayerPosture,
    ServerCommand, StationAction, StationInteraction,
};
use rfs_core::time::TICK_DURATION_MS;
use rfs_damage::damage::DamageSystem;
use rfs_net::*;
use rfs_ship::loader::create_default_frigate;
use rfs_ship::ship::{Ship, ShipConfig, ShipState};
use rfs_ship::station::{FireResult, Station};
use rfs_sim::ballistics::ProjectileConfig;
use rfs_sim::map::WorldMap;
use rfs_sim::tick::{GameEvent as SimGameEvent, Projectile, TickSystem};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

const SHIP_ID: u64 = 10;
const SHIP_ID_STRIDE: u64 = 10_000;
const SHIP_SPACING: f32 = 3000.0;
const PLAYER_ID_BASE: u64 = 1_000_000;
const PROJECTILE_ID_BASE: u64 = 5_000_000;
/// Bug №604 (analysis 3.1.3): the repair amount arrives as an `f32` from the
/// trusted-ish client. `f32::INFINITY` used to sail through `.max(0.0)` and
/// `.min(max_health)`, turning one packet into a full repair. One answer for
/// both paths: the command path and the input path clamp to the same ceiling.
const MAX_REPAIR_PER_ACTION: f32 = 25.0;

/// Name used in logs for a generated world, so a line in the log says which of
/// the two paths produced it.
const PROCEDURAL_MAP_NAME: &str = "open-sea";
const PROCEDURAL_MAP_SEED: u64 = 1;
/// **Zero.**
///
/// `TODO.md` part G, «Погода, мир, обнаружение»: the map is 50×50 km of open
/// sea with no land. A world generator that emits nothing looked unfinished, so
/// the default was filled with 256 drifting rocks - from a project with no rocks
/// in it. Structures exist because the same section says hard obstacles cast a
/// radar shadow, but the server has no reason to invent any.
///
/// A map that has them is a map file: `--map path.json`.
const PROCEDURAL_MAP_OBSTACLES: usize = 0;
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
fn arg_value(args: &[String], flag: &str) -> Option<String> {
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
const DEFAULT_SHIP_COUNT: usize = 15;
/// The declared ceiling; also the clamp, so an absurd `--ships 1000` is refused
/// rather than silently generating a 3 000 km spawn line.
const MAX_SHIP_COUNT: usize = 64;

/// Counts ticks the interval skipped, which it does silently by design.
///
/// Bug №614 (analysis 3.6.4): `MissedTickBehavior::Skip` drops a tick whose
/// deadline passed while the loop was busy, and says nothing. The only evidence
/// was `loop avg` sitting above budget — which cannot separate "the iteration
/// is slow" from "the iteration never happened". A server silently running at
/// 23 Hz against a 30 Hz contract looks like a network problem to every client
/// and like nothing at all in the log.
///
/// `Interval::tick()` hands back the *scheduled* deadline even when it is
/// already late, so the expected count is the deadline's distance from the first
/// one, and the skipped count is whatever the loop did not run. No wall-clock
/// guessing: the answer comes from the same clock the scheduler uses.
struct TickCadence {
    origin: Option<tokio::time::Instant>,
    ran: u64,
    skipped: u64,
}

impl TickCadence {
    fn new() -> Self {
        Self {
            origin: None,
            ran: 0,
            skipped: 0,
        }
    }

    /// Records one completed iteration against the deadline that woke it.
    fn observe(&mut self, deadline: tokio::time::Instant, period: Duration) {
        let period_nanos = period.as_nanos();
        if period_nanos == 0 {
            // A zero period would divide by zero. The tick budget is a constant
            // of the crate, but this is the answer to "how many did we skip",
            // and a panic here would be reported as a tick-rate bug.
            self.ran += 1;
            return;
        }
        match self.origin {
            // The first deadline is the origin, not a measurement.
            None => self.origin = Some(deadline),
            Some(origin) => {
                self.ran += 1;
                let expected =
                    deadline.saturating_duration_since(origin).as_nanos() / period_nanos;
                // Assigned, not accumulated. `expected` and `ran` are both
                // absolute from the first deadline, so their difference is the
                // total skipped so far; adding the difference on every tick
                // would count the same shortfall once per tick it persists, and
                // the number would climb for as long as the loop stayed behind.
                if expected > self.ran as u128 {
                    self.skipped = (expected - self.ran as u128) as u64;
                }
            }
        }
    }

    fn ran(&self) -> u64 {
        self.ran
    }

    fn skipped(&self) -> u64 {
        self.skipped
    }
}

/// Resolves the ship count from the command line, or the default.
///
/// Bug №609: `--ships` with no value, or with junk, silently fell back to 1 —
/// the same number as "flag absent", so a typo looked exactly like the
/// default. Both are reported instead, because a match that runs one ship when
/// the operator asked for forty looks like the engine ignoring them.
fn resolve_ship_count(args: &[String]) -> Result<usize> {
    // The flag being present with nothing after it is a typo, not an absent
    // flag: `arg_value` cannot tell the two apart on its own, and treating the
    // typo as "use the default" is how a mistyped flag silently produces the
    // default match.
    if !args.iter().any(|a| a == "--ships") {
        return Ok(DEFAULT_SHIP_COUNT);
    }
    let raw = arg_value(args, "--ships")
        .with_context(|| "`--ships` was given with no value")?;
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
fn world_radius_for(ship_count: usize) -> f32 {
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
fn load_world(args: &[String], ship_count: usize) -> Result<WorldMap> {
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
    let world = WorldMap::procedural(PROCEDURAL_MAP_NAME, seed, radius, PROCEDURAL_MAP_OBSTACLES)
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

/// Whether a spawn point is inside a solid volume.
///
/// Reported, not repaired. Ships do not collide with map obstacles yet, so
/// moving the spawn would hide the fact rather than fix anything; the useful
/// output is the line saying the starting position is already inside a rock.
fn spawn_is_blocked(world: &WorldMap, point: Vec3f) -> Option<u64> {
    world
        .obstacles
        .iter()
        .find(|obstacle| obstacle.bounds.contains(point))
        .map(|obstacle| obstacle.id)
}

static NEXT_PROJECTILE_ID: AtomicU64 = AtomicU64::new(PROJECTILE_ID_BASE);

fn new_projectile_id() -> EntityId {
    EntityId::new(NEXT_PROJECTILE_ID.fetch_add(1, Ordering::Relaxed))
}

struct CompartmentStatic {
    name: String,
    local_bounds: Bounds,
    max_water_level: f32,
    pump_capacity: f32,
    connected_compartments: Vec<EntityId>,
    bulkheads: Vec<BulkheadEntity>,
}

struct StationStatic {
    station_type: rfs_core::entity::StationType,
    local_transform: Transform,
    compartment_id: EntityId,
    max_ammo: u32,
    max_health: f32,
    /// Bug №85: the real reload-cycle time, not `max_ammo`.
    reload_time: f32,
}

struct TrackedShip {
    entity_id: EntityId,
    class_id: u32,
    max_health: f32,
    max_fuel: f32,
    max_speed: f32,
    compartments: HashMap<EntityId, CompartmentStatic>,
    stations: HashMap<EntityId, StationStatic>,
}

impl TrackedShip {
    fn capture(ship: &Ship) -> Self {
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
                    reload_time: station
                        .map(|s| s.config().reload_time)
                        .unwrap_or(0.0),
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

    fn ship_entity(&self, state: &ShipState) -> ShipEntity {
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
struct PlayerMeta {
    player_entity_id: EntityId,
    ship_id: EntityId,
    name: String,
    spawn_local: Vec3f,
}

fn spawn_player(net: &NetServer, ship: &Ship, connection_id: u32, name: &str) -> PlayerMeta {
    let player_entity_id = EntityId::new(PLAYER_ID_BASE + connection_id as u64);
    let spawn_local = Vec3f::new(0.0, 4.0, -8.0);
    let player = PlayerEntity {
        entity_id: player_entity_id,
        player_id: connection_id,
        name: name.to_string(),
        team: 0,
        transform: Transform::new(ship.local_to_world(spawn_local), Quatf::IDENTITY, Vec3f::ONE),
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

fn despawn_player(sim: &TickSystem, net: &NetServer, connection_id: u32, players: &mut HashMap<u32, PlayerMeta>) {
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

/// Bug №59: the current helmsman — the player occupying a Helm station.
fn current_helm_occupant(ship: &Ship) -> Option<EntityId> {
    ship.get_state()
        .station_states
        .values()
        .find(|s| s.station_type == rfs_core::entity::StationType::Helm && s.occupant.is_some())
        .and_then(|s| s.occupant)
}

/// Bug №150: NaN-safe clamp for axis inputs — f32::clamp passes NaN through,
/// which would poison physics. Non-finite inputs are treated as 0.
fn clamp_axis(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn apply_input(
    sim: &TickSystem,
    ship: &Ship,
    player_id: EntityId,
    input: &InputPacket,
) {
    // Bug №59: steering is arbitrated — whoever ended up at a Helm seat, and
    // only that player, may drive. With nobody at the helm, anyone may (the
    // ship is un-manned). This replaces "last packet of the tick wins".
    let helm = current_helm_occupant(ship);
    if helm.is_none() || helm == Some(player_id) {
        ship.set_throttle(clamp_axis(input.move_forward));
        let rudder = (clamp_axis(input.move_right) + clamp_axis(input.yaw) * 0.5).clamp(-1.0, 1.0);
        ship.set_rudder(rudder);
    }

    if let Some(interaction) = &input.station_interaction {
        // Bug №196: a refused shot returns a reason instead of vanishing. The
        // input path has no command ack to carry it, so it is logged here — the
        // point is that the drop is observable at all.
        if let Some(reason) = handle_station_interaction(sim, ship, player_id, interaction) {
            warn!("player {:?} station action refused: {reason}", player_id.0);
        }
    }
}

/// Applies a station action, returning a reason if the shot was refused.
fn handle_station_interaction(
    sim: &TickSystem,
    ship: &Ship,
    player_id: EntityId,
    interaction: &StationInteraction,
) -> Option<String> {
    // Bug №59: only the occupant may operate a station — the old code let
    // any crew member yaw/fire/reload any gun of their ship without ever
    // entering the station.
    let occupied_by = ship
        .get_state()
        .station_states
        .get(&interaction.station_id)
        .and_then(|s| s.occupant);
    if occupied_by != Some(player_id) {
        return None;
    }

    let station_compartment = || {
        ship.get_station(interaction.station_id)
            .map(|s| s.compartment_id())
            .unwrap_or(EntityId::nil())
    };

    match interaction.action {
        StationAction::SetYaw => {
            if let Some((_, pitch)) = ship.station_angles(interaction.station_id) {
                ship.set_station_target_angles(interaction.station_id, interaction.value, pitch);
            }
        }
        StationAction::SetPitch => {
            if let Some((yaw, _)) = ship.station_angles(interaction.station_id) {
                ship.set_station_target_angles(interaction.station_id, yaw, interaction.value);
            }
        }
        StationAction::SetThrottle => {
            ship.set_throttle(clamp_axis(interaction.value));
        }
        StationAction::SetRudder => {
            ship.set_rudder(clamp_axis(interaction.value));
        }
        StationAction::Fire => {
            if let Some(result) = ship.try_fire_station(interaction.station_id, None) {
                // Bug №196: a refused spawn used to be dropped silently, so the
                // shooter saw a normal frame and no shell.
                if let Err(rejected) = spawn_projectile(sim, ship, interaction.station_id, result) {
                    return Some(format!("shot refused: {}", rejected.as_str()));
                }
            }
        }
        StationAction::Reload => {
            if let Some(station) = ship.get_station(interaction.station_id) {
                ship.reload_station(
                    interaction.station_id,
                    station.config().default_ammo_type,
                );
            }
        }
            StationAction::SetAmmoType => {
                // Bug №192: the value arrived as an `f32` and went through a bare
                // `as u8`. That cast saturates rather than rejecting, so `-3.0`
                // and `NaN` both became ammo type 0 — a *valid* cannonball — and
                // an out-of-range byte became 255, which the old `_ => Cannonball`
                // catch-all silently fired anyway. So the range is checked on the
                // float, before the cast, and it has to be a whole number: `5.5`
                // is not a shell.
                if !interaction.value.is_finite() || interaction.value.fract() != 0.0 {
                    return Some(format!(
                        "ammo type {:?} is not a whole number",
                        interaction.value
                    ));
                }
                let requested = interaction.value as i64;
                if !(0..=255).contains(&requested) || !Station::is_valid_ammo_type(requested as u8)
                {
                    return Some(format!("unknown ammo type {requested}"));
                }
                ship.reload_station(interaction.station_id, requested as u8);
            }
        // Bug №157: engineering controls were a dead `_ => {}` — a pump
        // station could never pump, damage control never repaired nor
        // extinguished, bulkheads could not be opened or sealed.
        StationAction::PumpWater => {
            ship.toggle_station_pump(interaction.station_id);
        }
        StationAction::SealBulkhead => {
            let comp_id = station_compartment();
            if !comp_id.is_nil() {
                for b in ship
                    .get_state()
                    .compartment_states
                    .get(&comp_id)
                    .map(|c| c.bulkhead_states.clone())
                    .unwrap_or_default()
                {
                    ship.set_bulkhead_seal(b.entity_id, true);
                }
            }
        }
        StationAction::OpenBulkhead => {
            let comp_id = station_compartment();
            if !comp_id.is_nil() {
                for b in ship
                    .get_state()
                    .compartment_states
                    .get(&comp_id)
                    .map(|c| c.bulkhead_states.clone())
                    .unwrap_or_default()
                {
                    ship.set_bulkhead_seal(b.entity_id, false);
                }
            }
        }
        StationAction::Repair => {
            // Bug №604 (analysis 3.1.3): `interaction.value.max(0.0)` let
            // `f32::INFINITY` reach `.min(max_health)`, and one packet produced
            // a full repair. Non-finite is refused (and reported, №196 style);
            // finite values are clamped to the same per-action ceiling the
            // command path uses.
            if !interaction.value.is_finite() {
                return Some(format!("repair amount {:?} is not finite", interaction.value));
            }
            ship.repair_station(
                interaction.station_id,
                interaction.value.clamp(0.0, MAX_REPAIR_PER_ACTION),
            );
        }
        StationAction::ExtinguishFire => {
            // Bug №607: `extinguish_compartment` used to return `true` for any
            // existing compartment, so this path acked nothing. It now reports
            // the cold case, and this names it instead of swallowing it.
            if !ship.extinguish_compartment(station_compartment()) {
                return Some("nothing is burning there".into());
            }
        }
    }
    None
}

fn spawn_projectile(
    sim: &TickSystem,
    ship: &Ship,
    weapon: EntityId,
    result: FireResult,
) -> Result<(), rfs_sim::tick::ProjectileRejected> {
    let world_pos = ship.local_to_world(result.position);
    let world_vel = ship.transform().rotation.mul_vec3(result.velocity);
    // Bug №193: damage, penetration and blast radius now come from the
    // projectile type's own config instead of a hardcoded 100/50 in the station
    // and a hardcoded 5.0 here. Those two answers disagreed with the config,
    // and with each other, and the config was the one nobody read.
    let config = ProjectileConfig::for_type(result.projectile_type);
    sim.fire_projectile(Projectile {
        entity_id: new_projectile_id(),
        projectile_type: result.projectile_type,
        position: world_pos,
        prev_position: world_pos,
        velocity: world_vel,
        spawn_tick: sim.current_tick(),
        lifetime: 0.0,
        max_lifetime: 12.0,
        distance_traveled: 0.0,
        owner: ship.entity_id(),
        weapon,
        damage: config.damage,
        penetration: config.penetration,
        explosion_radius: config.explosion_radius,
    })
}

fn handle_command(
    net: &NetServer,
    sim: &TickSystem,
    ship: &Ship,
    connection_id: u32,
    player_id: EntityId,
    command: &CommandPacket,
) {
    let (success, error) = translate_command(sim, ship, player_id, &command.command);

    // Bug №153: StationOccupied/StationVacated were never pushed — broadcast
    // the real, command-driven transition on success.
    if success {
        match &command.command {
            ServerCommand::EnterStation { station, .. } => {
                net.broadcast_event(CoreGameEvent::StationOccupied {
                    station: *station,
                    player: player_id,
                });
            }
            ServerCommand::ExitStation { .. } => {
                let occupied = ship
                    .get_state()
                    .station_states
                    .iter()
                    .find(|(_, s)| s.occupant == Some(player_id))
                    .map(|(id, _)| *id);
                if let Some(sid) = occupied {
                    net.broadcast_event(CoreGameEvent::StationVacated {
                        station: sid,
                        player: player_id,
                    });
                }
            }
            // Bug №607: broadcast the extinguish moment, matching how the
            // occupancy commands above announce theirs. `FireStarted` is pushed
            // from the tick (it is a collision outcome); the extinguish is a
            // command, so it is announced here where the command succeeded.
            ServerCommand::ExtinguishFire { compartment } => {
                net.broadcast_event(CoreGameEvent::FireExtinguished {
                    compartment: *compartment,
                });
            }
            _ => {}
        }
    }

    net.send_command_ack(connection_id, command.command_id, success, error);
}

fn translate_command(sim: &TickSystem, ship: &Ship, player_id: EntityId, command: &ServerCommand) -> (bool, Option<String>) {
    // Bug №59: a command on a station is only valid for its occupant.
    let commands_station = |station_id: &EntityId| {
        ship.get_state()
            .station_states
            .get(station_id)
            .and_then(|s| s.occupant)
            == Some(player_id)
    };
    // Bug №59: steering follows the same helm arbitration as inputs.
    let stance = || {
        let helm = current_helm_occupant(ship);
        helm.is_none() || helm == Some(player_id)
    };
    // Bug №604 (analysis 3.1.2 + 3.1.4): the six engineering commands checked
    // nothing — not occupancy, not the type of the station the player sat at,
    // so the helmsman could seal bulkheads, run pumps and repair the ship. Each
    // action now demands the authenticated player be the occupant of a station
    // whose type can actually perform it.
    let commands_engineering = |allowed: &[StationType]| {
        ship.get_state()
            .station_states
            .values()
            .any(|s| s.occupant == Some(player_id) && allowed.contains(&s.station_type))
    };

    match command {
        ServerCommand::SetShipThrottle { ship: target, throttle } => {
            if *target == ship.entity_id() && stance() {
                ship.set_throttle(*throttle);
                (true, None)
            } else {
                (false, Some(if *target == ship.entity_id() { "not at the helm".into() } else { "unknown ship".into() }))
            }
        }
        ServerCommand::SetShipRudder { ship: target, rudder } => {
            if *target == ship.entity_id() && stance() {
                ship.set_rudder(*rudder);
                (true, None)
            } else {
                (false, Some(if *target == ship.entity_id() { "not at the helm".into() } else { "unknown ship".into() }))
            }
        }
        ServerCommand::FireWeapon { station, target_pos } => {
            if !commands_station(station) {
                (false, Some("station not occupied by you".into()))
            } else {
                // Bug №154: target_pos used to be ignored entirely — the
                // client could aim anywhere and the gun fired wherever it
                // happened to point. Honor it by slewing the turret to the
                // spot (local frame, station yaw convention: x=sin, z=cos)
                // before the shot.
                if let (Some(target), Some(st_meta)) = (target_pos, ship.get_station(*station)) {
                    if target.is_finite() {
                        let dir_local = ship.world_to_local(*target)
                            - st_meta.local_transform().position;
                        let dir = dir_local.normalize();
                        if dir.length() > 0.01 {
                            let yaw = dir.x.atan2(dir.z);
                            let pitch = dir.y.clamp(-1.0, 1.0).asin();
                            ship.set_station_target_angles(*station, yaw, pitch);
                        }
                    }
                }
                if let Some(result) = ship.try_fire_station(*station, None) {
                    // Bug №196: this is the command path, so the ack can carry
                    // the reason. The spawn used to be dropped silently, so a
                    // shot refused by the projectile cap was acked as success
                    // and the player saw a normal frame with no shell.
                    match spawn_projectile(sim, ship, *station, result) {
                        Ok(()) => (true, None),
                        Err(rejected) => {
                            (false, Some(format!("shot refused: {}", rejected.as_str())))
                        }
                    }
                } else {
                    (false, Some("weapon not ready".into()))
                }
            }
        }
        ServerCommand::ReloadWeapon { station, ammo_type } => {
            if !commands_station(station) {
                (false, Some("station not occupied by you".into()))
            } else if !Station::is_valid_ammo_type(*ammo_type) {
                // Bug №192: an unknown ammo type used to be accepted and then
                // fired as a cannonball by the mapping's catch-all. Reporting it
                // here is the point: an ack that says "ok" for a request that
                // loaded nothing is how this stayed invisible.
                (
                    false,
                    Some(format!("unknown ammo type {ammo_type}")),
                )
            } else {
                ship.reload_station(*station, *ammo_type);
                (true, None)
            }
        }
        // Bug №157: the Ship engineering commands were a dead
        // `_ => "command not implemented"` — flooding could not be repaired,
        // fires not extinguished, bulkheads not sealed/open, pumps not run.
        ServerCommand::SealBulkhead { bulkhead } => {
            if !commands_engineering(&[StationType::DamageControl]) {
                (false, Some("not at a damage-control station".into()))
            } else if ship.set_bulkhead_seal(*bulkhead, true) {
                (true, None)
            } else {
                (false, Some("bulkhead not found".into()))
            }
        }
        ServerCommand::OpenBulkhead { bulkhead } => {
            if !commands_engineering(&[StationType::DamageControl]) {
                (false, Some("not at a damage-control station".into()))
            } else if ship.set_bulkhead_seal(*bulkhead, false) {
                (true, None)
            } else {
                (false, Some("bulkhead not found".into()))
            }
        }
        ServerCommand::ActivatePump { pump } => {
            if !commands_engineering(&[StationType::Pump]) {
                (false, Some("not at a pump station".into()))
            } else if ship.set_station_pump(*pump, true) {
                (true, None)
            } else {
                (false, Some("pump not found".into()))
            }
        }
        ServerCommand::DeactivatePump { pump } => {
            if !commands_engineering(&[StationType::Pump]) {
                (false, Some("not at a pump station".into()))
            } else if ship.set_station_pump(*pump, false) {
                (true, None)
            } else {
                (false, Some("pump not found".into()))
            }
        }
        ServerCommand::RepairStation { station } => {
            if !commands_engineering(&[StationType::DamageControl]) {
                (false, Some("not at a damage-control station".into()))
            } else if ship.repair_station(*station, MAX_REPAIR_PER_ACTION) {
                (true, None)
            } else {
                (false, Some("station not found".into()))
            }
        }
        ServerCommand::ExtinguishFire { compartment } => {
            if !commands_engineering(&[StationType::DamageControl]) {
                (false, Some("not at a damage-control station".into()))
            } else if ship.extinguish_compartment(*compartment) {
                (true, None)
            } else if ship.get_state().compartment_states.contains_key(compartment) {
                // Bug №607: the compartment exists but was not on fire, so the
                // old "compartment not found" reason was both wrong and hid the
                // fact that nothing happened. `extinguish_compartment` now
                // reports the cold case as a failure; name it.
                (false, Some("nothing is burning there".into()))
            } else {
                (false, Some("compartment not found".into()))
            }
        }
        ServerCommand::EnterStation { station, .. } => {
            // Auth: station is occupied by the authenticated connection player,
            // never by the client-supplied `player` field (spoofable).
            if ship.occupy_station(*station, player_id) {
                (true, None)
            } else {
                (false, Some("cannot occupy station".into()))
            }
        }
        ServerCommand::ExitStation { .. } => {
            // Auth: only the authenticated player can vacate their own station.
            let occupied = ship
                .get_state()
                .station_states
                .iter()
                .find(|(_, s)| s.occupant == Some(player_id))
                .map(|(id, _)| *id);
            if let Some(sid) = occupied {
                ship.vacate_station(sid);
                (true, None)
            } else {
                (false, Some("player is not in a station".into()))
            }
        }
        _ => (false, Some("command not implemented".into())),
    }
}

fn core_game_event(event: &SimGameEvent) -> CoreGameEvent {
    match event {
        SimGameEvent::ShipHit {
            target,
            projectile,
            position,
            normal,
            damage,
            penetration,
            hit_compartment,
        } => CoreGameEvent::ShipHit {
            target: *target,
            projectile: *projectile,
            position: *position,
            normal: *normal,
            damage: *damage,
            penetration: *penetration,
            hit_compartment: *hit_compartment,
        },
        SimGameEvent::ShipSunk { ship, position } => CoreGameEvent::ShipSunk {
            ship: *ship,
            position: *position,
        },
        SimGameEvent::CompartmentFlooded {
            compartment,
            water_level,
        } => CoreGameEvent::CompartmentFlooded {
            compartment: *compartment,
            water_level: *water_level,
        },
        SimGameEvent::StationOccupied { station, player } => CoreGameEvent::StationOccupied {
            station: *station,
            player: *player,
        },
        SimGameEvent::StationVacated { station, player } => CoreGameEvent::StationVacated {
            station: *station,
            player: *player,
        },
        SimGameEvent::FireStarted {
            compartment,
            intensity,
        } => CoreGameEvent::FireStarted {
            compartment: *compartment,
            intensity: *intensity,
        },
        SimGameEvent::FireExtinguished { compartment } => CoreGameEvent::FireExtinguished {
            compartment: *compartment,
        },
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse()?),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    // Bug №609: the default was 1, so the declared 15-40 ship scenario was
    // unreachable without knowing the flag, and a typo in the flag silently
    // produced the same one-ship match as no flag at all. Both are reported now.
    let ship_count = resolve_ship_count(&args)?;

    let config = ServerConfig::default();
    let net = Arc::new(NetServer::new(config).await?);
    let mut events = net.get_event_receiver().expect("server event receiver");
    let interest = net.interest_manager();

    // The world is loaded before anything is placed in it, so every ship spawn
    // can be checked against it and nothing runs in an empty universe by
    // accident.
    let world = load_world(&args, ship_count)?;

    struct ShipEntry {
        id: EntityId,
        tracked: TrackedShip,
    }

    let mut sim = TickSystem::new();
    sim.set_world(world.clone()).context("map validation failed")?;
    let mut ships: Vec<ShipEntry> = Vec::with_capacity(ship_count);
    // Plan §19.1: several ships so bots spread across crews instead of one point.
    for i in 0..ship_count {
        let ship_id = EntityId::new(SHIP_ID + i as u64 * SHIP_ID_STRIDE);
        let class = create_default_frigate();
        let ship_config: ShipConfig = class.into();
        let ship = Ship::new(ship_config, ship_id, Arc::new(DamageSystem::new()));
        let mut initial_state = ship.get_state();
        let wanted = Vec3f::new(i as f32 * SHIP_SPACING, 0.0, 0.0);
        // Clamped, not trusted: the spawn line is generated, and a map smaller
        // than the line would leave the tail ships outside the world. `moved`
        // says whether that happened instead of letting the tick's boundary
        // clamp hide it a few frames later.
        let (spawn, moved) = world.clamp_point(wanted);
        if moved {
            warn!(
                "ship {:?} wanted to spawn at {:?}, outside the world; clamped to {:?}",
                ship_id, wanted, spawn
            );
        }
        if let Some(blocking) = spawn_is_blocked(&world, spawn) {
            warn!(
                "ship {:?} spawns inside obstacle {} at {:?}",
                ship_id, blocking, spawn
            );
        }
        initial_state.transform = Transform::new(spawn, Quatf::IDENTITY, Vec3f::ONE);
        ship.apply_state(initial_state);

        let tracked = TrackedShip::capture(&ship);
        sim.add_ship(ship);

        let ship_handle = sim.get_ship(ship_id).expect("ship registered");
        let ship_entity = tracked.ship_entity(&ship_handle.get_state());
        for compartment in &ship_entity.compartments {
            interest.add_compartment(compartment.clone());
        }
        for station in &ship_entity.stations {
            interest.add_station(station.clone());
        }
        net.update_entity_interest(ship_entity);
        ships.push(ShipEntry { id: ship_id, tracked });
    }

    net.start();
    info!(
        "server listening on {} (match {}), ships={}",
        net.local_addr()?,
        net.match_id(),
        ships.len(),
    );

    let mut tick_interval = tokio::time::interval(Duration::from_millis(TICK_DURATION_MS));
    tick_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Bug №610 (analysis 3.6.4, plan step 6): the loop below had no exit at
    // all, so Ctrl-C killed the process outright — `NetServer::stop()` existed,
    // disconnected every client with `ServerShutdown`, and was called from
    // tests only. In production that path was dead: a stopping server dropped
    // its UDP socket without a word, so every client saw a transport error
    // rather than a shutdown, and nothing logged that the match ended. The
    // signal is awaited once and reused, not re-registered every tick.
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    // Bug №614: `MissedTickBehavior::Skip` drops a late deadline silently, so
    // the loop now asks `TickCadence` how many it ran against how many were due
    // and reports the difference instead of leaving it to be inferred from a
    // `loop avg` that cannot tell "slow" from "never happened".
    let mut cadence = TickCadence::new();
    let tick_period = Duration::from_millis(TICK_DURATION_MS);

    let mut players: HashMap<u32, PlayerMeta> = HashMap::new();
    let mut projectile_ids: HashSet<EntityId> = HashSet::new();
    // Plan §19.1: measure tick cost as bots scale up.
    let mut tick_time_sum = Duration::ZERO;
    let mut tick_time_max = Duration::ZERO;
    let mut tick_time_count = 0u64;
    // Bug №265: the whole iteration, not just the simulation step.
    let mut loop_time_sum = Duration::ZERO;
    let mut loop_time_max = Duration::ZERO;
    // Per-phase breakdown. "sim+sync" alone is a sum, and a sum cannot be
    // acted on: the three phases have completely different fixes, so guessing
    // which one dominates is how effort gets aimed at the wrong subsystem.
    let mut sim_time_sum = Duration::ZERO;
    let mut entity_sync_time_sum = Duration::ZERO;
    let mut interest_time_sum = Duration::ZERO;
    let mut broadcast_time_sum = Duration::ZERO;
    // Bug №273: the event drain sits between `loop_start` and `tick_start`, so
    // it was inside `loop avg` but outside all four measured phases. At 200 bots
    // that gap was 66 ms against a 33 ms budget while every measured phase read
    // under 2 ms — the loop was twice over budget and the report could not say
    // where. Guessing is how effort gets aimed at the wrong subsystem, so it is
    // measured like the rest.
    let mut event_drain_time_sum = Duration::ZERO;
    // Bug №266: publication rate, counted separately from the tick rate so the
    // two can be compared instead of assumed equal.
    let mut last_rounds = 0u64;
    let mut last_packets = 0u64;
    // Bug №614: the skipped total is monotonic, so the per-report figure is the
    // difference against the last one. Same shape as the round/packet counters
    // above, which is why it reads the same way in the log.
    let mut last_skipped = 0u64;

    loop {
        // Bug №610: the shutdown signal is raced against the tick instead of
        // being awaited after it. Awaiting the tick first would mean a Ctrl-C
        // still waits out a whole 33 ms iteration before anything happens, and
        // on a busy tick — the case where stopping matters most — that is the
        // iteration that is already over budget.
        tokio::select! {
            _ = &mut shutdown => {
                info!("shutdown signal received; stopping the match");
                break;
            }
            deadline = tick_interval.tick() => {
                // Bug №614: the deadline is the interval's own, not `now()` — it
                // is the value that reveals the skips.
                cadence.observe(deadline, tick_period);
            }
        }
        // Bug №265: the `perf:` line below starts its timer at `sim.tick()`,
        // which is AFTER the event drain and input handling above. So it timed
        // "sim+sync" at 0.7 ms while the loop only reached 23 Hz against a
        // 33.3 ms budget — the unmeasured half of the iteration was where the
        // time actually went, and the report made the server look four times
        // faster than it was. `loop_start` covers the whole iteration, and the
        // two numbers are now reported together so the gap is visible instead
        // of being invisible by construction.
        let loop_start = std::time::Instant::now();
        net.check_timeouts();

        while let Ok(event) = events.try_recv() {
            match event {
                ServerEvent::ClientConnected { connection_id, addr } => {
                    info!("client {connection_id} connected from {addr}");
                    let ship_idx = connection_id as usize % ships.len();
                    if let Some(ship) = sim.get_ship(ships[ship_idx].id) {
                        players.insert(connection_id, spawn_player(&net, &ship, connection_id, "player"));
                    }
                }
                ServerEvent::ClientDisconnected { connection_id, .. } => {
                    info!("client {connection_id} disconnected");
                    despawn_player(&sim, &net, connection_id, &mut players);
                }
                ServerEvent::ClientTimeout { connection_id } => {
                    info!("client {connection_id} timed out");
                    despawn_player(&sim, &net, connection_id, &mut players);
                }
                ServerEvent::ClientInput { connection_id, input } => {
                    // Bug №82/#71: echo back the client's own input.tick so its
                    // pending map can be keyed by the same value, and always
                    // answer — reject with a negative ack when the client is
                    // unknown, so its pending inputs cannot grow forever.
                    let accepted = players
                        .get(&connection_id)
                        .and_then(|meta| sim.get_ship(meta.ship_id).map(|ship| (meta.player_entity_id, ship)))
                        .map(|(player_id, ship)| {
                            apply_input(&sim, &ship, player_id, &input);
                            true
                        })
                        .unwrap_or(false);
                    net.send_input_ack(connection_id, InputAckPacket {
                        tick: input.tick,
                        accepted,
                    });
                }
                ServerEvent::ClientCommand { connection_id, command } => {
                    let actor = players.get(&connection_id).map(|m| (m.ship_id, m.player_entity_id));
                    match actor.and_then(|(ship_id, _)| sim.get_ship(ship_id)) {
                        Some(ship) => {
                            let player_id = players.get(&connection_id).map(|m| m.player_entity_id).unwrap_or(EntityId::nil());
                            handle_command(&net, &sim, &ship, connection_id, player_id, &command);
                        }
                        None => {
                            net.send_command_ack(
                                connection_id,
                                command.command_id,
                                false,
                                Some("ship not ready".into()),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        let event_drain_done = std::time::Instant::now();

        let tick_start = std::time::Instant::now();
        let result = sim.tick();
        let sim_done = std::time::Instant::now();
        let entity_sync_start = sim_done;

        // Bug №160: a ship that sank was removed from the sim, but its crew
        // stayed orphaned in the players map — their meta pointed at a ship
        // that no longer exists, so inputs were rejected and the ghost player
        // entity lingered in the interest grid forever. Respawn them on a
        // live ship (or drop them when none is left).
        if !players.is_empty() {
            let orphaned: Vec<(u32, PlayerMeta)> = players
                .iter()
                .filter(|(_, m)| sim.get_ship(m.ship_id).is_none())
                .map(|(cid, m)| (*cid, m.clone()))
                .collect();
            if !orphaned.is_empty() {
                let live_ships: Vec<EntityId> = ships
                    .iter()
                    .map(|e| e.id)
                    .filter(|id| sim.get_ship(*id).is_some())
                    .collect();
                for (cid, meta) in orphaned {
                    net.remove_player(cid, meta.player_entity_id);
                    if live_ships.is_empty() {
                        players.remove(&cid);
                    } else {
                        let idx = cid as usize % live_ships.len();
                        if let Some(ship) = sim.get_ship(live_ships[idx]) {
                            players.insert(cid, spawn_player(&net, &ship, cid, &meta.name));
                        }
                    }
                }
            }
        }

        for entry in &ships {
            let Some(ship) = sim.get_ship(entry.id) else {
                continue;
            };
            let state = ship.get_state();
            let ship_entity = entry.tracked.ship_entity(&state);
            for compartment in ship_entity.compartments.iter() {
                interest.update_compartment(compartment.clone());
            }
            for station in ship_entity.stations.iter() {
                interest.update_station(station.clone());
            }
            net.add_entity_to_interest(ship_entity);

            for (connection_id, meta) in players.iter().filter(|(_, m)| m.ship_id == entry.id) {
                let occupied_station = state
                    .station_states
                    .iter()
                    .find(|(_, s)| s.occupant == Some(meta.player_entity_id))
                    .map(|(id, _)| *id);
                let transform = Transform::new(
                    ship.local_to_world(meta.spawn_local),
                    Quatf::IDENTITY,
                    Vec3f::ONE,
                );
                let updated = PlayerEntity {
                    entity_id: meta.player_entity_id,
                    player_id: *connection_id,
                    name: meta.name.clone(),
                    team: 0,
                    transform,
                    velocity: state.velocity,
                    posture: PlayerPosture::Standing,
                    health: 100.0,
                    max_health: 100.0,
                    stamina: 100.0,
                    max_stamina: 100.0,
                    current_station: occupied_station,
                    current_ship: Some(entry.id),
                    input_sequence: 0,
                    last_acknowledged_tick: result.tick.0 as u32,
                };
                interest.update_player(updated);
            }
        }

        // Bug №605: `remove_entity_from_interest` existed but had zero callers,
        // so a ship the sim had already dropped (sunk, removed) stayed in the
        // interest grids — and therefore in every player's visible set —
        // forever and was replicated every tick as a ghost. Take the hull and
        // its compartments and stations out as soon as the simulation no
        // longer knows the ship. Players on it were already respawned/dropped
        // just above, so this pass only clears the wreckage.
        for entry in &ships {
            if sim.get_ship(entry.id).is_some() {
                continue;
            }
            let compartments: Vec<EntityId> =
                entry.tracked.compartments.keys().copied().collect();
            let stations: Vec<EntityId> = entry.tracked.stations.keys().copied().collect();
            net.remove_entity_from_interest(entry.id, &compartments, &stations);
        }

        let mut next_ids: HashSet<EntityId> = HashSet::new();
        for projectile in sim.get_active_projectiles() {
            let entity = ProjectileEntity {
                entity_id: projectile.entity_id,
                projectile_type: projectile.projectile_type,
                position: projectile.position,
                velocity: projectile.velocity,
                spawn_tick: projectile.spawn_tick.0 as u32,
                lifetime: projectile.lifetime,
                max_lifetime: projectile.max_lifetime,
                owner: projectile.owner,
                weapon: projectile.weapon,
                damage: projectile.damage,
                penetration: projectile.penetration,
                explosion_radius: projectile.explosion_radius,
                has_exploded: false,
            };
            if projectile_ids.contains(&projectile.entity_id) {
                interest.update_projectile(entity);
            } else {
                interest.add_projectile(entity);
            }
            next_ids.insert(projectile.entity_id);
        }
        for removed in projectile_ids.difference(&next_ids) {
            interest.remove_projectile(*removed);
        }
        projectile_ids = next_ids;

        let entity_sync_done = std::time::Instant::now();

        interest.update_all_interests();
        let interest_done = std::time::Instant::now();

        for event in &result.events {
            net.broadcast_event(core_game_event(event));
        }
        let broadcast_done = std::time::Instant::now();

        // Bug №268: publish the simulation tick LAST, once every piece of this
        // tick's state is in the interest manager (entities, projectiles,
        // recomputed interest sets, events). The send loop reads this value and
        // stamps every packet with it, so a snapshot claiming to be tick N is
        // always built from state that is at least tick N.
        //
        // It used to keep its own counter and advance it at a steady 30 Hz
        // regardless of the simulation. Under load the main loop overran the
        // 33 ms budget, `MissedTickBehavior::Skip` dropped simulation ticks
        // silently, and clients were told the world had advanced ~5x further
        // than it had. Publishing here makes the two impossible to disagree.
        net.publish_tick(result.tick);

        // Per-phase accounting: the single "sim+sync" number hid whether the
        // cost was the simulation, the interest rebuild or the event
        // broadcast, and a wrong guess here sends the next fix at the wrong
        // subsystem.
        sim_time_sum += sim_done.duration_since(tick_start);
        entity_sync_time_sum += entity_sync_done.duration_since(entity_sync_start);
        interest_time_sum += interest_done.duration_since(entity_sync_done);
        // Bug №273: the drain belongs with the other phases — see the note on
        // `event_drain_time_sum`.
        event_drain_time_sum += event_drain_done.duration_since(loop_start);
        broadcast_time_sum += broadcast_done.duration_since(interest_done);

        let elapsed = tick_start.elapsed();
        let loop_elapsed = loop_start.elapsed();
        tick_time_sum += elapsed;
        tick_time_max = tick_time_max.max(elapsed);
        loop_time_sum += loop_elapsed;
        loop_time_max = loop_time_max.max(loop_elapsed);
        tick_time_count += 1;
        if result.tick.0.is_multiple_of(150) {
            let avg_ms = tick_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            let loop_avg_ms = loop_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            let sim_ms = sim_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            let entity_sync_ms = entity_sync_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            let interest_ms = interest_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            let broadcast_ms = broadcast_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            // Bug №273: the previously unmeasured half of the iteration.
            let event_drain_ms = event_drain_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            let stats = interest.stats();
            // Bug №266: publication rate, counted apart from the tick rate so
            // the two can be compared instead of assumed equal. A round that
            // emits nothing is counted too — from the client it is
            // indistinguishable from a round that never happened.
            let (rounds, packets) = net.snapshot_totals();
            // Bug №274: sent deltas per layer. A low CLIENT-side per-layer count
            // has two possible causes, and only this distinguishes them: the
            // server diffed an unchanged layer to nothing, or the client dropped
            // something that was sent.
            let sent_by_layer = net.sent_deltas_by_layer();
            let rounds_delta = rounds.saturating_sub(std::mem::replace(&mut last_rounds, rounds));
            let packets_delta = packets.saturating_sub(std::mem::replace(&mut last_packets, packets));
            // Bug №614: the skipped count is reported next to the average, not
            // instead of it. `loop avg` over budget says the iteration was slow;
            // it cannot say whether the missing ticks were slow or skipped, and
            // those two have different fixes.
            let ran_this_report = cadence.ran();
            let skipped_this_report = cadence.skipped().saturating_sub(last_skipped);
            last_skipped = cadence.skipped();
            info!(
                "perf: tick={} sim+sync avg={:.2}ms max={:.2}ms | phases events={event_drain_ms:.2}ms sim={sim_ms:.2}ms entity_sync={entity_sync_ms:.2}ms interest={interest_ms:.2}ms broadcast={broadcast_ms:.2}ms | loop avg={loop_avg_ms:.2}ms max={:.2}ms budget={:.2}ms | ticks ran={ran_this_report} skipped={skipped_this_report} | publish rounds={rounds_delta} packets={packets_delta} rounds_per_tick={:.2} packets_per_round={:.2} sent_deltas_by_layer={sent_by_layer:?} ships={} players={} entities={{ships:{},players:{},stations:{},compartments:{},projectiles:{}}}",
                result.tick.0,
                avg_ms,
                tick_time_max.as_secs_f64() * 1000.0,
                loop_time_max.as_secs_f64() * 1000.0,
                TICK_DURATION_MS as f64,
                rounds_delta as f64 / tick_time_count.max(1) as f64,
                packets_delta as f64 / rounds_delta.max(1) as f64,
                ships.len(),
                players.len(),
                stats.ships,
                stats.players,
                stats.stations,
                stats.compartments,
                stats.projectiles,
            );
            if skipped_this_report > 0 {
                // Bug №614: `MissedTickBehavior::Skip` drops these by
                // definition. Until now the only trace was an average over
                // budget, which reads the same whether the loop was slow or the
                // ticks were never run — and a server quietly at 23 Hz against a
                // 30 Hz contract looks like a network fault from every client.
                warn!(
                    "perf: {skipped_this_report} tick(s) skipped in {} ticks — the loop is not \
                     keeping up with its own budget, and clients cannot observe ticks that \
                     were never published",
                    ran_this_report,
                );
            }
            // A shell that hit a rock leaves no trace anywhere else: obstacle
            // health is not modelled, and the projectile is gone from the entity
            // list by the time this line runs. Without a running count, a map
            // full of asteroids swallows every shot aimed at one and the log looks
            // identical to a world where nothing was ever fired.
            if sim.obstacle_hits() > 0 {
                info!("map: {} projectile(s) spent on obstacles", sim.obstacle_hits());
            }
            if (rounds_delta as f64 / tick_time_count.max(1) as f64) < 0.95 {
                // A client cannot learn about a tick the server never published,
                // so this is the server-side half of an observed-tick shortfall
                // that would otherwise look like client or network loss.
                warn!(
                    "perf: only {rounds_delta} snapshot rounds for {} ticks; clients cannot \
                     observe ticks that were never published",
                    tick_time_count,
                );
            }
            if loop_avg_ms > TICK_DURATION_MS as f64 {
                // Stated plainly, because a `perf:` line that reports 0.7 ms
                // while the loop misses its budget is how this stayed hidden.
                // Name the dominant phase explicitly: the phases are measured
                // separately, so the message can point at one subsystem instead
                // of listing every candidate and leaving the reader to guess.
                // Bug №273: `events` participates in the comparison, because it was
                // the dominant cost while being excluded from it — every phase
                // it is compared against read under 2 ms while the loop was
                // twice over budget, and the warning named `sim`.
                let phases = [
                    ("events", event_drain_ms),
                    ("sim", sim_ms),
                    ("entity_sync", entity_sync_ms),
                    ("interest", interest_ms),
                    ("broadcast", broadcast_ms),
                ];
                let (dominant, dominant_ms) = *phases.iter().max_by(|a, b| {
                    a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                }).expect("phase list is never empty");
                warn!(
                    "perf: the whole tick iteration averages {loop_avg_ms:.2}ms against a \
                     {budget:.2}ms budget; dominant phase is {dominant} at {dominant_ms:.2}ms \
                     (events={event_drain_ms:.2}ms sim={sim_ms:.2}ms \
                     entity_sync={entity_sync_ms:.2}ms interest={interest_ms:.2}ms \
                     broadcast={broadcast_ms:.2}ms)",
                    budget = TICK_DURATION_MS as f64,
                );
            }
            tick_time_sum = Duration::ZERO;
            tick_time_max = Duration::ZERO;
            loop_time_sum = Duration::ZERO;
            loop_time_max = Duration::ZERO;
            // Bug №270: these four were missing from this reset, so the sums
            // kept accumulating across every report while `tick_time_count`
            // below restarted each time. The per-phase averages are computed as
            // sum / count, so from the second report on they read N times too
            // high and keep growing — the `dominant phase ... at {ms}ms`
            // warning named the right phase with a number that meant nothing,
            // two fields away from a correct `loop avg`. Reset them with the
            // others or the per-phase breakdown is worse than no breakdown.
            sim_time_sum = Duration::ZERO;
            entity_sync_time_sum = Duration::ZERO;
            interest_time_sum = Duration::ZERO;
            broadcast_time_sum = Duration::ZERO;
            // Bug №273: same reset requirement, same reason — added with the
            // measurement so the two cannot drift apart.
            event_drain_time_sum = Duration::ZERO;
            tick_time_count = 0;
        }
    }

    // Bug №610: `stop()` was only ever called from tests, so the one line that
    // disconnects clients with a `ServerShutdown` reason and closes the socket
    // cleanly never ran in a real server. Falling off the end of `main` without
    // it means every client sees a transport error instead of a shutdown, and
    // the match's end leaves no trace in the log. The graceful variant also
    // gives the outbox a window to actually deliver the farewells.
    net.stop_gracefully().await;
    info!("match stopped after {} ships", ships.len());
    Ok(())
}

#[cfg(test)]
mod engineering_command_tests {
    use super::*;

    fn ship() -> Ship {
        let ship_config: ShipConfig = create_default_frigate().into();
        Ship::new(ship_config, EntityId::new(SHIP_ID), Arc::new(DamageSystem::new()))
    }

    fn station_of(ship: &Ship, kind: StationType) -> EntityId {
        ship.get_state()
            .station_states
            .iter()
            .find(|(_, s)| s.station_type == kind)
            .map(|(id, _)| *id)
            .expect("the default frigate has one of each station type")
    }

    fn some_bulkhead(ship: &Ship) -> EntityId {
        ship.get_state()
            .compartment_states
            .values()
            .flat_map(|c| c.bulkhead_states.iter())
            .map(|b| b.entity_id)
            .next()
            .expect("the default frigate has bulkheads")
    }

    /// Sits the fixture player at a seat of `kind`, then runs the command
    /// exactly the way `handle_command` does. The player is vacated first
    /// because the ship keeps the one-player-one-station invariant.
    fn seated_command(
        ship: &Ship,
        kind: StationType,
        command: &ServerCommand,
    ) -> (bool, Option<String>) {
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        for (id, s) in ship.get_state().station_states {
            if s.occupant == Some(player) {
                ship.vacate_station(id);
            }
        }
        let station = station_of(ship, kind);
        assert!(ship.occupy_station(station, player), "fixture could not sit at {kind:?}");
        translate_command(&sim, ship, player, command)
    }

    /// Bug №604 (3.1.2): the six engineering commands checked nothing, so a
    /// player seated at no station at all could seal bulkheads.
    #[test]
    fn a_player_at_no_station_cannot_seal_a_bulkhead() {
        let ship = ship();
        let sim = TickSystem::new();
        let bulkhead = some_bulkhead(&ship);
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            EntityId::new(42),
            &ServerCommand::SealBulkhead { bulkhead },
        );
        assert!(!ok, "an unseated player sealed a bulkhead");
        assert_eq!(reason.as_deref(), Some("not at a damage-control station"));
    }

    /// Bug №604 (3.1.4): occupancy alone was never the point — the type of the
    /// seat has to matter, or the helmsman runs the whole damage-control
    /// routine.
    #[test]
    fn the_helmsman_cannot_seal_a_bulkhead() {
        let ship = ship();
        let bulkhead = some_bulkhead(&ship);
        let (ok, reason) =
            seated_command(&ship, StationType::Helm, &ServerCommand::SealBulkhead { bulkhead });
        assert!(!ok, "the helmsman sealed a bulkhead");
        assert_eq!(reason.as_deref(), Some("not at a damage-control station"));
    }

    /// The fix must not cost the feature: the seat built for the job still
    /// works on every one of its commands.
    #[test]
    fn damage_control_still_seals_repairs_and_extinguishes() {
        let ship = ship();
        let bulkhead = some_bulkhead(&ship);
        let gun = station_of(&ship, StationType::Gun);
        let compartment = *ship
            .get_state()
            .compartment_states
            .keys()
            .next()
            .expect("the default frigate has compartments");

        // Bug №607: extinguishing only succeeds if something was burning, so
        // the fixture lights the compartment first. This case used to pass on a
        // cold compartment — that was the defect, not the feature.
        assert!(
            ship.ignite_compartment(compartment, 0.5),
            "the fixture could not light the compartment"
        );

        let cases = [
            ("seal", ServerCommand::SealBulkhead { bulkhead }),
            ("open", ServerCommand::OpenBulkhead { bulkhead }),
            ("repair", ServerCommand::RepairStation { station: gun }),
            ("extinguish", ServerCommand::ExtinguishFire { compartment }),
        ];
        for (label, command) in cases {
            let (ok, reason) = seated_command(&ship, StationType::DamageControl, &command);
            assert!(ok, "damage control lost {label}: {reason:?}");
        }
    }

    /// A pump runs only from a pump seat, not from any other engineering seat.
    #[test]
    fn only_a_pump_station_runs_a_pump() {
        let ship = ship();
        let pump = station_of(&ship, StationType::Pump);

        let (ok, reason) =
            seated_command(&ship, StationType::Pump, &ServerCommand::ActivatePump { pump });
        assert!(ok, "the pump seat lost its own pump: {reason:?}");

        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::ActivatePump { pump },
        );
        assert!(!ok, "damage control ran a pump");
        assert_eq!(reason.as_deref(), Some("not at a pump station"));
    }

    /// A seat at 10 / max_health health, so any repair — honest or infinite —
    /// is visible as a number that moved.
    fn damaged_ship() -> (Ship, EntityId) {
        let ship = ship();
        let station = station_of(&ship, StationType::DamageControl);
        let mut state = ship.get_state();
        state.station_states.get_mut(&station).expect("seat exists").health = 10.0;
        ship.apply_state(state);
        (ship, station)
    }

    /// Bug №604 (3.1.3): `f32::INFINITY` arrived as `interaction.value`, sailed
    /// through `.max(0.0)`, and `.min(max_health)` handed out a full repair for
    /// one packet. The refusal shows up twice: as the returned reason, and as
    /// health that did not move.
    #[test]
    fn an_infinite_repair_amount_is_refused_and_fills_nothing() {
        let (ship, station) = damaged_ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        assert!(ship.occupy_station(station, player));

        let reason = handle_station_interaction(
            &sim,
            &ship,
            player,
            &StationInteraction {
                station_id: station,
                action: StationAction::Repair,
                value: f32::INFINITY,
            },
        );

        assert_eq!(
            reason.as_deref(),
            Some("repair amount inf is not finite"),
            "the infinite repair was not refused"
        );
        assert_eq!(
            ship.get_state().station_states[&station].health,
            10.0,
            "the infinite repair still moved health"
        );
    }

    /// A finite-but-absurd value is clamped to the one ceiling both paths use.
    #[test]
    fn a_huge_finite_repair_amount_is_clamped_to_the_ceiling() {
        let (ship, station) = damaged_ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        assert!(ship.occupy_station(station, player));

        let reason = handle_station_interaction(
            &sim,
            &ship,
            player,
            &StationInteraction {
                station_id: station,
                action: StationAction::Repair,
                value: 1.0e9,
            },
        );

        assert_eq!(reason, None, "a finite repair was refused");
        assert_eq!(
            ship.get_state().station_states[&station].health,
            10.0 + MAX_REPAIR_PER_ACTION,
            "the value was not clamped to MAX_REPAIR_PER_ACTION"
        );
    }

    fn some_compartment(ship: &Ship) -> EntityId {
        *ship
            .get_state()
            .compartment_states
            .keys()
            .next()
            .expect("the default frigate has compartments")
    }

    /// Bug №607: `extinguish_compartment` returned `true` for any existing
    /// compartment, so a damage-control seat acked a successful extinguishing
    /// on a compartment that was not on fire. The command path now names the
    /// cold case instead of claiming success.
    #[test]
    fn an_extinguish_on_a_cold_compartment_is_refused() {
        let ship = ship();
        let compartment = some_compartment(&ship);

        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::ExtinguishFire { compartment },
        );
        assert!(!ok, "a cold compartment was acked as extinguished");
        assert_eq!(reason.as_deref(), Some("nothing is burning there"));
    }

    /// The other half of the same test: a real fire must still go out. Without
    /// this, refusing everything would also be green.
    #[test]
    fn extinguishing_puts_out_a_burning_compartment() {
        let ship = ship();
        let compartment = some_compartment(&ship);
        assert!(
            ship.ignite_compartment(compartment, 0.5),
            "the fixture could not light the compartment"
        );

        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::ExtinguishFire { compartment },
        );
        assert!(ok, "damage control could not put out a fire: {reason:?}");
        assert_eq!(
            ship.get_state().compartment_states[&compartment].fire_intensity,
            0.0,
            "the fire was acked but not put out"
        );
    }

    /// The continuous input path carries the same truth, not the old silent
    /// success.
    #[test]
    fn the_input_path_also_refuses_a_cold_extinguish() {
        let ship = ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        let station = station_of(&ship, StationType::DamageControl);
        assert!(ship.occupy_station(station, player));

        let reason = handle_station_interaction(
            &sim,
            &ship,
            player,
            &StationInteraction {
                station_id: station,
                action: StationAction::ExtinguishFire,
                value: 0.0,
            },
        );

        assert_eq!(reason.as_deref(), Some("nothing is burning there"));
    }
}

/// Bug №614 (analysis 3.6.4): the skipped-tick arithmetic, separated from the
/// loop so it can be driven with deadlines no test can actually wait for.
#[cfg(test)]
mod tick_cadence_tests {
    use super::*;

    const PERIOD: Duration = Duration::from_millis(TICK_DURATION_MS);

    /// The headline case: a loop that keeps up skips nothing, and the counter
    /// saying otherwise would send someone hunting a bug that is not there.
    #[test]
    fn a_loop_that_keeps_up_skips_nothing() {
        let mut cadence = TickCadence::new();
        let start = tokio::time::Instant::now();
        for i in 0..30u32 {
            cadence.observe(start + PERIOD * i, PERIOD);
        }
        assert_eq!(cadence.ran(), 29, "the origin deadline is not a run");
        assert_eq!(
            cadence.skipped(),
            0,
            "a perfectly paced loop reported {} skipped ticks",
            cadence.skipped()
        );
    }

    /// The defect itself: one wake-up that arrives three periods late has
    /// swallowed two ticks, and the old code reported nothing at all.
    #[test]
    fn a_late_iteration_reports_the_ticks_it_swallowed() {
        let mut cadence = TickCadence::new();
        let start = tokio::time::Instant::now();
        cadence.observe(start, PERIOD);
        // One iteration, but the deadline is three periods on: two were skipped.
        cadence.observe(start + PERIOD * 3, PERIOD);
        assert_eq!(cadence.ran(), 1);
        assert_eq!(cadence.skipped(), 2, "the swallowed ticks were not counted");
    }

    /// Recovery must not manufacture skips or erase real ones: the counter is
    /// absolute against the first deadline, so returning to pace only adds
    /// future catches up, never rewrites history.
    #[test]
    fn recovering_does_not_rewrite_what_was_already_skipped() {
        let mut cadence = TickCadence::new();
        let start = tokio::time::Instant::now();
        cadence.observe(start, PERIOD);
        cadence.observe(start + PERIOD * 4, PERIOD);
        assert_eq!(cadence.skipped(), 3);
        // Back on pace for the next two deadlines.
        cadence.observe(start + PERIOD * 5, PERIOD);
        cadence.observe(start + PERIOD * 6, PERIOD);
        assert_eq!(
            cadence.skipped(),
            3,
            "the tally changed while the loop was back on schedule"
        );
        assert_eq!(cadence.ran(), 3);
    }

    /// A zero period must not divide by zero: this number exists to explain a
    /// tick-rate problem, and panicking inside it would be reported as one.
    #[test]
    fn a_zero_period_does_not_panic() {
        let mut cadence = TickCadence::new();
        let start = tokio::time::Instant::now();
        cadence.observe(start, Duration::ZERO);
        cadence.observe(start + Duration::from_millis(100), Duration::ZERO);
        assert_eq!(cadence.skipped(), 0);
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