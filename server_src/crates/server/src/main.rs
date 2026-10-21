//! The match binary (plan H5, «Server structure»): thin wiring only — argument
//! parsing, the net server, the per-tick loop and its perf accounting run the
//! functions the `rfs_server` library exports, so tools can reuse them
//! untouched.

use anyhow::{Context, Result};
use rfs_core::entity::{EntityId, PlayerEntity, ProjectileEntity};
use rfs_core::math::{Quatf, Transform, Vec3f};
use rfs_core::packet::{InputAckPacket, PlayerPosture, ServerCommand};
use rfs_core::time::TICK_DURATION_MS;
use rfs_damage::damage::DamageSystem;
use rfs_net::*;
use rfs_server::config::MatchConfig;
use rfs_server::control::TickCadence;
use rfs_server::match_::{
    arg_value, despawn_player, load_ship_classes, load_world, resolve_input_target,
    resolve_ship_count, spawn_is_blocked, spawn_player, PlayerMeta, TrackedShip,
    DEFAULT_SHIP_CLASS_ID, SHIP_ID, SHIP_ID_STRIDE, SHIP_SPACING,
};
use rfs_server::net::handle_command;
use rfs_server::tick::{apply_input, core_game_event, resolve_deferred_shots, DeferredShot};
use rfs_server::votes::{handle_vote_kick, VoteKickState};
use rfs_sim::tick::TickSystem;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?),
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
    // Bug №699: the network layer needs the world's DESCRIPTION, so it can
    // hand it to every client that connects — the layers carry ships, players
    // and shots, but not the structures, and a client cannot draw a world it
    // was never told about.
    net.set_world_description(world.to_description());

    // Owner decision (2026-10-19): the config file is created with defaults
    // on the first run and read on every start; `--config` moves it off the
    // working directory's default name. A broken file is a hard stop, not a
    // silent-default run.
    let config_path = PathBuf::from(
        arg_value(&args, "--config")
            .unwrap_or_else(|| rfs_server::config::DEFAULT_CONFIG_PATH.to_string()),
    );
    let match_config = MatchConfig::load(&config_path).context("server config")?;
    info!(
        "config: {} (vote-kick enabled: {}, {} votes / {}% share / {} ticks)",
        config_path.display(),
        match_config.vote_kick.enabled,
        match_config.vote_kick.min_votes,
        (match_config.vote_kick.majority * 100.0) as u32,
        match_config.vote_kick.window_ticks
    );
    // Bug №216: the secret a client has to present. A match without one is
    // open, and the log says so out loud — a server that looks protected and
    // is not is worse than one that says it is open.
    match match_config.auth_token.as_deref() {
        Some(token) if !token.is_empty() => {
            info!("match requires a secret ({} bytes)", token.len());
        }
        _ => {
            warn!("match has NO secret: every client that reaches this port is admitted");
        }
    }
    net.set_auth_token(match_config.auth_token.clone());

    struct ShipEntry {
        id: EntityId,
        tracked: TrackedShip,
    }

    let mut sim = TickSystem::new();
    sim.set_world(world.clone())
        .context("map validation failed")?;
    // Bug №667: ships are built from the class registry (disk-first, built-in
    // frigate fallback), not by calling `create_default_frigate()` per ship —
    // one door from a class to a ship, with the JSON path live.
    let classes = load_ship_classes(config_path.parent()).context("ship classes")?;
    let mut ships: Vec<ShipEntry> = Vec::with_capacity(ship_count);
    // Plan §19.1: several ships so bots spread across crews instead of one point.
    for i in 0..ship_count {
        let ship_id = EntityId::new(SHIP_ID + i as u64 * SHIP_ID_STRIDE);
        let ship = classes
            .create_ship(
                DEFAULT_SHIP_CLASS_ID,
                ship_id,
                Arc::new(DamageSystem::new()),
            )
            .with_context(|| format!("building ship {i} from class {DEFAULT_SHIP_CLASS_ID}"))?;
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
        ships.push(ShipEntry {
            id: ship_id,
            tracked,
        });
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
    // Bug №630: shots that wait for their turret. Match-scoped state next to
    // the players map — the same kind of plumbing the projectile id counter is.
    let mut deferred_shots: Vec<DeferredShot> = Vec::new();
    // Owner decision (2026-10-19): VoteKick lives and its strength is
    // config-driven; the machine runs on the sim's own tick count.
    let mut vote_kick = VoteKickState::new(match_config.vote_kick);
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
                ServerEvent::ClientConnected {
                    connection_id,
                    addr,
                } => {
                    info!("client {connection_id} connected from {addr}");
                    let ship_idx = connection_id as usize % ships.len();
                    if let Some(ship) = sim.get_ship(ships[ship_idx].id) {
                        players.insert(
                            connection_id,
                            spawn_player(&net, &ship, connection_id, "player"),
                        );
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
                ServerEvent::ClientInput {
                    connection_id,
                    input,
                } => {
                    // Bug №82/#71: echo back the client's own input.tick so its
                    // pending map can be keyed by the same value, and always
                    // answer — reject with a negative ack when the client is
                    // unknown, so its pending inputs cannot grow forever.
                    let accepted = match resolve_input_target(&players, &sim, connection_id) {
                        Some((player_id, ship)) => {
                            apply_input(&sim, &ship, player_id, &input);
                            true
                        }
                        None => false,
                    };
                    net.send_input_ack(
                        connection_id,
                        InputAckPacket {
                            tick: input.tick,
                            accepted,
                        },
                    );
                }
                ServerEvent::ClientCommand {
                    connection_id,
                    command,
                } => {
                    let actor = players
                        .get(&connection_id)
                        .map(|m| (m.ship_id, m.player_entity_id));
                    match actor.and_then(|(ship_id, _)| sim.get_ship(ship_id)) {
                        Some(ship) => {
                            let player_id = players
                                .get(&connection_id)
                                .map(|m| m.player_entity_id)
                                .unwrap_or(EntityId::nil());
                            // VoteKick reads server-wide state (net, the players
                            // map) the per-ship command translator does not see,
                            // so it is routed here directly; everything else
                            // keeps the per-ship path.
                            if let ServerCommand::VoteKick { .. } = &command.command {
                                handle_vote_kick(
                                    &net,
                                    &sim,
                                    connection_id,
                                    player_id,
                                    &command,
                                    &mut vote_kick,
                                    &mut players,
                                );
                            } else {
                                handle_command(
                                    &net,
                                    &sim,
                                    &ship,
                                    connection_id,
                                    player_id,
                                    &mut deferred_shots,
                                    &command,
                                );
                            }
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
        // Bug №630: deferred shots resolve right after the tick's slew step
        // and before the entity sync, so a shell whose turret arrived during
        // this tick is part of this tick's published state, not the next
        // one's.
        resolve_deferred_shots(&sim, &mut deferred_shots);
        // VoteKick expiry rides the simulation's tick count for the same
        // reason the deferred shots do: a skipped cadence deadline must not
        // shorten either window (№614). An expired vote is logged, not lost
        // silently.
        if let Some(target) = vote_kick.resolve() {
            info!(
                "vote-kick against player {:?} expired with no verdict",
                target.0
            );
        }
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
            let compartments: Vec<EntityId> = entry.tracked.compartments.keys().copied().collect();
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
            let entity_sync_ms =
                entity_sync_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            let interest_ms = interest_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            let broadcast_ms = broadcast_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
            // Bug №273: the previously unmeasured half of the iteration.
            let event_drain_ms =
                event_drain_time_sum.as_secs_f64() * 1000.0 / tick_time_count as f64;
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
            let packets_delta =
                packets.saturating_sub(std::mem::replace(&mut last_packets, packets));
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
                info!(
                    "map: {} projectile(s) spent on obstacles",
                    sim.obstacle_hits()
                );
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
                let (dominant, dominant_ms) = *phases
                    .iter()
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .expect("phase list is never empty");
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
