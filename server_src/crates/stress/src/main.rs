use anyhow::Result;
use rfs_core::packet::{InputActions, InputPacket, ServerCommand};
use rfs_net::*;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tracing::{info, warn};

// Plan №19.1: bot harness, no render. Many bot clients spread across ships,
// measuring server tick cost and per-client traffic as bots scale 20 -> 200.
struct BotReport {
    connected: bool,
    state_updates: u64,
    last_entities: usize,
    sent: u64,
    acks: u64,
    errors: u64,
    /// Bug №175: a `ServerFull` rejection is a legitimate measurement outcome,
    /// not a fault — the harness is there to find the connection ceiling. It
    /// is counted separately so `errors` stays meaningful.
    rejected_server_full: u64,
    rejected_other: u64,
    events_dropped: u64,
    bw_in_bps: f64,
    bw_out_bps: f64,
    server_tick: u64,
    /// Latency samples in milliseconds.
    ///
    /// `NetClient::rtt()` has existed all along; the harness never called it, so
    /// a "stress test" produced a bandwidth number and no latency at all, while
    /// latency is the first thing the plan asks for.
    rtt_samples_ms: Vec<f64>,
    /// The first server tick this bot ever saw, and the number of times the
    /// observed tick moved.
    ///
    /// A single end-of-run tick cannot answer "did the server keep up": a server
    /// that stalled at tick 40 and one that ran to tick 600 both report a number,
    /// and only the second is healthy. `tick_advances == 0` is a stalled server.
    first_tick: Option<u64>,
    tick_advances: u64,
    /// Wall-clock seconds the bot was connected and sampling, so the observed
    /// tick rate can be compared against the nominal one.
    observed_secs: f64,
    /// Packets this bot received and sent, for comparison against the server's
    /// own `packets_per_round`.
    ///
    /// Bug №266: the server publishes a snapshot round every tick and emits
    /// roughly one packet per client, but clients observed only ~20 tick
    /// advances per second against the server's 30. Whether packets are lost on
    /// the wire or pile up unread on the client is the difference between a
    /// network problem and a receive-loop problem, and only the received count
    /// tells them apart. `BandwidthStats` already keeps it.
    rx_packets: u64,
    tx_packets: u64,
    /// Bug #267: deltas this bot dropped for base mismatch. Each one stalls its
    /// layer until the next resync, so this is the direct measure of how much
    /// of the tick stream the client actually missed.
    stalled_deltas: u64,
    /// Bug #267 follow-up: deltas dropped purely as stale/out-of-order (tick
    /// already applied). Base-mismatch measured zero, so this is the other
    /// candidate for the gap between server 30 Hz and the observed ~20 Hz.
    stale_deltas: u64,
    /// Base-mismatch drops split per layer. The total alone cannot tell one
    /// busy layer apart from every layer stalling briefly, and the two have
    /// different causes.
    stalls_by_layer: Vec<u64>,
    /// Last tick applied per layer. Both drop counters are zero, so the gap is
    /// about which layers carry data, not about loss. This makes that visible.
    layers: [u64; LAYER_COUNT],
    /// Bug №274: deltas APPLIED, counted at the apply site.
    ///
    /// This is the honest delivery figure, and it is reported next to
    /// `state_updates` precisely because the two disagree. `state_updates`
    /// counts reads of the coalescing `latest_state` slot, so it under-reads
    /// whenever several layers land in one server tick — measured on the real
    /// harness, 1 bot reported 6.10 Hz this way while its layers had reached
    /// the server's own tick with zero stalls. Reading one number as
    /// "throughput" when it is really "consumption rate" is what produced the
    /// false conclusions about the 200-bot wall.
    applied_deltas: u64,
    /// Bug №274: the same, per layer, so a slow layer is distinguishable from a
    /// slow consumer.
    applied_deltas_by_layer: Vec<u64>,
}

