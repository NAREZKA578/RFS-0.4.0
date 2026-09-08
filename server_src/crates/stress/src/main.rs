use anyhow::Result;
use rfs_core::packet::{InputActions, InputPacket};
use rfs_net::*;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tracing::{info, warn};

// Plan §19.1: bot harness, no render. Many bot clients spread across ships,
// measuring server tick cost and per-client traffic as bots scale 20 -> 200.
struct BotReport {
    connected: bool,
    state_updates: u64,
    last_entities: usize,
    sent: u64,
    acks: u64,
    errors: u64,
    bw_in_bps: f64,
    bw_out_bps: f64,
    server_tick: u64,
}

async fn run_bot(
    id: usize,
    server_addr: std::net::SocketAddr,
    duration: Duration,
    connected_now: Arc<AtomicUsize>,
    updates_total: Arc<AtomicU64>,
) -> BotReport {
    let mut report = BotReport {
        connected: false,
        state_updates: 0,
        last_entities: 0,
        sent: 0,
        acks: 0,
        errors: 0,
        bw_in_bps: 0.0,
        bw_out_bps: 0.0,
        server_tick: 0,
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
    let mut tick: u32 = 0;
    let mut live_entities: HashSet<rfs_core::entity::EntityId> = HashSet::new();

    loop {
        if start.elapsed() >= duration {
            break;
        }
        tokio::time::sleep(Duration::from_millis(16)).await;

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
                    warn!("bot-{id} connection failed: {reason:?}");
                    report.errors += 1;
                    return report;
                }
                ClientEvent::StateUpdate { snapshot } => {
                    report.state_updates += 1;
                    updates_total.fetch_add(1, Ordering::Relaxed);
                    report.last_entities = snapshot.entities.len() + snapshot.projectiles.len();
                    live_entities.clear();
                    live_entities.extend(snapshot.entities.iter().map(|e| e.entity_id));
                    live_entities.extend(snapshot.projectiles.iter().map(|p| p.entity_id));
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
                _ => {}
            }
        }

        if client.is_connected() {
            // Per-bot phase offset so inputs differ across the fleet.
            let phase = (tick as f32) * 0.05 + id as f32 * 0.7;
            let input = InputPacket {
                tick,
                delta_time: 1.0 / 30.0,
                move_forward: phase.sin() * 0.6,
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
        }
    }

    finish(client, report, live_entities.len())
}

fn finish(client: NetClient, mut report: BotReport, live: usize) -> BotReport {
    let stats = client.bandwidth_stats();
    report.bw_in_bps = stats.received_bps;
    report.bw_out_bps = stats.sent_bps;
    report.server_tick = client.server_tick().0;
    report.last_entities = report.last_entities.max(live);
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
    let mut handles = Vec::with_capacity(bots);
    for id in 0..bots {
        let addr = server_addr;
        let connected = connected_now.clone();
        let updates = updates_total.clone();
        handles.push(tokio::spawn(run_bot(id, addr, duration, connected, updates)));
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
    let entities: usize = reports.iter().map(|r| r.last_entities).sum();
    let bw_in: f64 = reports.iter().map(|r| r.bw_in_bps).sum();
    let bw_out: f64 = reports.iter().map(|r| r.bw_out_bps).sum();
    let n = reports.len().max(1);
    info!(
        "stress summary: bots={bots} connected_at_end={connected_peak} \
         state_updates={updates} avg_entities_per_bot={:.1} sent={sent} acks={acks} errors={errors} \
         bw_in_total={:.1}KB/s bw_out_total={:.1}KB/s bw_in_per_bot={:.1}KB/s",
        entities as f64 / n as f64,
        bw_in / 1024.0,
        bw_out / 1024.0,
        bw_in / 1024.0 / n as f64,
    );
    Ok(())
}
