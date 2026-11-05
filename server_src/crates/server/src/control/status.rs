//! The server's shared status snapshot: the numbers the tick loop already
//! computes for its `perf:` line, published into a structure the control port
//! reads instead of computing anything itself (plan section 6.3: a read never
//! re-derives state, it reads what the loop produced — a frequent panel poll
//! then costs nothing the loop does not already pay).
//!
//! The update cadence follows the plan (Приложение А, А.1 item 1): cheap
//! numbers — current tick, uptime, entity counts — every tick; the players
//! list and the network totals once per second. The `perf:` line in
//! `main.rs` is untouched by this: it keeps its own accumulation, and the
//! same window aggregates ride along here (plan risk item 2: a small
//! structure the loop updates and the port reads).

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Nominal cadence constants ride with the snapshot so a reader never has to
/// guess the budget the averages are compared against.
use rfs_core::time::{TICK_DURATION_MS, TICK_RATE};

/// The tick-loop's own perf accounting, as published once per report window
/// (~5 s — the same numbers the `perf:` line prints, see `main.rs`). Field
/// names mirror the log line's words so a reader can compare them one to one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PerfStatus {
    /// Per-window average over `window_ticks` ticks — the `sim+sync avg`.
    pub sim_sync_avg_ms: f64,
    /// The single largest tick of the window — the `sim+sync max`.
    pub sim_sync_max_ms: f64,
    /// Per-phase window averages, in the order the `perf` line prints them.
    pub phase_events_ms: f64,
    pub phase_sim_ms: f64,
    pub phase_entity_sync_ms: f64,
    pub phase_interest_ms: f64,
    pub phase_broadcast_ms: f64,
    /// The whole iteration (`loop avg`) against the tick budget — the number
    /// that separates "the sim is slow" from "the loop around it is".
    pub loop_avg_ms: f64,
    pub loop_max_ms: f64,
    pub budget_ms: f64,
    /// Cumulative counters since server start (not per window): the `perf:`
    /// line prints per-window deltas, here the totals live next to them.
    pub ticks_ran: u64,
    pub ticks_skipped_cumulative: u64,
    /// The window's own deltas — the same figures the log line carries.
    pub window_ticks: u64,
    pub window_rounds: u64,
    pub window_packets: u64,
    pub rounds_per_tick: f64,
    pub packets_per_round: f64,
    pub window_sent_deltas_by_layer: Vec<u64>,
    pub window_first_fulls: u64,
    /// Working set of the server process at window close (Windows; 0 off it).
    pub working_set_mb: f64,
}

/// Network totals plus per-second rates, refreshed once per second. Sources
/// are the net server's own counters (`bandwidth_stats`, `snapshot_totals`,
/// `sent_deltas_by_layer`, the dropped- and reject-counters) — none of them
/// recomputed, only copied.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetStatus {
    pub sent_bps: f64,
    pub received_bps: f64,
    pub sent_bytes_total: u64,
    pub received_bytes_total: u64,
    pub sent_packets_total: u64,
    pub received_packets_total: u64,
    pub avg_packet_size_sent: f64,
    pub avg_packet_size_received: f64,
    pub snapshot_rounds_total: u64,
    pub snapshot_packets_total: u64,
    pub sent_deltas_by_layer_total: Vec<u64>,
    pub dropped_events: u64,
    pub dropped_outbox: u64,
    pub first_fulls_total: u64,
    pub session_rejects: u64,
    pub pending_challenges: usize,
}

/// One player line of the `players` listing. The name rides from the player
/// bookkeeping (plan risk item 1: it is the placeholder the account still uses
/// until the entrance packet's name is threaded through — that fix is stage 3,
/// the listing is honest about what the server knows today).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerStatus {
    pub connection_id: u32,
    pub name: String,
    pub addr: String,
    /// The ship the player was spawned onto, as the entity id Word.
    pub ship_id: u64,
    /// Smoothed round-trip estimate in milliseconds.
    pub ping_ms: f64,
    /// Seconds since the connection entered the connected state.
    pub age_s: u64,
}

