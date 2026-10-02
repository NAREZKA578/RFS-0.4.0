use anyhow::Result;
use rfs_core::entity::{
    BulkheadEntity, CompartmentEntity, EntityFlags, EntityId, HitMark, PlayerEntity, ProjectileEntity,
    ShipEntity, StationEntity,
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
use rfs_ship::station::FireResult;
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
        handle_station_interaction(sim, ship, player_id, interaction);
    }
}

fn handle_station_interaction(sim: &TickSystem, ship: &Ship, player_id: EntityId, interaction: &StationInteraction) {
    // Bug №59: only the occupant may operate a station — the old code let
    // any crew member yaw/fire/reload any gun of their ship without ever
    // entering the station.
    let occupied_by = ship
        .get_state()
        .station_states
        .get(&interaction.station_id)
        .and_then(|s| s.occupant);
    if occupied_by != Some(player_id) {
        return;
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
                spawn_projectile(sim, ship, interaction.station_id, result);
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
            ship.reload_station(interaction.station_id, interaction.value as u8);
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
            ship.repair_station(interaction.station_id, interaction.value.max(0.0));
        }
        StationAction::ExtinguishFire => {
            ship.extinguish_compartment(station_compartment());
        }
    }
}

fn spawn_projectile(sim: &TickSystem, ship: &Ship, weapon: EntityId, result: FireResult) {
    let world_pos = ship.local_to_world(result.position);
    let world_vel = ship.transform().rotation.mul_vec3(result.velocity);
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
        damage: result.damage,
        penetration: result.penetration,
        explosion_radius: match result.projectile_type {
            rfs_core::entity::ProjectileType::ExplosiveShell => 5.0,
            _ => 0.0,
        },
    });
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
                    spawn_projectile(sim, ship, *station, result);
                    (true, None)
                } else {
                    (false, Some("weapon not ready".into()))
                }
            }
        }
        ServerCommand::ReloadWeapon { station, ammo_type } => {
            if !commands_station(station) {
                (false, Some("station not occupied by you".into()))
            } else {
                ship.reload_station(*station, *ammo_type);
                (true, None)
            }
        }
        // Bug №157: the Ship engineering commands were a dead
        // `_ => "command not implemented"` — flooding could not be repaired,
        // fires not extinguished, bulkheads not sealed/open, pumps not run.
        ServerCommand::SealBulkhead { bulkhead } => {
            if ship.set_bulkhead_seal(*bulkhead, true) {
                (true, None)
            } else {
                (false, Some("bulkhead not found".into()))
            }
        }
        ServerCommand::OpenBulkhead { bulkhead } => {
            if ship.set_bulkhead_seal(*bulkhead, false) {
                (true, None)
            } else {
                (false, Some("bulkhead not found".into()))
            }
        }
        ServerCommand::ActivatePump { pump } => {
            if ship.set_station_pump(*pump, true) {
                (true, None)
            } else {
                (false, Some("pump not found".into()))
            }
        }
        ServerCommand::DeactivatePump { pump } => {
            if ship.set_station_pump(*pump, false) {
                (true, None)
            } else {
                (false, Some("pump not found".into()))
            }
        }
        ServerCommand::RepairStation { station } => {
            if ship.repair_station(*station, 25.0) {
                (true, None)
            } else {
                (false, Some("station not found".into()))
            }
        }
        ServerCommand::ExtinguishFire { compartment } => {
            if ship.extinguish_compartment(*compartment) {
                (true, None)
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
    let ship_count = args
        .iter()
        .position(|a| a == "--ships")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 64);

    let config = ServerConfig::default();
    let net = Arc::new(NetServer::new(config).await?);
    let mut events = net.get_event_receiver().expect("server event receiver");
    let interest = net.interest_manager();

    struct ShipEntry {
        id: EntityId,
        tracked: TrackedShip,
    }

    let mut sim = TickSystem::new();
    let mut ships: Vec<ShipEntry> = Vec::with_capacity(ship_count);
    // Plan §19.1: several ships so bots spread across crews instead of one point.
    for i in 0..ship_count {
        let ship_id = EntityId::new(SHIP_ID + i as u64 * SHIP_ID_STRIDE);
        let class = create_default_frigate();
        let ship_config: ShipConfig = class.into();
        let ship = Ship::new(ship_config, ship_id, Arc::new(DamageSystem::new()));
        let mut initial_state = ship.get_state();
        initial_state.transform = Transform::new(
            Vec3f::new(i as f32 * SHIP_SPACING, 0.0, 0.0),
            Quatf::IDENTITY,
            Vec3f::ONE,
        );
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

    loop {
        tick_interval.tick().await;
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
            info!(
                "perf: tick={} sim+sync avg={:.2}ms max={:.2}ms | phases events={event_drain_ms:.2}ms sim={sim_ms:.2}ms entity_sync={entity_sync_ms:.2}ms interest={interest_ms:.2}ms broadcast={broadcast_ms:.2}ms | loop avg={loop_avg_ms:.2}ms max={:.2}ms budget={:.2}ms | publish rounds={rounds_delta} packets={packets_delta} rounds_per_tick={:.2} packets_per_round={:.2} sent_deltas_by_layer={sent_by_layer:?} ships={} players={} entities={{ships:{},players:{},stations:{},compartments:{},projectiles:{}}}",
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
}