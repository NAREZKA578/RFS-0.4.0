use anyhow::Result;
use rfs_core::entity::{
    BulkheadEntity, CompartmentEntity, EntityFlags, EntityId, PlayerEntity, ProjectileEntity,
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
use tracing::info;

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
                    is_sealed: live.map_or(false, |c| c.is_sealed),
                    is_breached: live.map_or(false, |c| c.is_breached),
                    fire_intensity: live.map_or(0.0, |c| c.fire_intensity),
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
                    is_operational: live.map_or(false, |s| s.is_operational),
                    health: live.map_or(0.0, |s| s.health),
                    max_health: ss.max_health,
                    cooldown: live.map_or(0.0, |s| s.cooldown),
                    max_cooldown: live.map_or(0.0, |s| s.max_ammo as f32),
                }
            })
            .collect()
    }
}

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

fn despawn_player(net: &NetServer, connection_id: u32, players: &mut HashMap<u32, PlayerMeta>) {
    if let Some(meta) = players.remove(&connection_id) {
        net.remove_player(connection_id, meta.player_entity_id);
    }
}

fn apply_input(
    net: &NetServer,
    sim: &TickSystem,
    ship: &Ship,
    connection_id: u32,
    input: &InputPacket,
) {
    ship.set_throttle(input.move_forward.clamp(-1.0, 1.0));
    let rudder = (input.move_right.clamp(-1.0, 1.0) + input.yaw.clamp(-1.0, 1.0) * 0.5)
        .clamp(-1.0, 1.0);
    ship.set_rudder(rudder);

    if let Some(interaction) = &input.station_interaction {
        handle_station_interaction(sim, ship, interaction);
    }

    net.send_input_ack(
        connection_id,
        InputAckPacket {
            tick: sim.current_tick().0 as u32,
            accepted: true,
        },
    );
}

fn handle_station_interaction(sim: &TickSystem, ship: &Ship, interaction: &StationInteraction) {
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
        _ => {}
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
    command: &CommandPacket,
) {
    let (success, error) = translate_command(sim, ship, &command.command);
    net.send_command_ack(connection_id, command.command_id, success, error);
}

fn translate_command(sim: &TickSystem, ship: &Ship, command: &ServerCommand) -> (bool, Option<String>) {
    match command {
        ServerCommand::SetShipThrottle { ship: target, throttle } => {
            if *target == ship.entity_id() {
                ship.set_throttle(*throttle);
                (true, None)
            } else {
                (false, Some("unknown ship".into()))
            }
        }
        ServerCommand::SetShipRudder { ship: target, rudder } => {
            if *target == ship.entity_id() {
                ship.set_rudder(*rudder);
                (true, None)
            } else {
                (false, Some("unknown ship".into()))
            }
        }
        ServerCommand::FireWeapon { station, .. } => {
            if let Some(result) = ship.try_fire_station(*station, None) {
                spawn_projectile(sim, ship, *station, result);
                (true, None)
            } else {
                (false, Some("weapon not ready".into()))
            }
        }
        ServerCommand::ReloadWeapon { station, ammo_type } => {
            ship.reload_station(*station, *ammo_type);
            (true, None)
        }
        ServerCommand::EnterStation { player, station } => {
            if ship.occupy_station(*station, *player) {
                (true, None)
            } else {
                (false, Some("cannot occupy station".into()))
            }
        }
        ServerCommand::ExitStation { player } => {
            let occupied = ship
                .get_state()
                .station_states
                .iter()
                .find(|(_, s)| s.occupant == Some(*player))
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
        let mut ship = Ship::new(ship_config, ship_id, Arc::new(DamageSystem::new()));
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

    loop {
        tick_interval.tick().await;
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
                    despawn_player(&net, connection_id, &mut players);
                }
                ServerEvent::ClientTimeout { connection_id } => {
                    info!("client {connection_id} timed out");
                    despawn_player(&net, connection_id, &mut players);
                }
                ServerEvent::ClientInput { connection_id, input } => {
                    if let Some(meta) = players.get(&connection_id) {
                        if let Some(ship) = sim.get_ship(meta.ship_id) {
                            apply_input(&net, &sim, &ship, connection_id, &input);
                        }
                    }
                }
                ServerEvent::ClientCommand { connection_id, command } => {
                    let ship_id = players.get(&connection_id).map(|m| m.ship_id);
                    match ship_id.and_then(|id| sim.get_ship(id)) {
                        Some(ship) => handle_command(&net, &sim, &ship, connection_id, &command),
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

        let tick_start = std::time::Instant::now();
        let result = sim.tick();

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

        interest.update_all_interests();

        for event in &result.events {
            net.broadcast_event(core_game_event(event));
        }

        let elapsed = tick_start.elapsed();
        tick_time_sum += elapsed;
        tick_time_max = tick_time_max.max(elapsed);
        tick_time_count += 1;
        if result.tick.0 % 150 == 0 {
            let avg_ms = tick_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            let stats = interest.stats();
            info!(
                "perf: tick={} sim+sync avg={:.2}ms max={:.2}ms ships={} players={} entities={{ships:{},players:{},stations:{},compartments:{},projectiles:{}}}",
                result.tick.0,
                avg_ms,
                tick_time_max.as_secs_f64() * 1000.0,
                ships.len(),
                players.len(),
                stats.ships,
                stats.players,
                stats.stations,
                stats.compartments,
                stats.projectiles,
            );
            tick_time_sum = Duration::ZERO;
            tick_time_max = Duration::ZERO;
            tick_time_count = 0;
        }
    }
}