/// What the interest manager is tracking right now — the same numbers the
/// `perf:` line prints under `entities={...}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntitiesStatus {
    pub ships: usize,
    pub players: usize,
    pub stations: usize,
    pub compartments: usize,
    pub projectiles: usize,
}

/// The whole snapshot. Every field carries `#[serde(default)]` so an older
/// reader and a newer writer disagree only in the fields each side knows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerStatus {
    /// The lifecycle word: stage 1 runs `starting` → `running` and nothing
    /// else yet; `stopping` arrives with the stage-2 `stop` command.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub match_id: String,
    #[serde(default)]
    pub addr: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub uptime_s: u64,
    #[serde(default)]
    pub tick: u64,
    #[serde(default)]
    pub tick_rate: u32,
    #[serde(default)]
    pub tick_budget_ms: f64,
    #[serde(default)]
    pub ships: usize,
    #[serde(default)]
    pub players: usize,
    #[serde(default)]
    pub connections: usize,
    #[serde(default)]
    pub entities: EntitiesStatus,
    #[serde(default)]
    pub perf: PerfStatus,
    #[serde(default)]
    pub net: NetStatus,
    /// Sorted by connection id; refreshed at most once a second.
    #[serde(default)]
    pub player_list: Vec<PlayerStatus>,
}

impl ServerStatus {
    /// The starting snapshot, before the loop has anything to say: the bound
    /// address and match identity are known at wiring time, everything else
    /// fills in on the first tick.
    pub fn starting(match_id: String, addr: String, port: u16) -> Self {
        Self {
            state: "starting".to_string(),
            match_id,
            addr,
            port,
            tick_rate: TICK_RATE,
            tick_budget_ms: TICK_DURATION_MS as f64,
            ..Self::default()
        }
    }
}

/// The shared handle: the loop writes through it, every control connection
/// reads a copy through `snapshot()` — a read can never block a tick for long,
/// and the unpleasant case (a writer cancelled mid-update) is invisible
/// because the guard is only held for pointer swizzles, not for serialization.
#[derive(Clone)]
pub struct StatusHandle {
    inner: Arc<RwLock<ServerStatus>>,
}

impl StatusHandle {
    pub fn new(starting: ServerStatus) -> Self {
        Self {
            inner: Arc::new(RwLock::new(starting)),
        }
    }

    /// A clone for the request path: commands read a stable copy even while
    /// the loop keeps advancing. `ServerStatus` is plain data — the clone is
    /// the read.
    pub fn snapshot(&self) -> ServerStatus {
        self.inner.read().clone()
    }

    /// The every-tick cheap numbers. Called at most once per loop iteration.
    ///
    /// Stage 2 reworded this: the tick never (re)writes the state word. It
    /// used to assert `running` every tick, which would overwrite
    /// `stopping` one iteration after a queued stop — the state now moves
    /// only where the loop means it (`set_state` at the loop's start, at
    /// the stop order, and nowhere else).
    pub fn update_tick(&self, tick: u64, uptime_s: u64, players: usize, ships: usize) {
        let mut status = self.inner.write();
        status.tick = tick;
        status.uptime_s = uptime_s;
        status.players = players;
        status.ships = ships;
    }

    /// The per-window perf aggregates. The values are the same ones the
    /// `perf:` line prints; this call never computes anything itself.
    pub fn publish_perf(&self, perf: PerfStatus, entities: EntitiesStatus) {
        let mut status = self.inner.write();
        status.perf = perf;
        status.entities = entities;
    }

    /// The once-a-second refresh: net totals, players list, connection count.
    /// `player_list` is sorted by connection id here so the listing never
    /// depends on the bookkeeping's internal order.
    pub fn refresh_world(
        &self,
        net: NetStatus,
        player_list: Vec<PlayerStatus>,
        connections: usize,
    ) {
        let mut status = self.inner.write();
        status.net = net;
        status.connections = connections;
        let mut list = player_list;
        list.sort_by_key(|p| p.connection_id);
        status.player_list = list;
    }