/// Bug №274: what the bot learned about the world from one snapshot.
///
/// A bot that only sends symmetric input leaves the world unchanged: ships
/// barely move, so layer 0 emits a delta every 2 ticks at best, and layer 2 —
/// players ride the ship through `local_to_world` — barely moves either.
/// Measured: layer 2 sent 155 deltas in 30 s where its 1-tick interval implies
/// ~900. The run then measures an idle world, not a loaded one, and every
/// conclusion drawn from it is about a system doing nothing.
///
/// So the bot has to actually fight. This walks the three links a real client
/// would walk to pick a post: the player's `current_ship` names the ship, the
/// ship's `stations` list names its stations, and each station's `station_type`
/// says what it is. It also picks a target ship to aim at, because a gun
/// firing into empty water produces no ballistics, no collision and no damage —
/// the entire combat path stays unexercised.
#[derive(Default)]
struct WorldView {
    /// A Gun station on the bot's own ship, if it found one.
    gun: Option<rfs_core::entity::EntityId>,
    /// Whether the bot is sitting in that gun right now.
    at_gun: bool,
    /// Position of an enemy ship to aim at, if any is in sight.
    target: Option<rfs_core::math::Vec3f>,
}

fn scan_world(snapshot: &Snapshot, connection_id: Option<u32>) -> WorldView {
    let me = match connection_id {
        Some(id) => id,
        None => return WorldView::default(),
    };

    let my_ship = snapshot
        .entities
        .iter()
        .find(|e| {
            e.player_data
                .as_ref()
                .map(|p| p.player_id == me)
                .unwrap_or(false)
        })
        .and_then(|e| e.player_data.as_ref())
        .and_then(|p| p.current_ship);

    let my_ship = match my_ship {
        Some(ship) => ship,
        None => return WorldView::default(),
    };

    let stations = snapshot
        .entities
        .iter()
        .find(|e| e.entity_id == my_ship)
        .and_then(|e| e.ship_data.as_ref())
        .map(|s| s.stations.clone())
        .unwrap_or_default();

    let gun = stations.into_iter().find(|sid| {
        snapshot
            .entities
            .iter()
            .find(|e| e.entity_id == *sid)
            .and_then(|e| e.station_data.as_ref())
            .map(|s| s.station_type == rfs_core::packet::StationType::Gun)
            .unwrap_or(false)
    });

    // Occupation is confirmed by the server echoing it back: `current_station`
    // is set only after `occupy_station` succeeds, so this cannot be faked by
    // the bot sending the command and hoping.
    let at_gun = gun
        .and_then(|sid| {
            snapshot
                .entities
                .iter()
                .find(|e| {
                    e.player_data
                        .as_ref()
                        .map(|p| p.player_id == me)
                        .unwrap_or(false)
                })
                .and_then(|e| e.player_data.as_ref())
                .map(|p| p.current_station == Some(sid))
        })
        .unwrap_or(false);

    // Aim at the nearest OTHER ship. Cycling is unnecessary: the ships are
    // static in this scenario, so the nearest one stays the nearest one, and a
    // moving turret is itself load on layer 1.
    let my_pos = snapshot
        .entities
        .iter()
        .find(|e| e.entity_id == my_ship)
        .map(|e| e.transform.position);

    let target = my_pos.and_then(|from| {
        snapshot
            .entities
            .iter()
            .filter(|e| {
                e.ship_data.is_some() && e.entity_id != my_ship
            })
.map(|e| e.transform.position)
            .min_by(|a, b| {
                let da = (*a - from).length_squared();
                let db = (*b - from).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    WorldView { gun, at_gun, target }
}

async fn run_bot(
    id: usize,
    server_addr: std::net::SocketAddr,
    duration: Duration,
    connected_now: Arc<AtomicUsize>,
    updates_total: Arc<AtomicU64>,
    rejected_server_full: Arc<AtomicUsize>,
) -> BotReport {
    let mut report = BotReport {
        connected: false,
        state_updates: 0,
        last_entities: 0,
        sent: 0,
        acks: 0,
        errors: 0,
        rejected_server_full: 0,
        rejected_other: 0,
        events_dropped: 0,
        bw_in_bps: 0.0,
        bw_out_bps: 0.0,
        server_tick: 0,
        rtt_samples_ms: Vec::new(),
        first_tick: None,
        tick_advances: 0,
        observed_secs: 0.0,
        rx_packets: 0,
        tx_packets: 0,
        stalled_deltas: 0,
        stale_deltas: 0,
    stalls_by_layer: vec![0; LAYER_COUNT],
        layers: [0; LAYER_COUNT],
        // Bug №274: read from the client at report time.
        applied_deltas: 0,
        applied_deltas_by_layer: vec![0; LAYER_COUNT],
    };

    let client = match NetClient::new(ClientConfig {
        server_addr,
        player_name: format!("bot-{id}"),
        ..ClientConfig::default()
    })
    .await
    {
        Ok(client) => client,
        Err(e) => {
            warn!("bot-{id} bind failed: {e}");
            report.errors += 1;
            return report;
        }
    };
    let mut events = match client.get_event_receiver() {
        Some(rx) => rx,
        None => return report,
    };
    client.connect();

    let start = Instant::now();
    let mut rtt_measured = false;
    let mut tick: u32 = 0;
    let mut last_server_tick: u64 = u64::MAX;
    let mut live_entities: HashSet<rfs_core::entity::EntityId> = HashSet::new();
    // Bug №274: load realism. See `scan_world` for why a bot that only sends
    // symmetric input measures an idle world.
    let mut world = WorldView::default();
    let mut entered_gun = false;
    let mut fire_cooldown: u32 = 0;

    loop {
        if start.elapsed() >= duration {
            break;
        }
        // Bug №175: 16 ms gave ~62 input packets per second against a 30 Hz
        // server, so the bots injected input at twice the simulation rate and
        // the measurement was of the wrong thing. Drain at 120 Hz and only
        // emit input when the server actually advanced a tick.
        tokio::time::sleep(Duration::from_millis(8)).await;

        while let Ok(event) = events.try_recv() {
            match event {
                ClientEvent::Connected { .. } => {
                    report.connected = true;
                    connected_now.fetch_add(1, Ordering::Relaxed);
                }
                ClientEvent::Disconnected { reason } => {
                    warn!("bot-{id} disconnected: {reason:?}");
                    client.disconnect(reason);
                    connected_now.fetch_sub(1, Ordering::Relaxed);
                    report.connected = false;
                    return finish(client, report, live_entities.len());
                }
                ClientEvent::ConnectionFailed { reason } => {
                    // Bug №175: a full server is a result, not an error.
                    match reason {
                        rfs_core::packet::ConnectRejectReason::ServerFull => {
                            report.rejected_server_full += 1;
                            rejected_server_full.fetch_add(1, Ordering::Relaxed);
                            info!("bot-{id} rejected: server full");
                        }
                        other => {
                            warn!("bot-{id} connection failed: {other:?}");
                            report.rejected_other += 1;
                        }
                    }
                    return report;
                }
                ClientEvent::StateUpdate { snapshot } => {
                    report.state_updates += 1;
                    updates_total.fetch_add(1, Ordering::Relaxed);
                    report.last_entities = snapshot.entities.len() + snapshot.projectiles.len();
                    live_entities.clear();
                    live_entities.extend(snapshot.entities.iter().map(|e| e.entity_id));
                    live_entities.extend(snapshot.projectiles.iter().map(|p| p.entity_id));
                    // Bug №274: re-scan on every state. The world changes as
                    // ships move and guns get occupied, and the bot needs the
                    // current gun and a current target to be worth anything.
                    world = scan_world(&snapshot, client.connection_id());
                }
                ClientEvent::EntityUpdate { entity_id, .. } => {
                    live_entities.insert(entity_id);
                }
                ClientEvent::EntityRemoved { entity_id } => {
                    live_entities.remove(&entity_id);
                }
                ClientEvent::ProjectileUpdate { projectile_id, .. } => {
                    live_entities.insert(projectile_id);
                }
                ClientEvent::ProjectileRemoved { projectile_id } => {
                    live_entities.remove(&projectile_id);
                }
                ClientEvent::InputAck { accepted, .. } => {
                    report.acks += 1;
                    if !accepted {
                        report.errors += 1;
                    }
                }
                ClientEvent::Error { .. } => {
                    report.errors += 1;
                }
                // The client starts with a placeholder RTT of 100 ms and only
                // replaces it once a heartbeat comes back. Sampling before that
                // records the placeholder, and with a long heartbeat interval
                // those placeholders dominate the tail — which is exactly what a
                // first run showed: p95, p99 and max all pinned at 100 ms.
                ClientEvent::RttUpdate { .. } => {
                    rtt_measured = true;
                }
                _ => {}
            }
        }

        // Bug №175: the client coalesces full snapshots, so the newest one sits
        // in a slot rather than the queue. Drain it here, or the harness would
        // under-count state updates by exactly the amount the client merged.
        if let Some(ClientEvent::StateUpdate { snapshot }) = client.take_latest_state() {
            report.state_updates += 1;
            updates_total.fetch_add(1, Ordering::Relaxed);
            report.last_entities = snapshot.entities.len() + snapshot.projectiles.len();
            live_entities.clear();
            live_entities.extend(snapshot.entities.iter().map(|e| e.entity_id));
            live_entities.extend(snapshot.projectiles.iter().map(|p| p.entity_id));
        }
        report.events_dropped = client.events_dropped();

        if client.is_connected() {
            // Bug №175: send one input per SERVER tick, not per harness
            // iteration. The loop runs at 120 Hz and the server simulates at
            // 30 Hz, so gating on the observed tick is what makes the offered
            // load match the simulation rate.
            let server_tick = client.server_tick().0;
            if server_tick != last_server_tick {
                last_server_tick = server_tick;
                // Per-bot phase offset so inputs differ across the fleet.
                let phase = (tick as f32) * 0.05 + id as f32 * 0.7;

                // Bug №274: asymmetric throttle. `phase.sin() * 0.6` averaged to
                // zero, so ships barely moved — and a ship that does not move
                // produces almost no deltas: layer 0 is on a 2-tick interval and
                // layer 2 rides the ship, so both went nearly silent. Measured:
                // layer 2 sent 155 deltas in 30 s where its 1-tick interval
                // implies ~900. Biasing the throttle positive makes the ship
                // actually sail, which is what loads layers 0 and 2.
                let throttle = (0.55 + 0.45 * phase.sin()).clamp(0.0, 1.0);

                let input = InputPacket {
                    tick,
                    delta_time: 1.0 / 30.0,
                    move_forward: throttle,
                    move_right: (phase * 0.5).cos() * 0.3,
                    move_up: 0.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    roll: 0.0,
                    actions: InputActions::NONE,
                    station_interaction: None,
                };
                client.send_input(input);
                report.sent += 1;
                tick = tick.wrapping_add(1);

                // Bug №274: occupy the gun once we know which one it is.
                // Firing requires occupation — the server rejects a shot from a
                // non-occupant — so this has to land before the first round.
                if !entered_gun {
                    if let Some(gun) = world.gun {
                        // The server ignores this field: `EnterStation` occupies
                        // with the AUTHENTICATED connection's player id, never
                        // with the client-supplied one (spoofable). So the bot
                        // has nothing correct to put here, and nil is the
                        // honest value — it says "not claiming an identity".
                        client.send_command(ServerCommand::EnterStation {
                            player: rfs_core::entity::EntityId::nil(),
                            station: gun,
                        });
                        entered_gun = true;
                    }
                }

                // Bug №274: fire at an enemy ship. This is what loads layer 3
                // (projectiles), ballistics, collision and damage — the whole
                // combat path. A gun firing into empty water exercises none of
                // it. `FireWeapon` also slews the turret to the target, so the
                // shot can actually land.
                //
                // Gated to ~7.5 shots/s per bot: the gun's own cooldown gates
                // the real rate, and 200 bots at 30 Hz would otherwise put
                // 6000 commands/s on the reliable channel for no extra load.
                if world.at_gun {
                    if fire_cooldown == 0 {
                        fire_cooldown = 4;
                        if let (Some(gun), Some(target)) = (world.gun, world.target) {
                            client.send_command(ServerCommand::FireWeapon {
                                station: gun,
                                target_pos: Some(target),
                            });
                        }
                    } else {
                        fire_cooldown -= 1;
                    }
                }
            }
        }

        // Latency and convergence, sampled alongside the input load.
        //
        // Both values are read from the client, which already tracks them; the
        // harness simply never asked, so a run could not answer "is the network
        // holding" or "is the server keeping up". Sampling is per harness
        // iteration and cheap, and the RTT list is bounded by the run length.
        if client.is_connected() {
            let observed = client.server_tick().0;
            if report.first_tick.is_none() {
                report.first_tick = Some(observed);
            }
            if observed != report.server_tick {
                report.server_tick = observed;
                report.tick_advances += 1;
            }
            if rtt_measured {
                report.rtt_samples_ms.push(client.rtt().as_secs_f64() * 1000.0);
            }
            report.observed_secs = start.elapsed().as_secs_f64();
        }
    }

    finish(client, report, live_entities.len())
}

fn finish(client: NetClient, mut report: BotReport, live: usize) -> BotReport {
    let stats = client.bandwidth_stats();
    report.bw_in_bps = stats.received_bps;
    report.bw_out_bps = stats.sent_bps;
    // Bug #266: the packet counts, not just the byte rates.
    report.rx_packets = stats.received_packets_total;
    report.tx_packets = stats.sent_packets_total;
    report.stalled_deltas = client.base_mismatch_drops();
    report.stale_deltas = client.stale_delta_drops();
    report.stalls_by_layer = client.base_mismatch_drops_by_layer();
    report.layers = client.applied_layer_ticks();
    // Bug №274: the honest delivery figure, read alongside `state_updates`.
    // If the two disagree sharply, the gap is coalescing in the state slot,
    // not loss on the wire.
    report.applied_deltas = client.applied_delta_count();
    report.applied_deltas_by_layer = client.applied_delta_count_by_layer();
    report.server_tick = client.server_tick().0;
    report.last_entities = report.last_entities.max(live);
    // Bug №175: capture backpressure before the client goes away.
    report.events_dropped = client.events_dropped();
    // Clean shutdown: abort recv/send tasks so the runtime can exit,
    // and let the server release the player immediately (not via timeout).
    client.disconnect(rfs_core::packet::DisconnectReason::ClientQuit);
    report
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let server_addr = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("127.0.0.1:7777")
        .parse()?;
    let bots = args
        .get(2)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20)
        .clamp(1, 1000);
    let duration = Duration::from_secs(
        args.get(3).and_then(|s| s.parse::<u64>().ok()).unwrap_or(20),
    );

    info!("stress: {bots} bots -> {server_addr} for {}s", duration.as_secs());

    let connected_now = Arc::new(AtomicUsize::new(0));
    let updates_total = Arc::new(AtomicU64::new(0));
    // Bug №175: ServerFull is measured, not an error.
    let rejected_server_full = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::with_capacity(bots);
    for id in 0..bots {
        let addr = server_addr;
        let connected = connected_now.clone();
        let updates = updates_total.clone();
        let full = rejected_server_full.clone();
        handles.push(tokio::spawn(run_bot(id, addr, duration, connected, updates, full)));
        // Stagger connects so the accept path isn't hit by a SYN-like burst.
        tokio::time::sleep(Duration::from_millis(15)).await;
        if id % 20 == 0 {
            info!(
                "stress progress: launched {}/{bots}, connected={}",
                id + 1,
                connected_now.load(Ordering::Relaxed)
            );
        }
    }

    let deadline = Instant::now() + duration + Duration::from_secs(10);
    let mut reports = Vec::with_capacity(bots);
    for (i, handle) in handles.into_iter().enumerate() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, handle).await {
            Ok(Ok(report)) => reports.push(report),
            Ok(Err(e)) => warn!("bot task panicked: {e}"),
            Err(_) => warn!("bot task {i} timed out"),
        }
    }

    let connected_peak = reports.iter().filter(|r| r.connected).count();
    let updates: u64 = reports.iter().map(|r| r.state_updates).sum();
    let sent: u64 = reports.iter().map(|r| r.sent).sum();
    let acks: u64 = reports.iter().map(|r| r.acks).sum();
    let errors: u64 = reports.iter().map(|r| r.errors).sum();
    let rejected_full: u64 = reports.iter().map(|r| r.rejected_server_full).sum();
    let rejected_other: u64 = reports.iter().map(|r| r.rejected_other).sum();
    let dropped: u64 = reports.iter().map(|r| r.events_dropped).sum();
    let entities: usize = reports.iter().map(|r| r.last_entities).sum();
    let bw_in: f64 = reports.iter().map(|r| r.bw_in_bps).sum();
    let bw_out: f64 = reports.iter().map(|r| r.bw_out_bps).sum();
    let n = reports.len().max(1);

    // The two numbers the plan actually asks for.
    //
    // Latency: every bot's RTT samples pooled into one list, so a percentile is
    // over the whole run rather than over a per-bot average — an average of
    // averages hides exactly the tail the plan is looking for.
    let mut all_rtt: Vec<f64> = reports.iter().flat_map(|r| r.rtt_samples_ms.clone()).collect();
    all_rtt.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = |p: f64| -> f64 {
        if all_rtt.is_empty() {
            return 0.0;
        }
        let idx = ((all_rtt.len() as f64 - 1.0) * p).round() as usize;
        all_rtt[idx.min(all_rtt.len() - 1)]
    };
    let rtt_mean = if all_rtt.is_empty() {
        0.0
    } else {
        all_rtt.iter().sum::<f64>() / all_rtt.len() as f64
    };

    // Convergence: a bot saw a live server if the tick it observed actually
    // moved during the run. `tick_advances == 0` means the server's tick never
    // changed for that client, whatever the final number happens to be.
    let live_bots = reports.iter().filter(|r| r.tick_advances > 0).count();
    let stalled_bots = reports.iter().filter(|r| r.connected && r.tick_advances == 0).count();
    let ticks_advanced: u64 = reports.iter().map(|r| r.tick_advances).sum();
    let observed_secs: f64 = reports.iter().map(|r| r.observed_secs).sum();
    let observed_hz = if observed_secs > 0.0 {
        ticks_advanced as f64 / observed_secs
    } else {
        0.0
    };
    // Per-bot rate against the nominal one: one shared figure would let a
    // handful of fast bots hide a crowd of stalled ones.
    let per_bot_hz: Vec<f64> = reports
        .iter()
        .filter(|r| r.observed_secs > 0.0 && r.tick_advances > 0)
        .map(|r| r.tick_advances as f64 / r.observed_secs)
        .collect();
    let per_bot_sorted = {
        let mut v = per_bot_hz.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v
    };
    let per_bot_p05 = per_bot_sorted
        .first()
        .copied()
        .unwrap_or(0.0);
    let per_bot_p50 = if per_bot_sorted.is_empty() {
        0.0
    } else {
        per_bot_sorted[per_bot_sorted.len() / 2]
    };
    // Bug №175: the cross-check that was previously computed and thrown away.
    // `updates_total` is maintained by every bot as events arrive, so the sum
    // of per-bot reports must match it; a mismatch means the harness lost
    // events somewhere and the numbers above cannot be trusted.
    let cross_checked = updates_total.load(Ordering::Relaxed);
    if cross_checked != updates {
        warn!(
            "stress: event accounting mismatch — updates_total={cross_checked} \
             but per-bot sum={updates}; some events were dropped or coalesced"
        );
    }
    if dropped > 0 {
        warn!("stress: {dropped} client events were dropped (bounded queue full)");
    }
    info!(
        "stress summary: bots={bots} connected_at_end={connected_peak} \
         state_updates={updates} avg_entities_per_bot={:.1} sent={sent} acks={acks} errors={errors} \
         rejected_server_full={rejected_full} rejected_other={rejected_other} events_dropped={dropped} \
         bw_in_total={:.1}KB/s bw_out_total={:.1}KB/s bw_in_per_bot={:.1}KB/s",
        entities as f64 / n as f64,
        bw_in / 1024.0,
        bw_out / 1024.0,
        bw_in / 1024.0 / n as f64,
    );
    info!(
        "stress network: rtt_mean={rtt_mean:.2}ms p50={:.2}ms p95={:.2}ms p99={:.2}ms max={:.2}ms samples={}",
        percentile(0.50),
        percentile(0.95),
        percentile(0.99),
        all_rtt.last().copied().unwrap_or(0.0),
        all_rtt.len(),
    );
    info!(
        "stress convergence: live_bots={live_bots}/{bots} stalled_bots={stalled_bots} \
         ticks_advanced={ticks_advanced} observed_hz_aggregate={observed_hz:.2} \
         per_bot_hz_p05={per_bot_p05:.2} per_bot_hz_p50={per_bot_p50:.2} nominal_hz={}",
rfs_core::time::TICK_RATE as f32,
    );
    // Bug #267: the client-apply rate, from events actually delivered to the
    // harness, not from the harness's own sampling loop (that loop is starved
    // under 30-bot load and under-reads the server). If this is ~30 while
    // observed_hz is ~20, the observed_hz gap is the sampling loop, not the
    // network.
    let updates: u64 = reports.iter().map(|r| r.state_updates).sum();
    let update_hz = updates as f64 / observed_secs;
    info!(
        "stress applied: state_updates_total={updates} state_updates_hz_aggregate={update_hz:.2}",
    );

    // Bug №274: the same figure measured where the deltas are actually applied,
    // next to the coalesced one above.
    //
    // `state_updates` counts reads of a single overwrite slot, so when several
    // layers land in one server tick they collapse into one observation. It is
    // a consumption rate, not a delivery rate. Reporting only the former is
    // what made a healthy 1-bot client look like it delivered 6 Hz, and it is
    // why the two numbers must never be compared against each other or against
    // the server's tick rate as if they measured the same thing.
    let applied_deltas: u64 = reports.iter().map(|r| r.applied_deltas).sum();
    let applied_deltas_hz = if observed_secs > 0.0 {
        applied_deltas as f64 / observed_secs
    } else {
        0.0
    };
    let applied_by_layer: Vec<u64> = (0..LAYER_COUNT)
        .map(|l| reports.iter().map(|r| r.applied_deltas_by_layer.get(l).copied().unwrap_or(0)).sum())
        .collect();
    info!(
        "stress delivery: applied_deltas_total={applied_deltas} applied_deltas_hz={applied_deltas_hz:.2} \
         by_layer={applied_by_layer:?} (state_updates_hz={update_hz:.2} is a COALESCED consumption \
         rate, not delivery — see bug №274)",
    );
    // Name the discrepancy rather than leaving two numbers side by side.
    if applied_deltas > 0 && (updates as f64) < applied_deltas as f64 * 0.8 {
        warn!(
            "stress: state_updates ({updates}) is well below applied_deltas ({applied_deltas}) — \
             the state slot coalesced updates. Read applied_deltas_hz={applied_deltas_hz:.2} as the \
             delivery rate; state_updates_hz={update_hz:.2} under-reads it by design and is not a \
             network measurement"
        );
    }
    if stalled_bots > 0 {
        warn!(
            "stress: {stalled_bots} connected bot(s) never saw the server tick advance; \
             the server is not simulating, or these clients are starved"
        );
    }
    // Bug №266: received packets per second, next to the server's own
    // `packets_per_round`. If these match, nothing is lost on the wire and the
    // shortfall is the receive path; if received is materially lower, packets
    // are being dropped before the bot sees them.
    //
    // Per-bot, from per-bot totals over per-bot wall clock. An earlier version
    // summed the per-bot elapsed time and divided by that, which made the
    // divisor grow with the fleet and reported 645 packets/s as 3.1 — the
    // denominator was wrong by exactly the number of bots, and the number it
    // produced looked like catastrophic packet loss.
    let rx_total: u64 = reports.iter().map(|r| r.rx_packets).sum();
    let tx_total: u64 = reports.iter().map(|r| r.tx_packets).sum();
    let rx_per_bot = reports
        .iter()
        .map(|r| {
            if r.observed_secs > 0.0 {
                r.rx_packets as f64 / r.observed_secs
            } else {
                0.0
            }
        })
        .sum::<f64>()
        / n as f64;
    let tx_per_bot = reports
        .iter()
        .map(|r| {
            if r.observed_secs > 0.0 {
                r.tx_packets as f64 / r.observed_secs
            } else {
                0.0
            }
        })
        .sum::<f64>()
        / n as f64;
    // Bug #267: the direct measure of what the client missed. A non-zero value
    // here is a layer that will not advance until its next resync, which is the
    // mechanism behind a shortfall in the observed tick rate.
    let stalled: u64 = reports.iter().map(|r| r.stalled_deltas).sum();
    let bots_with_stalls = reports.iter().filter(|r| r.stalled_deltas > 0).count();
    let stale: u64 = reports.iter().map(|r| r.stale_deltas).sum();
    let bots_with_stale = reports.iter().filter(|r| r.stale_deltas > 0).count();
    let by_layer: Vec<u64> = (0..LAYER_COUNT)
        .map(|l| reports.iter().map(|r| r.stalls_by_layer.get(l).copied().unwrap_or(0)).sum())
        .collect();
    info!(
        "stress packets: rx_total={rx_total} tx_total={tx_total} \
         rx_per_bot_per_s={rx_per_bot:.1} tx_per_bot_per_s={tx_per_bot:.1} \
         stalled_deltas={stalled} bots_with_stalls={bots_with_stalls} \
         stalls_by_layer={by_layer:?} \
         stale_deltas={stale} bots_with_stale={bots_with_stale}",
    );

    // Bug #267: which layers actually advance, and how far behind the server
    // each one is per client. If layer0 peaks at ~tick/2 and the others sit at
    // near-zero applied ticks, the gap is the layer cadence, not the channel.
    let l0 = reports.iter().map(|r| r.layers[0]).max().unwrap_or(0);
    let l1 = reports.iter().map(|r| r.layers[1]).max().unwrap_or(0);
    let l2 = reports.iter().map(|r| r.layers[2]).max().unwrap_or(0);
    let l3 = reports.iter().map(|r| r.layers[3]).max().unwrap_or(0);
    let srv_tick = reports.iter().map(|r| r.server_tick).max().unwrap_or(0);
    info!(
        "stress layers: server_tick_max={srv_tick} layer0_max={l0} layer1_max={l1} layer2_max={l2} layer3_max={l3}",
    );
    Ok(())
}