    /// The state word (`running` here; stage 2 adds the rest).
    pub fn set_state(&self, state: &str) {
        self.inner.write().state = state.to_string();
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn a_starting_snapshot_names_the_match_and_the_nominal_cadence() {
        let status = ServerStatus::starting("m".into(), "1.2.3.4:7777".into(), 7777);
        assert_eq!(status.state, "starting");
        assert_eq!(status.match_id, "m");
        assert_eq!(status.port, 7777);
        // The budget the port reports against is the loop's own contract.
        assert_eq!(status.tick_rate, 30);
        assert_eq!(status.tick_budget_ms, TICK_DURATION_MS as f64);
        assert_eq!(status.tick, 0);
    }

    #[test]
    fn tick_updates_move_the_cheap_fields_and_leave_the_state_word_alone() {
        let handle = StatusHandle::new(ServerStatus::starting("m".into(), String::new(), 7777));
        handle.set_state("running");
        handle.update_tick(18450, 615, 3, 4);
        let snap = handle.snapshot();
        assert_eq!(snap.state, "running");
        assert_eq!(snap.tick, 18450);
        assert_eq!(snap.uptime_s, 615);
        assert_eq!(snap.players, 3);
        assert_eq!(snap.ships, 4);
        // Stage 2: a stop order marks `stopping`, and the cheap per-tick
        // numbers keep flowing UNDER that word — a tick must never
        // overwrite it back to running.
        handle.set_state("stopping");
        handle.update_tick(18451, 616, 3, 4);
        assert_eq!(handle.snapshot().state, "stopping");
    }

    #[test]
    fn the_world_refresh_sorts_the_players_and_carries_the_net_totals() {
        let handle = StatusHandle::new(ServerStatus::starting("m".into(), String::new(), 1));
        let list = vec![
            PlayerStatus { connection_id: 9, name: "b".into(), ..Default::default() },
            PlayerStatus { connection_id: 2, name: "a".into(), ..Default::default() },
        ];
        let net = NetStatus { session_rejects: 5, ..Default::default() };
        handle.refresh_world(net, list, 2);
        let snap = handle.snapshot();
        let ids: Vec<u32> = snap.player_list.iter().map(|p| p.connection_id).collect();
        assert_eq!(ids, vec![2, 9], "the listing must not depend on map order");
        assert_eq!(snap.connections, 2);
        assert_eq!(snap.net.session_rejects, 5);
    }

    #[test]
    fn publish_perf_stores_the_window_and_the_entity_counts_untouched() {
        let handle = StatusHandle::new(ServerStatus::starting("m".into(), String::new(), 1));
        let perf = PerfStatus { loop_avg_ms: 9.7, window_rounds: 5, ..Default::default() };
        let entities = EntitiesStatus { ships: 4, projectiles: 7, ..Default::default() };
        handle.publish_perf(perf, entities);
        let snap = handle.snapshot();
        assert_eq!(snap.perf.loop_avg_ms, 9.7);
        assert_eq!(snap.perf.window_rounds, 5);
        assert_eq!(snap.entities.projectiles, 7);
    }

    /// A snapshot and a locked writer must not deadlock a reader: the clone
    /// path is what every connection pays, so it is exercised here directly.
    #[test]
    fn snapshots_are_independent_copies_of_the_shared_state() {
        let handle = StatusHandle::new(ServerStatus::starting("m".into(), String::new(), 1));
        handle.update_tick(1, 1, 1, 1);
        let first = handle.snapshot();
        handle.update_tick(2, 2, 2, 2);
        assert_eq!(first.tick, 1, "an earlier snapshot does not change behind the reader");
        assert_eq!(handle.snapshot().tick, 2);
    }
}
