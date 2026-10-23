use crate::bandwidth::{Admission, BandwidthLimiter, BandwidthStats, BandwidthTracker};
use crate::connection::{
    channel_from_u8, is_reliable_channel, smooth_rtt, Connection, ConnectionConfig,
    ConnectionError, ConnectionState, OutgoingPacket, ReliableArrival, SESSION_UNSET,
};
use crate::snapshot::{
    entity_layer, EntitySnapshot, ProjectileSnapshot, Snapshot, SnapshotBuffer,
    SnapshotInterpolator, LAYER_COUNT, LAYER_PROJECTILE, MAX_ENTITIES_PER_SNAPSHOT,
    MAX_PROJECTILES_PER_SNAPSHOT,
};
use parking_lot::{Mutex, RwLock};
use rfs_core::packet::*;
use rfs_core::time::{Tick, TICK_DURATION, TICK_RATE};
use std::collections::HashMap;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Connect retransmit cadence (bug №74).
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(250);
/// Give up waiting for ConnectAccept after this long.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Wire cap for a single bincode message (DoS: Vec len prefix could OOM).
const WIRE_LIMIT: u64 = 1024 * 1024;

/// Move the client's world clock forward, never backward. Bug №269.
///
/// `server_tick`/`server_time` are the client's answer to "where is the world
/// now", and they are shared by every layer — but the layers arrive at
/// different rates (`LAYER_INTERVAL_TICKS = [2, u64::MAX, 1, 1]`), so an older
/// tick from a slower layer routinely lands after a newer one from a faster
/// layer. Overwriting unconditionally rewound the clock, and the stress harness
/// counts an observed tick on every change of `server_tick()`, so each rewind
/// was counted as delivery that never happened.
///
/// Returns whether the clock moved. Note the guard is on the tick alone: the
/// time is a derived quantity, so it is only taken when the tick advances and
/// can never drift out of step with it.
pub fn advance_world_clock(
    current: &mut Tick,
    current_time: &mut f64,
    incoming: u64,
    incoming_time: f64,
) -> bool {
    if incoming > current.value() {
        *current = Tick(incoming);
        *current_time = incoming_time;
        true
    } else {
        false
    }
}

/// Bug №168: a legitimately reassembled full state can be as large as
/// MAX_REASSEMBLED_SIZE (4 MB). Rejecting it at 1 MB meant the client could
/// never apply the first full state, and the only way forward was the layer
/// resync loop. Full states use the reassembly bound; every other message
/// keeps the tighter 1 MB cap.
const FULL_STATE_LIMIT: u64 = crate::fragment::MAX_REASSEMBLED_SIZE as u64;

/// Bug №171: how long a reliable packet may wait for bandwidth tokens before
/// the send path gives up and reports an error. The point is not to wait
/// forever — an error the caller can see beats a silent, permanent loss — but
/// to cover normal bursts. A stalled client is disconnected by the timeout
/// sweep regardless.
const RELIABLE_ADMIT_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);

/// Bug №175: capacity of the client event queue. State updates bypass it
/// (see `latest_state`), so this only has to cover the low-rate lifecycle and
/// per-entity events; a full queue means the consumer is not keeping up, and
/// the oldest event is dropped rather than growing without bound.
const CLIENT_EVENT_QUEUE: usize = 4096;

/// Bug №613: capacity of the outbox. Same reasoning as the event queue — a full
/// queue means the writer is not keeping up, so the newest datagram is dropped
/// and counted rather than buffered until the process dies. Generous next to
/// `CLIENT_EVENT_QUEUE` because state updates legitimately queue in bursts.
const CLIENT_OUTBOX_QUEUE_LIMIT: usize = 8192;

fn deserialize_limited<T: serde::de::DeserializeOwned>(
    payload: &[u8],
) -> Result<T, bincode::Error> {
    use bincode::Options;
    bincode::DefaultOptions::new()
        .with_limit(WIRE_LIMIT)
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize(payload)
}

fn deserialize_full_state<T: serde::de::DeserializeOwned>(
    payload: &[u8],
) -> Result<T, bincode::Error> {
    use bincode::Options;
    bincode::DefaultOptions::new()
        .with_limit(FULL_STATE_LIMIT)
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize(payload)
}

/// Outcome of an event delivery attempt.
///
/// Replaces `mpsc::error::{SendError, TrySendError}<ClientEvent>` as the return
/// type. Those carry the 288-byte `ClientEvent` back to the caller, which makes
/// every `Result` in this file large enough to matter and gives the caller
/// nothing it can use. The two failure modes are genuinely different, though,
/// and callers are expected to react differently:
///
/// - [`Full`](Self::Full) — the consumer is behind. Transient; the newest
///   state supersedes the lost one.
/// - [`Closed`](Self::Closed) — the receiver is gone for good. Retrying is
///   pointless and the connection should be torn down.
///
/// `#[must_use]` on purpose. A discarded `Disposition` is exactly the mistake
/// that made this sink exist in the first place — the old call sites wrote
/// `let _ = send(..)` and threw away every failure, so a broken event path was
/// indistinguishable from a healthy one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "the caller has to decide what a refused event means"]
enum Disposition {
    Delivered,
    /// The bounded queue was full (non-blocking path only).
    Full,
    /// The receiver was dropped; no further delivery will ever succeed.
    Closed,
}

impl Disposition {
    /// Whether the event actually reached the consumer.
    fn delivered(self) -> bool {
        matches!(self, Self::Delivered)
    }
}

/// Bug №175: sender wrapper that records backpressure.
///
/// The event queue is bounded, so `send` can now legitimately fail. Swallowing
/// that with `let _ =` would hide a consumer that cannot keep up — exactly the
/// condition that used to OOM the process. Dropping is still the right
/// behaviour; it just has to be visible.
#[derive(Clone)]
struct EventSink {
    tx: mpsc::Sender<ClientEvent>,
    dropped: Arc<AtomicU64>,
    /// Bug №175 follow-up: overflow and "the consumer is gone" are different
    /// faults. A closed channel is terminal and must not be counted as routine
    /// backpressure, or a shutdown looks like a slow client in the metrics.
    closed: Arc<AtomicU64>,
}

impl EventSink {
    fn new(tx: mpsc::Sender<ClientEvent>, dropped: Arc<AtomicU64>) -> Self {
        Self {
            tx,
            dropped,
            closed: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Delivers an event, waiting for room in the bounded channel.
    ///
    /// Only for the critical, low-rate lifecycle events (connect, disconnect,
    /// errors, acks) that must not be lost: the caller is an `async` context
    /// and blocking here is the intended backpressure.
    ///
    /// `Full` is impossible on this path — `mpsc::Sender::send` awaits for
    /// capacity — so only `Delivered` and `Closed` can come back.
    async fn send(&self, event: ClientEvent) -> Disposition {
        match self.tx.send(event).await {
            Ok(()) => Disposition::Delivered,
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                self.closed.fetch_add(1, Ordering::Relaxed);
                Disposition::Closed
            }
        }
    }

    /// Non-blocking deliver, for the high-rate per-entity deltas that are
    /// emitted from synchronous `process_state_packet` and where losing a
    /// frame is harmless — the next state packet supersedes it, and
    /// `latest_state` carries the coalesced snapshot.
    fn try_send(&self, event: ClientEvent) -> Disposition {
        match self.tx.try_send(event) {
            Ok(()) => Disposition::Delivered,
            Err(mpsc::error::TrySendError::Full(_)) => {
                // Transient: the consumer is behind. The next state packet
                // supersedes this one, so dropping is correct.
                self.dropped.fetch_add(1, Ordering::Relaxed);
                Disposition::Full
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // Terminal: the receiver is gone. Retrying is pointless.
                self.dropped.fetch_add(1, Ordering::Relaxed);
                self.closed.fetch_add(1, Ordering::Relaxed);
                Disposition::Closed
            }
        }
    }

    /// Fire-and-forget for the per-entity deltas.
    ///
    /// Both refusal modes are deliberately swallowed here, and this is the one
    /// place that decision is made: the next state packet supersedes a dropped
    /// `Full`, and a `Closed` receiver means there is nothing left to talk to
    /// — the receive loop notices via `event_channel_closed()`. Both are
    /// counted, so the discard is visible in the metrics rather than silent.
    #[allow(clippy::let_underscore_must_use)]
    fn send_delta(&self, event: ClientEvent) {
        let _ = self.try_send(event);
    }

    /// Whether the event receiver has been dropped for good.
    ///
    /// Distinct from "the queue was full": a full queue means the consumer is
    /// behind and catching up, a closed one means there is no consumer at all.
    fn channel_is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed) > 0
    }

    /// Send a critical lifecycle event; report whether the consumer is alive.
    ///
    /// `Closed` is the one result a caller must act on: the receiver is gone,
    /// so there is no point receiving more packets off the socket. It is
    /// logged here so all eight critical sites do not each have to remember.
    ///
    /// Returns `false` only on `Closed`. A plain `bool` rather than a
    /// `Disposition`, because the decision has already been made and logged at
    /// this point — callers only need the yes/no for control flow.
    async fn send_critical(&self, event: ClientEvent) -> bool {
        let disposition = self.send(event).await;
        if !disposition.delivered() {
            warn!("event channel closed: the client is no longer consuming events");
        }
        disposition.delivered()
    }
}

pub struct NetClient {
    socket: Arc<UdpSocket>,
    config: ClientConfig,
    #[allow(dead_code)]
    server_addr: SocketAddr,
    connection: Arc<Mutex<Option<Connection>>>,
    connection_id: Arc<Mutex<Option<u32>>>,
    /// Set while a Connect is unanswered; cleared on Accept/Reject/timeout.
    /// The retry task watches it (bug №74).
    connect_attempt: Arc<Mutex<Option<Instant>>>,
    snapshot_buffer: Arc<SnapshotBuffer>,
    snapshot_interpolator: Arc<Mutex<SnapshotInterpolator>>,
    bandwidth_tracker: Arc<BandwidthTracker>,
    /// Bug №169: this limiter used to be `#[allow(dead_code)]` — constructed
    /// and never consulted, so the client's own send rate was never actually
    /// capped. Every send path now goes through `admit`.
    bandwidth_limiter: Arc<BandwidthLimiter>,
    current_tick: Arc<Mutex<Tick>>,
    server_tick: Arc<Mutex<Tick>>,
    server_time: Arc<Mutex<f64>>,
    rtt: Arc<Mutex<Duration>>,
    running: Arc<Mutex<bool>>,
    send_handle: Mutex<Option<JoinHandle<()>>>,
    recv_handle: Mutex<Option<JoinHandle<()>>>,
    event_sender: EventSink,
    event_receiver: Mutex<Option<mpsc::Receiver<ClientEvent>>>,
    /// Bug №175: the newest full snapshot, kept in a slot rather than the
    /// queue. An unbounded channel plus a 16 ms consumer drain let state
    /// updates pile up without limit and OOM the process. Only the latest
    /// snapshot matters, so a new one overwrites the pending one instead of
    /// appending — the queue stays bounded and nothing is starved.
    latest_state: Arc<Mutex<Option<ClientEvent>>>,
    /// Bug №175: how many events the bounded channel refused. Surfaced so
    /// backpressure is measurable instead of invisible.
    events_dropped: Arc<AtomicU64>,
    /// How many events were refused because the receiver was gone. Kept apart
    /// from `events_dropped` because that one is transient backpressure and
    /// this one is terminal.
    events_closed: Arc<AtomicU64>,
    /// Bug №172: single ordered outbox for the synchronous send entry points
    /// (`send_input`, `send_command`). They cannot await, so they used to spawn
    /// a task per packet — unbounded scheduler growth and no ordering
    /// guarantee between concurrent senders.
    outbox: mpsc::Sender<OutboxItem>,
    outbox_handle: Mutex<Option<JoinHandle<()>>>,
    outbox_rx: Mutex<Option<mpsc::Receiver<OutboxItem>>>,
    /// Bug №613: outbox entries dropped because the writer fell behind.
    dropped_outbox: Arc<AtomicU64>,
    input_sequence: Arc<Mutex<u32>>,
    pending_inputs: Arc<Mutex<HashMap<u32, InputPacket>>>,
    last_acknowledged_tick: Arc<Mutex<u32>>,
    entity_states: Arc<RwLock<HashMap<EntityId, EntitySnapshot>>>,
    projectile_states: Arc<RwLock<HashMap<EntityId, ProjectileSnapshot>>>,
    /// Newest applied server tick per replication layer (plan №3.4). Layered
    /// deltas apply to the latest snapshot; stale ones are dropped.
    last_applied_layer: Arc<Mutex<[u64; LAYER_COUNT]>>,
    /// Bug №274: deltas actually applied, counted where they are applied. The
    /// `latest_state` slot cannot be used for this — it coalesces.
    applied_delta_count: Arc<AtomicU64>,
    /// Bug №274: the same, per layer, so one slow layer is distinguishable
    /// from a slow client.
    applied_delta_count_by_layer: Arc<std::sync::Mutex<Vec<u64>>>,
    /// Bug #267: how many deltas were dropped because their base tick did not
    /// match what this client had applied. Each one stalls its layer until the
    /// next scheduled resync, so this is the closest thing to a health figure
    /// the replication layer has.
    base_mismatch_drops: Arc<AtomicU64>,
    /// Same drops as `base_mismatch_drops`, split per layer. A single total
    /// cannot distinguish "one busy layer" from "every layer briefly stalling",
    /// and those need different fixes.
    base_mismatch_drops_by_layer: Arc<std::sync::Mutex<Vec<u64>>>,
    /// Latch so a stalled layer is logged once, not once per rejected delta.
    stall_logged: Arc<std::sync::Mutex<Vec<bool>>>,
    /// Deltas dropped by the ordinary out-of-order/stale guard (server_tick of
    /// the delta already applied). Distinct from `base_mismatch_drops`; both
    /// were added while chasing #267 and base-mismatch measured zero, making
    /// this the path that still needs accounting.
    stale_delta_drops: Arc<AtomicU64>,
    /// Bug №636: the session this client is attached to.
    ///
    /// A reconnect to a restarted server is a *new* session whose ticks start
    /// over, and the client cannot tell that from the packets alone — a full
    /// state from a fresh server looks exactly like a very late one. Holding the
    /// previous `match_id` is what makes the difference observable.
    match_id: Arc<Mutex<Option<Uuid>>>,
    /// How many times the applied world was discarded because the session
    /// changed. Counted rather than left to a log line: a client that resets on
    /// every reconnect never accumulates a world, and that is a condition, not
    /// a one-off.
    session_resets: Arc<AtomicU64>,
    /// Bug №636: full states dropped by the stale-guard (`server_tick` at or
    /// below the newest layer clock already applied). It used to return without
    /// a word, which is why a client in a ghost world looked healthy from the
    /// outside: the one number that would have shown the stall was never
    /// taken.
    stale_full_state_drops: Arc<AtomicU64>,
    /// Bug №267 (A): armed resync requests waiting for the receive loop.
    resync_requests: ResyncRequestChannel,
    /// Bug №698: requests actually sent, so the harness can compare the
    /// asks with the server's resync behaviour.
    resync_requests_sent: Arc<AtomicU64>,
    /// Bug №698: the world's static description, as the server told it.
    ///
    /// The layers carry ships, players, stations, compartments and
    /// projectiles — and no structure, so without this the client could not
    /// draw the world and could not explain a shell vanishing at a rock it
    /// had never heard of.
    map_description: Arc<RwLock<Option<rfs_core::world::MapDescription>>>,
    /// Bug №700: the shared secret with the server; `None` is the unsigned
    /// dialect. Taken from `ClientConfig::auth_token` once, at construction.
    auth_key: Option<Vec<u8>>,
    /// Bug №700: outbound auth sequence for C2S datagrams. Starts at 1 — a
    /// zero sequence is the unsigned dialect — and skips 0 on wrap.
    wire_seq: Arc<Mutex<u64>>,
    /// Bug №700: replay window against the server, checked before any packet
    /// parsing. A datagram with a sequence this window already saw is dropped.
    server_replay: Arc<Mutex<crate::auth::ReplayWindow>>,
}

/// One queued client datagram awaiting the writer task.
struct OutboxItem {
    header: PacketHeader,
    payload: Vec<u8>,
}

/// Bug №267 (A): the client-side channel for "this layer is stalled, ask the
/// server for an immediate resync". The apply site arms a per-layer bit (a
/// cooldown keeps a long stall from asking once per rejected delta); the
/// receive loop drains the bitmask and puts the tiny packets on the wire.
#[derive(Clone)]
struct ResyncRequestChannel {
    /// Bit `layer` set means: a resync request for that layer is due out.
    bitmask: Arc<AtomicU32>,
    /// Per-arm cooldown state, one slot per layer.
    last_arm: Arc<std::sync::Mutex<Vec<Instant>>>,
}

/// A stalled layer waits half a second before it asks again — the server's
/// resync pass runs every 33 ms, so a lost request resolves on the next arm
/// long before the scheduled cadence would have healed the layer.
const RESYNC_REQUEST_COOLDOWN: Duration = Duration::from_millis(500);

impl ResyncRequestChannel {
    fn new() -> Self {
        // The minus-cooldown start makes the first arm fire immediately: a
        // fresh client with no slots should not wait half a second after the
        // very first mismatch to ask for the recovery it already needs.
        let epoch = Instant::now()
            .checked_sub(RESYNC_REQUEST_COOLDOWN)
            .unwrap_or_else(Instant::now);
        Self {
            bitmask: Arc::new(AtomicU32::new(0)),
            last_arm: Arc::new(std::sync::Mutex::new(vec![epoch; LAYER_COUNT])),
        }
    }

    /// Arms the layer's bit if the cooldown allows; reports whether it fired.
    fn arm(&self, layer: usize) -> bool {
        if layer >= LAYER_COUNT {
            return false;
        }
        let mut last = self.last_arm.lock().unwrap();
        let now = Instant::now();
        let due = last
            .get(layer)
            .map(|t| now.duration_since(*t) >= RESYNC_REQUEST_COOLDOWN)
            .unwrap_or(true);
        if due {
            if let Some(slot) = last.get_mut(layer) {
                *slot = now;
            }
            self.bitmask.fetch_or(1u32 << layer, Ordering::Relaxed);
        }
        due
    }

    /// Drains the bitmask and lists the layers owed a request.
    fn take_due(&self) -> Vec<usize> {
        let bits = self.bitmask.swap(0, Ordering::Relaxed);
        (0..LAYER_COUNT)
            .filter(|layer| bits & (1u32 << layer) != 0)
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub bind_addr: SocketAddr,
    pub server_addr: SocketAddr,
    pub connection_config: ConnectionConfig,
    pub snapshot_history: usize,
    pub tick_rate: u32,
    pub max_bandwidth_bps: f64,
    pub enable_bandwidth_limit: bool,
    pub player_name: String,
    pub build_version: String,
    /// Bug №700: the shared secret with the server. `None` or blank is the
    /// unsigned dialect — the server answers in kind, so a client with a
    /// token must not talk to a server without one and vice versa.
    pub auth_token: Option<String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:0".parse().unwrap(),
            server_addr: "127.0.0.1:7777".parse().unwrap(),
            connection_config: ConnectionConfig::default(),
            snapshot_history: 128,
            tick_rate: TICK_RATE,
            max_bandwidth_bps: 512.0 * 1024.0,
            enable_bandwidth_limit: false,
            player_name: "Player".to_string(),
            build_version: "0.1.0".to_string(),
            auth_token: None,
        }
    }
}

#[derive(Debug, Clone)]
// The variants differ in size because some carry a snapshot inline. Boxing the
// large one would shrink the enum at the cost of an allocation on every state
// update, and would change a public enum that the UI and the harness both match
// on — a cost paid outside this crate to save bytes that are never allocated in
// bulk here, because these events are queued once and moved, not stored in
// dense arrays. The lint is real and deliberately not acted on.
#[allow(clippy::large_enum_variant)]
pub enum ClientEvent {
    Connected {
        connection_id: u32,
        server_tick: u64,
        match_id: Uuid,
    },
    Disconnected {
        reason: DisconnectReason,
    },
    ConnectionFailed {
        reason: ConnectRejectReason,
    },
    StateUpdate {
        snapshot: Snapshot,
    },
    EntityUpdate {
        entity_id: EntityId,
        snapshot: EntitySnapshot,
    },
    EntityRemoved {
        entity_id: EntityId,
    },
    ProjectileUpdate {
        projectile_id: EntityId,
        snapshot: ProjectileSnapshot,
    },
    ProjectileRemoved {
        projectile_id: EntityId,
    },
    GameEvent {
        event: GameEvent,
    },
    CommandAck {
        command_id: u64,
        success: bool,
        error: Option<String>,
    },
    InputAck {
        tick: u32,
        accepted: bool,
    },
    BandwidthStats {
        stats: BandwidthStats,
    },
    RttUpdate {
        rtt: Duration,
    },
    Error {
        error: String,
    },
}

impl NetClient {
    pub async fn new(config: ClientConfig) -> Result<Self, std::io::Error> {
        let socket = Arc::new(UdpSocket::bind(config.bind_addr).await?);
        socket.connect(config.server_addr).await?;
        let (event_tx, event_receiver) = mpsc::channel(CLIENT_EVENT_QUEUE);
        let events_dropped = Arc::new(AtomicU64::new(0));
        let event_sender = EventSink::new(event_tx, events_dropped.clone());
        // The sink owns its own `closed` counter; hand the client a handle to
        // the same one so `event_channel_closed()` and the sink cannot drift.
        let events_closed = event_sender.closed.clone();
        // Bug №172: outbox for the synchronous send entry points.
        // Bug №613: bounded, for the same reason as the server's — an unbounded
        // queue is an OOM waiting for a producer that outruns the writer.
        let (outbox, outbox_rx) = mpsc::channel(CLIENT_OUTBOX_QUEUE_LIMIT);

        let client = Self {
            socket,
            config: config.clone(),
            server_addr: config.server_addr,
            connection: Arc::new(Mutex::new(None)),
            connection_id: Arc::new(Mutex::new(None)),
            connect_attempt: Arc::new(Mutex::new(None)),
            snapshot_buffer: Arc::new(SnapshotBuffer::new(config.snapshot_history)),
            snapshot_interpolator: Arc::new(Mutex::new(SnapshotInterpolator::new(
                config.snapshot_history,
            ))),
            bandwidth_tracker: Arc::new(BandwidthTracker::new()),
            bandwidth_limiter: Arc::new(BandwidthLimiter::new(config.max_bandwidth_bps)),
            current_tick: Arc::new(Mutex::new(Tick(0))),
            server_tick: Arc::new(Mutex::new(Tick(0))),
            server_time: Arc::new(Mutex::new(0.0)),
            rtt: Arc::new(Mutex::new(Duration::from_millis(100))),
            running: Arc::new(Mutex::new(false)),
            send_handle: Mutex::new(None),
            recv_handle: Mutex::new(None),
            event_sender,
            event_receiver: Mutex::new(Some(event_receiver)),
            latest_state: Arc::new(Mutex::new(None)),
            events_dropped,
            events_closed,
            outbox,
            outbox_handle: Mutex::new(None),
            outbox_rx: Mutex::new(Some(outbox_rx)),
            dropped_outbox: Arc::new(AtomicU64::new(0)),
            input_sequence: Arc::new(Mutex::new(0)),
            pending_inputs: Arc::new(Mutex::new(HashMap::new())),
            last_acknowledged_tick: Arc::new(Mutex::new(0)),
            entity_states: Arc::new(RwLock::new(HashMap::new())),
            projectile_states: Arc::new(RwLock::new(HashMap::new())),
            last_applied_layer: Arc::new(Mutex::new([0; LAYER_COUNT])),
            applied_delta_count: Arc::new(AtomicU64::new(0)),
            applied_delta_count_by_layer: Arc::new(std::sync::Mutex::new(vec![0u64; LAYER_COUNT])),
            base_mismatch_drops: Arc::new(AtomicU64::new(0)),
            base_mismatch_drops_by_layer: Arc::new(std::sync::Mutex::new(vec![0u64; LAYER_COUNT])),
            stall_logged: Arc::new(std::sync::Mutex::new(vec![false; LAYER_COUNT])),
            stale_delta_drops: Arc::new(AtomicU64::new(0)),
            match_id: Arc::new(Mutex::new(None)),
            session_resets: Arc::new(AtomicU64::new(0)),
            stale_full_state_drops: Arc::new(AtomicU64::new(0)),
            resync_requests: ResyncRequestChannel::new(),
            resync_requests_sent: Arc::new(AtomicU64::new(0)),
            map_description: Arc::new(RwLock::new(None)),
            // Bug №700: one key for the client's whole life; blank means the
            // unsigned dialect, same rule the server applies to its token.
            auth_key: config
                .auth_token
                .as_ref()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .map(String::into_bytes),
            wire_seq: Arc::new(Mutex::new(0)),
            server_replay: Arc::new(Mutex::new(crate::auth::ReplayWindow::default())),
        };

        Ok(client)
    }

    pub fn connect(&self) {
        *self.running.lock() = true;
        *self.connect_attempt.lock() = Some(Instant::now());
        self.start_outbox_writer();
        self.start_receive_loop();
        self.start_send_loop();
        self.send_connect_request();
        info!("NetClient connecting to {}", self.config.server_addr);
    }

    /// Tear the connection down locally.
    ///
    /// Deliberately silent: the caller is the one who asked for it, so it
    /// already knows. Only a *remote* disconnect (a `Disconnect` packet
    /// arriving in the receive loop) emits `ClientEvent::Disconnected`.
    ///
    /// Emitting here as well was tried and reverted — a consumer that treats
    /// `Disconnected` as "the connection is gone" and calls `disconnect()` in
    /// response would receive the event back and could loop. See №245.
    pub fn disconnect(&self, reason: DisconnectReason) {
        *self.running.lock() = false;
        *self.connect_attempt.lock() = None;

        // Bug №68: this datagram used to be spawned and then aborted a
        // microsecond later, so it almost never left and the server only freed
        // the slot via the 10 s timeout. `disconnect` is synchronous, so send
        // it inline with `try_send` — no task to abort, no await needed. UDP
        // only fails this way if the socket buffer is full, and then the
        // outbox fallback still gets it out.
        if self.connection.lock().take().is_some() {
            let packet = DisconnectPacket { reason };
            match bincode::serialize(&packet) {
                Ok(payload) => {
                    let Ok(len) = u16::try_from(payload.len()) else {
                        warn!("disconnect: payload too large to frame");
                        return;
                    };
                    let mut header = PacketHeader::new(
                        PacketType::Disconnect,
                        ChannelType::ReliableOrdered,
                        0,
                        0,
                        0,
                        0,
                    );
                    header.payload_size = len;
                    match bincode::serialize(&header) {
                        Ok(head) => {
                            // Bug №700: the farewell carries the same auth
                            // block as every datagram, so the server unwraps
                            // it the same way it unwraps the rest.
                            let total = crate::auth::AUTH_WIRE_LEN + HEADER_SIZE + payload.len();
                            if total > MAX_PACKET_SIZE {
                                warn!("disconnect: {total} exceeds the MTU, not sending");
                            } else {
                                let mut packet_bytes = head.to_vec();
                                packet_bytes.extend_from_slice(&payload);
                                let seq = {
                                    let mut seq = self.wire_seq.lock();
                                    *seq = seq.wrapping_add(1);
                                    if *seq == 0 {
                                        *seq = 1;
                                    }
                                    *seq
                                };
                                let mut buffer = Vec::with_capacity(total);
                                crate::auth::seal(
                                    self.auth_key.as_deref(),
                                    crate::auth::Direction::ClientToServer,
                                    seq,
                                    &packet_bytes,
                                    &mut buffer,
                                );
                                match self.socket.try_send(&buffer) {
                                    Ok(n) => {
                                        self.bandwidth_tracker.record_sent(n);
                                    }
                                    Err(e) => {
                                        // Buffer full or socket closing: fall
                                        // back to the ordered outbox.
                                        warn!("disconnect: try_send failed ({e}), queueing");
                                        self.enqueue(header, payload);
                                    }
                                }
                            }
                        }
                        Err(e) => warn!("disconnect: header serialize failed: {e}"),
                    }
                }
                Err(e) => warn!("disconnect: serialize failed: {e}"),
            }
        }

        if let Some(handle) = self.send_handle.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.recv_handle.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.outbox_handle.lock().take() {
            handle.abort();
        }

        info!("NetClient disconnected");
    }

    /// Bug №172: the single writer for queued client datagrams.
    ///
    /// `send_input` and `send_command` are synchronous, so they cannot await a
    /// socket write. They used to spawn a task each; now they serialise into
    /// this FIFO and one task does the writing, in order.
    fn start_outbox_writer(&self) {
        let Some(mut rx) = self.outbox_rx.lock().take() else {
            return;
        };
        let socket = self.socket.clone();
        let bandwidth = self.bandwidth_tracker.clone();
        let limiter = self.bandwidth_limiter.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        // Bug №700: the seal travels with the writer, together with the
        // sequence counter it drives.
        let auth_key = self.auth_key.clone();
        let wire_seq = self.wire_seq.clone();

        let handle = tokio::spawn(async move {
            while let Some(OutboxItem { header, payload }) = rx.recv().await {
                let out = OutgoingPacket { header, payload };
                if let Err(e) = Self::send_raw(
                    &socket,
                    &bandwidth,
                    &limiter,
                    limit_enabled,
                    &out,
                    &auth_key,
                    &wire_seq,
                )
                .await
                {
                    warn!("client outbox send failed: {e}");
                }
            }
        });
        *self.outbox_handle.lock() = Some(handle);
    }

    /// Bug №172: serialise instead of spawning. `payload` must already be the
    /// bincode body; the writer frames it with the header.
    fn enqueue(&self, mut header: PacketHeader, payload: Vec<u8>) {
        let Ok(len) = u16::try_from(payload.len()) else {
            warn!(
                "outbox: payload {} exceeds the u16 header size",
                payload.len()
            );
            return;
        };
        header.payload_size = len;
        // Bug №613: the queue is bounded, so a send can now fail because it is
        // FULL — the writer fell behind. That is a dropped packet and it is
        // counted; bounding a queue must not become a new silent loss.
        match self.outbox.try_send(OutboxItem { header, payload }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.dropped_outbox.fetch_add(1, Ordering::Relaxed);
                warn!("client outbox full; dropping a queued packet");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                warn!("client outbox closed; dropping a queued packet");
            }
        }
    }

    /// Bug №613: outbox entries dropped because the writer fell behind.
    pub fn dropped_outbox(&self) -> u64 {
        self.dropped_outbox.load(Ordering::Relaxed)
    }

    fn send_connect_request(&self) {
        // One client_id per connect() call; the packet is re-sent until the
        // server answers (bug №74: a single lost Connect hung bots forever).
        let packet = ConnectPacket {
            client_id: Uuid::new_v4(),
            protocol_version: PROTOCOL_VERSION,
            player_name: self.config.player_name.clone(),
            build_version: self.config.build_version.clone(),
        };

        let header = PacketHeader::new(
            PacketType::Connect,
            ChannelType::ReliableOrdered,
            0,
            0,
            0,
            0,
        );

        let socket = self.socket.clone();
        let bandwidth = self.bandwidth_tracker.clone();
        let limiter = self.bandwidth_limiter.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        let attempt = self.connect_attempt.clone();
        let running = self.running.clone();
        let event_sender = self.event_sender.clone();
        // Bug №700: the Connect itself travels signed.
        let auth_key = self.auth_key.clone();
        let wire_seq = self.wire_seq.clone();
        tokio::spawn(async move {
            let mut first = true;
            loop {
                if attempt.lock().is_none() {
                    break; // Accept, Reject, timeout or disconnect.
                }
                if !first {
                    tokio::time::sleep(CONNECT_RETRY_INTERVAL).await;
                    if attempt.lock().is_none() || !*running.lock() {
                        break;
                    }
                }
                first = false;
                if attempt.lock().map(|t| t.elapsed()).unwrap_or_default() > CONNECT_TIMEOUT {
                    *attempt.lock() = None;
                    event_sender
                        .send_critical(ClientEvent::Error {
                            error: "connect timeout: no ConnectAccept".into(),
                        })
                        .await;
                    break;
                }
                let _ = Self::send_packet_static(
                    &socket,
                    &bandwidth,
                    &limiter,
                    limit_enabled,
                    &header,
                    &packet,
                    &auth_key,
                    &wire_seq,
                )
                .await;
            }
        });
    }

    /// Bug №169: `limiter` is the client's own `BandwidthLimiter`, finally
    /// consulted. `limit_enabled` mirrors the server's flag.
    ///
    /// Bug №171: an unreliable packet may be dropped when the bucket is empty,
    /// but a reliable one must not be — there is no retransmit queue yet, so
    /// the drop would be permanent. It waits for tokens instead, up to
    /// `RELIABLE_ADMIT_BUDGET`; exceeding that is reported as an error rather
    /// than silently losing the message.
    async fn send_packet_static(
        socket: &Arc<UdpSocket>,
        bandwidth: &BandwidthTracker,
        limiter: &BandwidthLimiter,
        limit_enabled: bool,
        header: &PacketHeader,
        packet: &impl Serializable,
        // Bug №700: the key this datagram is keyed with (None = unsigned
        // dialect) and the counter whose next number this send consumes.
        auth_key: &Option<Vec<u8>>,
        wire_seq: &Arc<Mutex<u64>>,
    ) -> anyhow::Result<()> {
        // Bug №81: a serialization failure must surface — not silently turn
        // into an empty payload the peer drops as invalid.
        let payload = bincode::serialize(packet)?;
        // The header length field is a u16: refuse to truncate instead of
        // lying about the size on the wire.
        let payload_len = u16::try_from(payload.len())
            .map_err(|_| anyhow::anyhow!("payload {} bytes exceeds u16 header", payload.len()))?;
        // Bug №700: the auth block rides on every datagram, so the MTU guard
        // and the limiter both count it.
        let total_size = crate::auth::AUTH_WIRE_LEN + HEADER_SIZE + payload.len();
        // Bug №30/#81: the peer deserializes strictly and discards oversized
        // datagrams — honour the MTU here so nothing bogus goes out.
        if total_size > MAX_PACKET_SIZE {
            return Err(anyhow::anyhow!(
                "packet too large: {total_size} > {MAX_PACKET_SIZE}"
            ));
        }
        let mut header = *header;
        header.payload_size = payload_len;

        let reliable = is_reliable_channel(header.channel);
        let mut budget = RELIABLE_ADMIT_BUDGET;
        loop {
            let decision = if limit_enabled {
                limiter.admit(total_size, reliable)
            } else {
                Admission::Send
            };
            match decision {
                Admission::Send => break,
                Admission::Drop => return Ok(()),
                Admission::Retry(wait) => {
                    if wait >= budget {
                        return Err(anyhow::anyhow!(
                            "limiter: reliable {} packet ({total_size} B) could not be admitted \
                             within {RELIABLE_ADMIT_BUDGET:?}",
                            header.channel
                        ));
                    }
                    tokio::time::sleep(wait).await;
                    budget -= wait;
                }
            }
        }

        let mut packet_bytes = bincode::serialize(&header)?;
        packet_bytes.extend_from_slice(&payload);
        let seq = {
            let mut seq = wire_seq.lock();
            *seq = seq.wrapping_add(1);
            if *seq == 0 {
                *seq = 1;
            }
            *seq
        };
        let mut buffer = Vec::with_capacity(total_size);
        crate::auth::seal(
            auth_key.as_deref(),
            crate::auth::Direction::ClientToServer,
            seq,
            &packet_bytes,
            &mut buffer,
        );

        socket.send(&buffer).await?;
        bandwidth.record_sent(total_size);
        Ok(())
    }

    /// Send a pre-framed OutgoingPacket (used for packets built by the
    /// Connection itself, e.g. the periodic heartbeat).
    async fn send_raw(
        socket: &Arc<UdpSocket>,
        bandwidth: &BandwidthTracker,
        limiter: &BandwidthLimiter,
        limit_enabled: bool,
        out: &OutgoingPacket,
        // Bug №700: the seal key and the sequence counter, same as
        // `send_packet_static`.
        auth_key: &Option<Vec<u8>>,
        wire_seq: &Arc<Mutex<u64>>,
    ) -> anyhow::Result<()> {
        // Bug №700: the block rides on every datagram and both guards count it.
        let total_size = crate::auth::AUTH_WIRE_LEN + out.total_size();
        if total_size > MAX_PACKET_SIZE {
            return Err(anyhow::anyhow!(
                "packet too large: {total_size} > {MAX_PACKET_SIZE}"
            ));
        }

        // Same admission rule as `send_packet_static` (bugs №169, №171).
        let reliable = is_reliable_channel(out.header.channel);
        let mut budget = RELIABLE_ADMIT_BUDGET;
        loop {
            let decision = if limit_enabled {
                limiter.admit(total_size, reliable)
            } else {
                Admission::Send
            };
            match decision {
                Admission::Send => break,
                Admission::Drop => return Ok(()),
                Admission::Retry(wait) => {
                    if wait >= budget {
                        return Err(anyhow::anyhow!(
                            "limiter: reliable {} packet ({total_size} B) could not be admitted \
                             within {RELIABLE_ADMIT_BUDGET:?}",
                            out.header.channel
                        ));
                    }
                    tokio::time::sleep(wait).await;
                    budget -= wait;
                }
            }
        }

        let mut packet_bytes = bincode::serialize(&out.header)?;
        packet_bytes.extend_from_slice(&out.payload);
        let seq = {
            let mut seq = wire_seq.lock();
            *seq = seq.wrapping_add(1);
            if *seq == 0 {
                *seq = 1;
            }
            *seq
        };
        let mut buffer = Vec::with_capacity(total_size);
        crate::auth::seal(
            auth_key.as_deref(),
            crate::auth::Direction::ClientToServer,
            seq,
            &packet_bytes,
            &mut buffer,
        );

        socket.send(&buffer).await?;
        bandwidth.record_sent(total_size);
        Ok(())
    }

    fn start_receive_loop(&self) {
        let socket = self.socket.clone();
        let connection = self.connection.clone();
        let connection_id = self.connection_id.clone();
        let config = self.config.clone();
        let snapshot_buffer = self.snapshot_buffer.clone();
        let snapshot_interpolator = self.snapshot_interpolator.clone();
        let bandwidth_tracker = self.bandwidth_tracker.clone();
        // Bug №169: the receive loop answers with real traffic, so it needs the
        // limiter as well.
        let limiter = self.bandwidth_limiter.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        let running = self.running.clone();
        let event_sender = self.event_sender.clone();
        // Bug №175: the receive loop publishes full snapshots into the
        // coalescing slot, not the (bounded) queue.
        let latest_state = self.latest_state.clone();
        let current_tick = self.current_tick.clone();
        let server_tick = self.server_tick.clone();
        let server_time = self.server_time.clone();
        let rtt = self.rtt.clone();
        let pending_inputs = self.pending_inputs.clone();
        let last_ack_tick = self.last_acknowledged_tick.clone();
        let entity_states = self.entity_states.clone();
        let projectile_states = self.projectile_states.clone();
        let last_applied_layer = self.last_applied_layer.clone();
        // Bug №274: shared with the apply site so it can count deltas where they
        // are applied, rather than inferring it from the coalescing slot.
        let applied_delta_count = self.applied_delta_count.clone();
        let applied_delta_count_by_layer = self.applied_delta_count_by_layer.clone();
        let base_mismatch_drops = self.base_mismatch_drops.clone();
        let base_mismatch_drops_by_layer = self.base_mismatch_drops_by_layer.clone();
        let stall_logged = self.stall_logged.clone();
        let stale_delta_drops = self.stale_delta_drops.clone();
        // Bug №636: the receive loop is where a session change is first
        // observable, so it owns both the record and the counter.
        let match_id = self.match_id.clone();
        let session_resets = self.session_resets.clone();
        let stale_full_state_drops = self.stale_full_state_drops.clone();
        // Bug №267 (A): the apply site arms per-layer requests, the receive
        // loop takes them out to the wire.
        let resync_requests = self.resync_requests.clone();
        let resync_requests_sent = self.resync_requests_sent.clone();
        // Bug №698: the world description, stored as it arrives.
        let map_description = self.map_description.clone();
        let connect_attempt = self.connect_attempt.clone();
        // Bug №700: the key and the replay window go with the loop; every
        // datagram opens its block before any packet parsing.
        let auth_key = self.auth_key.clone();
        let server_replay = self.server_replay.clone();
        // The HeartbeatAck answer and the resync request go back out through
        // the same helpers, so the counter travels here too.
        let wire_seq = self.wire_seq.clone();

        let handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];

            while *running.lock() {
                match socket.recv(&mut buf).await {
                    Ok(len) => {
                        let datagram = &buf[..len];
                        bandwidth_tracker.record_received(len);

                        // Bug №700: the framing of the wire is one block in
                        // front of the packet, in every dialect. The server's
                        // sequence freshness is refused here, the same way
                        // the server refuses ours.
                        let (seq, data) = match crate::auth::unseal(
                            auth_key.as_deref(),
                            crate::auth::Direction::ServerToClient,
                            datagram,
                        ) {
                            Some((seq, packet)) => (seq, packet),
                            None => {
                                debug!(
                                    "unauthenticated server datagram ({} bytes)",
                                    datagram.len()
                                );
                                continue;
                            }
                        };
                        if auth_key.is_some() {
                            let mut window = server_replay.lock();
                            if !window.accept(seq) {
                                debug!("stale server datagram (sequence {seq} already seen)");
                                continue;
                            }
                        }

                        if let Err(e) = Self::handle_received_packet(
                            &socket,
                            &connection,
                            &connection_id,
                            &config,
                            &snapshot_buffer,
                            &snapshot_interpolator,
                            &event_sender,
                            &latest_state,
                            &current_tick,
                            &server_tick,
                            &server_time,
                            &rtt,
                            &pending_inputs,
                            &last_ack_tick,
                            &entity_states,
                            &projectile_states,
                            &last_applied_layer,
                            // Bug №274: passed through so the apply site can
                            // count deltas without relying on the coalescing
                            // `latest_state` slot.
                            &applied_delta_count,
                            &applied_delta_count_by_layer,
                            &base_mismatch_drops,
                            &base_mismatch_drops_by_layer,
                            &stall_logged,
                            &stale_delta_drops,
                            &match_id,
                            &session_resets,
                            &stale_full_state_drops,
                            // Bug №267 (A): the arm/drain funnel.
                            &resync_requests,
                            &resync_requests_sent,
                            // Bug №698: the world description lands here.
                            &map_description,
                            &running,
                            &bandwidth_tracker,
                            &limiter,
                            limit_enabled,
                            &connect_attempt,
                            // Bug №700: the auth state for the answers.
                            &auth_key,
                            &wire_seq,
                            data,
                        )
                        .await
                        {
                            warn!("Error handling packet: {}", e);
                            event_sender
                                .send_critical(ClientEvent::Error {
                                    error: e.to_string(),
                                })
                                .await;
                        }

                        // The event receiver is gone, so nothing this loop
                        // produces can reach anyone. Stop reading the socket
                        // instead of decoding and discarding forever. This is
                        // the `Closed` half of `Disposition` being load-bearing:
                        // `Full` is a lagging consumer and must *not* stop us.
                        if event_sender.channel_is_closed() {
                            *running.lock() = false;
                            break;
                        }
                    }
                    Err(e) => {
                        if *running.lock() {
                            error!("Receive error: {}", e);
                        }
                    }
                }
            }
        });

        *self.recv_handle.lock() = Some(handle);
    }

    // The per-connection shared state is passed explicitly rather than
    // bundled into a context struct, to keep the borrow scopes of the
    // individual locks narrow and obvious at each use site.
    #[allow(clippy::too_many_arguments)]
    async fn handle_received_packet(
        socket: &Arc<UdpSocket>,
        connection: &Arc<Mutex<Option<Connection>>>,
        connection_id: &Arc<Mutex<Option<u32>>>,
        config: &ClientConfig,
        snapshot_buffer: &Arc<SnapshotBuffer>,
        snapshot_interpolator: &Arc<Mutex<SnapshotInterpolator>>,
        event_sender: &EventSink,
        latest_state: &Arc<Mutex<Option<ClientEvent>>>,
        current_tick: &Arc<Mutex<Tick>>,
        server_tick: &Arc<Mutex<Tick>>,
        server_time: &Arc<Mutex<f64>>,
        rtt: &Arc<Mutex<Duration>>,
        pending_inputs: &Arc<Mutex<HashMap<u32, InputPacket>>>,
        last_ack_tick: &Arc<Mutex<u32>>,
        entity_states: &Arc<RwLock<HashMap<EntityId, EntitySnapshot>>>,
        projectile_states: &Arc<RwLock<HashMap<EntityId, ProjectileSnapshot>>>,
        last_applied_layer: &Arc<Mutex<[u64; LAYER_COUNT]>>,
        // Bug №274: the honest delivery counter. The `latest_state` slot
        // coalesces, so it cannot measure how much arrived.
        applied_delta_count: &Arc<AtomicU64>,
        applied_delta_count_by_layer: &Arc<std::sync::Mutex<Vec<u64>>>,
        base_mismatch_drops: &Arc<AtomicU64>,
        base_mismatch_drops_by_layer: &Arc<std::sync::Mutex<Vec<u64>>>,
        stall_logged: &Arc<std::sync::Mutex<Vec<bool>>>,
        stale_delta_drops: &Arc<AtomicU64>,
        // Bug №636: the session this connection belongs to, and how many times
        // the applied world has been thrown away for a new one.
        match_id: &Arc<Mutex<Option<Uuid>>>,
        session_resets: &Arc<AtomicU64>,
        // Bug №636: same accounting for the full-state twin of the stale-delta
        // guard below.
        stale_full_state_drops: &Arc<AtomicU64>,
        // Bug №267 (A): the apply site arms a layer's resync request here; the
        // receive loop drains and wires it out.
        resync_requests: &ResyncRequestChannel,
        resync_requests_sent: &Arc<AtomicU64>,
        // Bug №698: where the world's static description is stored.
        map_description: &Arc<RwLock<Option<rfs_core::world::MapDescription>>>,
        running: &Arc<Mutex<bool>>,
        bandwidth: &BandwidthTracker,
        // Bug №169: the HeartbeatAck answer leaves through the limiter too.
        limiter: &BandwidthLimiter,
        limit_enabled: bool,
        connect_attempt: &Arc<Mutex<Option<Instant>>>,
        // Bug №700: the seal on the answers (HeartbeatAck, resync request)
        // and the counter it consumes.
        auth_key: &Option<Vec<u8>>,
        wire_seq: &Arc<Mutex<u64>>,
        data: &[u8],
    ) -> Result<(), ConnectionError> {
        if data.len() < HEADER_SIZE {
            // Bug №89: a short datagram is NOT "too large" — use the variant
            // with matching semantics so error handling keys off the right one.
            return Err(ConnectionError::PacketTooSmall(data.len(), HEADER_SIZE));
        }

        let (header, payload) = deserialize_packet(data)?;

        // Bug №77: keep the peer connection's view of the server sequence so
        // outbound headers (Input, Heartbeat) carry a real ack and the server
        // can GC its reliable queue instead of pinning it at the window cap.
        // Bug №167: also feed the reliable tracker, otherwise our own outbound
        // acks would keep claiming the last-seen sequence and the server could
        // never retire anything.
        // Bug №215: the verdict is kept — the dispatch below gates reliable
        // payloads on it (see the CommandAck branch).
        let reliable_arrival: Option<ReliableArrival> = {
            let guard = connection.lock();
            if let Some(conn) = guard.as_ref() {
                conn.record_remote(header.sequence, header.ack, header.ack_bitfield);
                conn.track_received_reliable(header.sequence, channel_from_u8(header.channel))
            } else {
                None
            }
        };

        let packet_type = match PacketType::from_u8(header.packet_type) {
            Some(packet_type) => packet_type,
            None => return Err(ConnectionError::InvalidPacketType(header.packet_type)),
        };

        match packet_type {
            PacketType::ConnectChallenge => {
                // Bug №216: the handshake the server expects before any
                // registration. The answer must not wait for the connect
                // loop's next retry — the loop only ever re-sends Connect,
                // and the server would keep re-challenging; send Confirm
                // straight from here through the same signed send path.
                let challenge: ConnectChallengePacket = deserialize_limited(payload)?;
                let confirm = ConnectConfirmPacket {
                    nonce: challenge.nonce,
                };
                let confirm_header = PacketHeader::new(
                    PacketType::ConnectConfirm,
                    ChannelType::ReliableOrdered,
                    0,
                    0,
                    0,
                    0,
                );
                let _ = Self::send_packet_static(
                    socket,
                    bandwidth,
                    limiter,
                    limit_enabled,
                    &confirm_header,
                    &confirm,
                    auth_key,
                    wire_seq,
                )
                .await;
            }
            PacketType::ConnectAccept => {
                let accept: ConnectAcceptPacket = deserialize_limited(payload)?;
                // Duplicate Accept (we retried, both got answered): refresh
                // state but emit Connected only once per session.
                let already = connection_id.lock().is_some();

                // Scoped so the guard is dropped before the `.await` below —
                // a parking_lot guard is not `Send` and the receive loop is
                // spawned with `tokio::spawn`.
                {
                    let mut conn_guard = connection.lock();
                    let conn = conn_guard.take().unwrap_or_else(|| {
                        Connection::new(
                            accept.assigned_client_id,
                            config.server_addr,
                            config.connection_config.clone(),
                        )
                    });
                    conn.set_state(ConnectionState::Connected);
                    conn.set_client_id(Some(accept.assigned_client_id));
                    // Bug №216: the accept is the ONLY courier of the session
                    // number. From here on, every Heartbeat/Input/Command we
                    // send carries it, and the server refuses those that do
                    // not echo what it minted for this connection.
                    conn.set_session(accept.session);
                    *conn_guard = Some(conn);
                }

                *server_tick.lock() = Tick(accept.server_tick);
                *server_time.lock() = accept.server_time;
                *connection_id.lock() = Some(accept.assigned_client_id);
                *connect_attempt.lock() = None;
                // Bug №82: a fresh session owns a fresh tick space — never keep
                // stale pending inputs the new server cannot ack.
                pending_inputs.lock().clear();
                *last_ack_tick.lock() = 0;

                // Bug №636: everything above is transport. The *world* this
                // client holds is the previous session's: layer clocks pinned
                // at the old server's last tick, mirrors full of its entities,
                // a snapshot history the new server will never re-base on. From
                // the packets alone a fresh session is indistinguishable from
                // a very late packet of the old one — a full state at tick ~5
                // loses to `incoming <= max_applied` and the client listens to
                // a ghost world forever. The accept names a session; a change
                // of session discards everything tied to it.
                let session_changed = {
                    let mut slot = match_id.lock();
                    let changed = !matches!(slot.as_ref(), Some(prev) if prev == &accept.match_id);
                    if changed {
                        *slot = Some(accept.match_id);
                    }
                    changed
                };
                if session_changed {
                    *last_applied_layer.lock() = [0; LAYER_COUNT];
                    entity_states.write().clear();
                    projectile_states.write().clear();
                    snapshot_buffer.reset();
                    snapshot_interpolator.lock().reset();
                    *latest_state.lock() = None;
                    // Per-session diagnostics reset with the world they
                    // measured, otherwise the next session starts with a
                    // "stalled" latch it never earned.
                    stall_logged
                        .lock()
                        .unwrap()
                        .iter_mut()
                        .for_each(|f| *f = false);
                    applied_delta_count.store(0, Ordering::Relaxed);
                    applied_delta_count_by_layer.lock().unwrap().fill(0);
                    base_mismatch_drops.store(0, Ordering::Relaxed);
                    base_mismatch_drops_by_layer.lock().unwrap().fill(0);
                    stale_delta_drops.store(0, Ordering::Relaxed);
                    stale_full_state_drops.store(0, Ordering::Relaxed);
                    // Bug №636: the discard is observable, not just counted —
                    // exactly the kind of event that must not happen silently.
                    warn!(
                        "match_id changed to {}: discarded the previous session's applied world",
                        accept.match_id
                    );
                    session_resets.fetch_add(1, Ordering::Relaxed);
                }

                if !already {
                    event_sender
                        .send_critical(ClientEvent::Connected {
                            connection_id: accept.assigned_client_id,
                            server_tick: accept.server_tick,
                            match_id: accept.match_id,
                        })
                        .await;
                }
            }
            PacketType::ConnectReject => {
                let reject: ConnectRejectPacket = deserialize_limited(payload)?;
                *connect_attempt.lock() = None;
                event_sender
                    .send_critical(ClientEvent::ConnectionFailed {
                        reason: reject.reason,
                    })
                    .await;
            }
            PacketType::Disconnect => {
                let disconnect: DisconnectPacket = deserialize_limited(payload)?;
                *running.lock() = false;
                event_sender
                    .send_critical(ClientEvent::Disconnected {
                        reason: disconnect.reason,
                    })
                    .await;
            }
            PacketType::Heartbeat => {
                let hb: HeartbeatPacket = deserialize_limited(payload)?;
                // Bug №216: only the accept carries the session, and the
                // accept went to the claimed socket's listener alone — a
                // beat that echoes a stranger's session is part of a spoof,
                // not of the game; answering it would confirm liveness.
                let known = connection.lock().as_ref().map(|conn| conn.session());
                if !known.is_some_and(|known| known == hb.session) {
                    return Ok(());
                }
                // The server pings us to measure ITS RTT on our ack — no send
                // stamp exists here, so we measure nothing and just answer.
                // Bug №176: this used to go out with sequence 0 and an empty
                // bitfield, so the server's own ack bookkeeping learned
                // nothing from our answers. Bug №167: carry the real SACK
                // state, and a real sequence, like every other send site.
                let (ack, ack_bitfield, ack_seq, session) = {
                    let guard = connection.lock();
                    match guard.as_ref() {
                        Some(conn) => {
                            let (a, b) = conn.ack_info();
                            (a, b, conn.next_sequence(), conn.session())
                        }
                        None => (0, 0, 0, SESSION_UNSET),
                    }
                };
                let ack_header = PacketHeader::new(
                    PacketType::HeartbeatAck,
                    ChannelType::ReliableOrdered,
                    ack_seq,
                    ack,
                    ack_bitfield,
                    0,
                );
                let ack_packet = HeartbeatPacket {
                    client_time: 0.0,
                    server_time: 0.0,
                    // Bug №216: echo the session so the server can tell a
                    // real answer from a spoofed one.
                    session,
                };
                let _ = Self::send_packet_static(
                    socket,
                    bandwidth,
                    limiter,
                    limit_enabled,
                    &ack_header,
                    &ack_packet,
                    // Bug №700: the answer is sealed like everything else.
                    auth_key,
                    wire_seq,
                )
                .await;
            }
            PacketType::HeartbeatAck => {
                // Bug №216: the ack carries the session the beat asked for; an
                // ack from a stranger's session would only drag RTT toward
                // wrong numbers, so it is refused before it moves anything.
                let ack: HeartbeatPacket = deserialize_limited(payload)?;
                let known = connection.lock().as_ref().map(|conn| conn.session());
                if !known.is_some_and(|known| known == ack.session) {
                    return Ok(());
                }
                // Bug №12: measure RTT against the last heartbeat WE sent
                // (stamped in start_send_loop), never Instant::now().elapsed()
                // which is always ~0. No nested lock: conn and rtt are distinct.
                let sample = connection
                    .lock()
                    .as_ref()
                    .map(|conn| conn.last_heartbeat.lock().elapsed());
                if let Some(sample) = sample {
                    // Read under the lock, send outside it — the guard is not
                    // `Send` and must not live across an await point.
                    let smoothed = {
                        let mut current = rtt.lock();
                        // Bug №176: clamp the sample — see `smooth_rtt`.
                        *current = smooth_rtt(*current, sample);
                        *current
                    };
                    event_sender
                        .send_critical(ClientEvent::RttUpdate { rtt: smoothed })
                        .await;
                }
            }
            PacketType::State | PacketType::StateDelta | PacketType::StateFull => {
                Self::process_state_packet(
                    packet_type,
                    payload,
                    snapshot_buffer,
                    snapshot_interpolator,
                    event_sender,
                    latest_state,
                    current_tick,
                    server_tick,
                    server_time,
                    entity_states,
                    projectile_states,
                    last_applied_layer,
                    // Bug №274: same as the direct path — a
                    // fragmented delta is applied the same way.
                    applied_delta_count,
                    applied_delta_count_by_layer,
                    base_mismatch_drops,
                    base_mismatch_drops_by_layer,
                    stall_logged,
                    stale_delta_drops,
                    stale_full_state_drops,
                    // Bug №267 (A): the drop site arms the request.
                    resync_requests,
                )?;
            }
            PacketType::Fragment => {
                let reassembled = {
                    let guard = connection.lock();
                    guard
                        .as_ref()
                        .and_then(|conn| conn.reassemble_fragment(payload))
                };
                if let Some((orig_type, bytes)) = reassembled {
                    match orig_type {
                        PacketType::State | PacketType::StateDelta | PacketType::StateFull => {
                            Self::process_state_packet(
                                orig_type,
                                &bytes,
                                snapshot_buffer,
                                snapshot_interpolator,
                                event_sender,
                                latest_state,
                                current_tick,
                                server_tick,
                                server_time,
                                entity_states,
                                projectile_states,
                                last_applied_layer,
                                // Bug №274: threaded through so the apply site
                                // can count deltas instead of inferring it from
                                // the coalescing slot.
                                applied_delta_count,
                                applied_delta_count_by_layer,
                                base_mismatch_drops,
                                base_mismatch_drops_by_layer,
                                stall_logged,
                                stale_delta_drops,
                                stale_full_state_drops,
                                // Bug №267 (A): the drop site arms the request.
                                resync_requests,
                            )?;
                        }
                        // Bug №163: these were dropped with a debug!(), so any
                        // reliable message above the MTU was lost silently.
                        // They arrive fragmented exactly like a full state.
                        PacketType::Event => {
                            if let Ok(packet) = deserialize_limited::<EventPacket>(&bytes) {
                                Self::dispatch_events(&packet, event_sender);
                            }
                        }
                        PacketType::CommandAck => {
                            if let Ok(ack) = deserialize_limited::<CommandAckPacket>(&bytes) {
                                event_sender
                                    .send_critical(ClientEvent::CommandAck {
                                        command_id: ack.command_id,
                                        success: ack.success,
                                        error: ack.error,
                                    })
                                    .await;
                            }
                        }
                        PacketType::InputAck => {
                            if let Ok(ack) = deserialize_limited::<InputAckPacket>(&bytes) {
                                Self::apply_input_ack(
                                    &ack,
                                    pending_inputs,
                                    last_ack_tick,
                                    event_sender,
                                );
                            }
                        }
                        other => {
                            debug!("Fragment for unsupported packet type: {:?}", other);
                        }
                    }
                }
            }
            PacketType::Event => {
                let event_packet: EventPacket = deserialize_limited(payload)?;
                Self::dispatch_events(&event_packet, event_sender);
            }
            PacketType::MapDescription => {
                // Bug №698: the world's static description. It is stored, not
                // interpreted: what to do with it is the client's business
                // (draw it, collide against it), and the network layer's job
                // is to make sure the client HAS it.
                let packet: MapDescriptionPacket = deserialize_limited(payload)?;
                *map_description.write() = Some(rfs_core::world::MapDescription {
                    name: packet.name,
                    bounds_min: packet.bounds_min,
                    bounds_max: packet.bounds_max,
                    obstacles: packet.obstacles,
                });
            }
            PacketType::InputAck => {
                let ack: InputAckPacket = deserialize_limited(payload)?;
                Self::apply_input_ack(&ack, pending_inputs, last_ack_tick, event_sender);
            }
            PacketType::CommandAck => {
                // Bug №215: only a genuinely NEW reliable datagram applies — a
                // retransmit means our receipt ack was lost, and firing the
                // same command result twice is worse than its (now bounded)
                // delay.
                if reliable_arrival != Some(ReliableArrival::New) {
                    return Ok(());
                }
                let ack: CommandAckPacket = deserialize_limited(payload)?;
                event_sender
                    .send_critical(ClientEvent::CommandAck {
                        command_id: ack.command_id,
                        success: ack.success,
                        error: ack.error,
                    })
                    .await;
            }
            _ => {
                debug!("Unhandled packet type: {:?}", packet_type);
            }
        }

        // Bug №267 (A): the stalled layers armed above want their request out
        // now, not on some later tick. The request itself is unreliable on
        // purpose: losing it costs one cooldown (half a second) of asking
        // again, while a reliable variant would spend the reliable window and
        // its admission budget on a packet the next input stream re-arms for
        // free.
        for layer in resync_requests.take_due() {
            let seq = connection.lock().as_ref().map(|c| c.next_sequence());
            let Some(seq) = seq else {
                break;
            };
            let (ack, ack_bitfield) = connection
                .lock()
                .as_ref()
                .map(|c| c.ack_info())
                .unwrap_or((0, 0));
            let header = PacketHeader::new(
                PacketType::ResyncRequest,
                ChannelType::UnreliableSequenced,
                seq,
                ack,
                ack_bitfield,
                0,
            );
            let request = ResyncRequestPacket { layer: layer as u8 };
            match Self::send_packet_static(
                socket,
                bandwidth,
                limiter,
                limit_enabled,
                &header,
                &request,
                // Bug №700: the request is sealed like everything else.
                auth_key,
                wire_seq,
            )
            .await
            {
                Ok(_) => {
                    resync_requests_sent.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    // A failed admission just re-arms on the next cooldown
                    // pass; not worth an error report per attempt.
                    debug!("resync request for layer {layer} not admitted: {e}");
                }
            }
        }

        Ok(())
    }

    /// Bug №163: shared by the direct and the fragmented receive paths.
    fn dispatch_events(packet: &EventPacket, event_sender: &EventSink) {
        for event in &packet.events {
            event_sender.send_delta(ClientEvent::GameEvent {
                event: event.clone(),
            });
        }
    }

    /// Bug №82/#71: the server echoes back input.tick — the very key we stored
    /// under — and every ack releases that slot. A negative ack (unknown
    /// ship/player) still consumes the entry.
    fn apply_input_ack(
        ack: &InputAckPacket,
        pending_inputs: &Arc<Mutex<HashMap<u32, InputPacket>>>,
        last_ack_tick: &Arc<Mutex<u32>>,
        event_sender: &EventSink,
    ) {
        pending_inputs.lock().remove(&ack.tick);
        if ack.accepted {
            // Reorder-safe: only move the high-water mark forward.
            let mut guard = last_ack_tick.lock();
            if ack.tick > *guard {
                *guard = ack.tick;
            }
        }
        event_sender.send_delta(ClientEvent::InputAck {
            tick: ack.tick,
            accepted: ack.accepted,
        });
    }

    /// Bug №171: record an input and keep the map bounded. The server acks
    /// over UDP, so acks are lost whenever they are dropped, filtered by the
    /// bandwidth limiter, or simply never sent — without a cap the map grows
    /// by ~30 entries/s for the whole session. The oldest ticks are evicted
    /// first: they are the ones a late ack is least likely to refer to.
    fn track_pending_input(
        pending_inputs: &Arc<Mutex<HashMap<u32, InputPacket>>>,
        input: &InputPacket,
    ) {
        /// Roughly two seconds of input at the 30 Hz server tick.
        const MAX_PENDING_INPUTS: usize = 64;

        let mut pending = pending_inputs.lock();
        pending.insert(input.tick, *input);

        let excess = pending.len().saturating_sub(MAX_PENDING_INPUTS);
        if excess == 0 {
            return;
        }
        let mut ticks: Vec<u32> = pending.keys().copied().collect();
        ticks.sort_unstable();
        for tick in ticks.into_iter().take(excess) {
            pending.remove(&tick);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_state_packet(
        packet_type: PacketType,
        payload: &[u8],
        snapshot_buffer: &Arc<SnapshotBuffer>,
        snapshot_interpolator: &Arc<Mutex<SnapshotInterpolator>>,
        event_sender: &EventSink,
        latest_state: &Arc<Mutex<Option<ClientEvent>>>,
        current_tick: &Arc<Mutex<Tick>>,
        server_tick: &Arc<Mutex<Tick>>,
        server_time: &Arc<Mutex<f64>>,
        entity_states: &Arc<RwLock<HashMap<EntityId, EntitySnapshot>>>,
        projectile_states: &Arc<RwLock<HashMap<EntityId, ProjectileSnapshot>>>,
        last_applied_layer: &Arc<Mutex<[u64; LAYER_COUNT]>>,
        // Bug №274: counted where a delta's base advances, so the figure
        // cannot be lost to the coalescing `latest_state` slot.
        applied_delta_count: &Arc<AtomicU64>,
        applied_delta_count_by_layer: &Arc<std::sync::Mutex<Vec<u64>>>,
        base_mismatch_drops: &Arc<AtomicU64>,
        base_mismatch_drops_by_layer: &Arc<std::sync::Mutex<Vec<u64>>>,
        stall_logged: &Arc<std::sync::Mutex<Vec<bool>>>,
        stale_delta_drops: &Arc<AtomicU64>,
        stale_full_state_drops: &Arc<AtomicU64>,
        // Bug №267 (A): armed here when a layer stalls on a base mismatch.
        resync_requests: &ResyncRequestChannel,
    ) -> Result<(), ConnectionError> {
        if packet_type == PacketType::State || packet_type == PacketType::StateFull {
            // Bug №168: full states may be up to MAX_REASSEMBLED_SIZE.
            let state: StatePacket = deserialize_full_state(payload)?;
            // Bug №20: refuse absurd states before they balloon client memory.
            if state.entities.len() > MAX_ENTITIES_PER_SNAPSHOT
                || state.projectiles.len() > MAX_PROJECTILES_PER_SNAPSHOT
            {
                warn!(
                    "dropping oversized state: {} entities, {} projectiles",
                    state.entities.len(),
                    state.projectiles.len()
                );
                return Ok(());
            }
            // Stale-guard for full states (reorder on unreliable channel):
            // an older full must not rewind layer clocks set by newer deltas.
            let incoming = state.server_tick as u64;
            let max_applied = last_applied_layer.lock().iter().copied().max().unwrap_or(0);
            if incoming <= max_applied {
                stale_full_state_drops.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            *server_tick.lock() = Tick(incoming);
            *server_time.lock() = state.server_time;
            *current_tick.lock() = Tick(incoming);

            let snapshot = Snapshot::from(&state);

            snapshot_buffer.write_snapshot(snapshot.clone());
            snapshot_interpolator.lock().add_snapshot(snapshot.clone());
            *last_applied_layer.lock() = [state.server_tick as u64; LAYER_COUNT];

            // A full state is authoritative: prune ghosts the delta stream
            // may have left behind (destroyed while we were desynced).
            //
            // Bug №170: the old code called states.retain(), which dropped the
            // entity from the map silently. Subscribers that drive their
            // renderers off EntityRemoved/ProjectileRemoved never heard about
            // the removal, so destroyed entities stayed on screen forever.
            // The removals are now reported like any other removal.
            {
                let present: HashSet<EntityId> =
                    snapshot.entities.iter().map(|e| e.entity_id).collect();
                let removed: Vec<EntityId> = {
                    let states = entity_states.read();
                    states
                        .keys()
                        .filter(|id| !present.contains(id))
                        .copied()
                        .collect()
                };
                {
                    let mut states = entity_states.write();
                    for id in &removed {
                        states.remove(id);
                    }
                    for entity in &snapshot.entities {
                        states.insert(entity.entity_id, entity.clone());
                    }
                }
                for entity_id in removed {
                    event_sender.send_delta(ClientEvent::EntityRemoved { entity_id });
                }
            }
            for entity in &snapshot.entities {
                event_sender.send_delta(ClientEvent::EntityUpdate {
                    entity_id: entity.entity_id,
                    snapshot: entity.clone(),
                });
            }

            {
                let present: HashSet<EntityId> =
                    snapshot.projectiles.iter().map(|p| p.entity_id).collect();
                let removed: Vec<EntityId> = {
                    let states = projectile_states.read();
                    states
                        .keys()
                        .filter(|id| !present.contains(id))
                        .copied()
                        .collect()
                };
                {
                    let mut states = projectile_states.write();
                    for id in &removed {
                        states.remove(id);
                    }
                    for proj in &snapshot.projectiles {
                        states.insert(proj.entity_id, proj.clone());
                    }
                }
                for projectile_id in removed {
                    event_sender.send_delta(ClientEvent::ProjectileRemoved { projectile_id });
                }
            }

            for proj in &snapshot.projectiles {
                event_sender.send_delta(ClientEvent::ProjectileUpdate {
                    projectile_id: proj.entity_id,
                    snapshot: proj.clone(),
                });
            }

            *latest_state.lock() = Some(ClientEvent::StateUpdate { snapshot });
        } else if packet_type == PacketType::StateDelta {
            let delta: StateDeltaPacket = deserialize_limited(payload)?;
            let layer = delta.layer as usize;
            if layer >= LAYER_COUNT {
                return Ok(());
            }
            // Bug №20: refuse absurd deltas before they balloon client memory.
            if delta.created.len() > MAX_ENTITIES_PER_SNAPSHOT
                || delta.updated.len() > MAX_ENTITIES_PER_SNAPSHOT
                || delta.destroyed.len() > MAX_ENTITIES_PER_SNAPSHOT
                || delta.projectile_created.len() > MAX_PROJECTILES_PER_SNAPSHOT
                || delta.projectile_updated.len() > MAX_PROJECTILES_PER_SNAPSHOT
                || delta.projectile_destroyed.len() > MAX_PROJECTILES_PER_SNAPSHOT
            {
                warn!("dropping oversized delta for layer {}", layer);
                return Ok(());
            }
            // Layers are independent streams: drop stale/out-of-order deltas,
            // apply fresh ones on top of the latest snapshot.
            if delta.server_tick as u64 <= last_applied_layer.lock()[layer] {
                // Measured while chasing #267: base-mismatch counter stayed at
                // zero across a 30-bot run, so this ordinary reorder guard is
                // the untraced drop path. Count it so the two paths are
                // distinguishable from outside.
                stale_delta_drops.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            // Bug №25: an incremental delta is only valid on top of the exact
            // base tick the server diffed against. A mismatch means we lost an
            // intermediate delta — applying would corrupt the merge, so drop.
            // Bug №23/№78: a resync (full layer replacement, diffed against an
            // empty base) is applied regardless — it is the healing mechanism
            // for exactly the state we may have lost.
            if !delta.is_resync && delta.base_tick as u64 != last_applied_layer.lock()[layer] {
                // This is a stall, not a hiccup. The layer rejects every later
                // delta until the next resync, which is why a client can sit at a
                // fraction of the server rate while reporting no errors at all.
                //
                // Logged once per layer, not once per drop. A single lost delta
                // is followed by a stream of rejected deltas until the resync, so
                // logging every drop turns the recovery path into the load
                // generator: tens of thousands of `warn!` calls with a mutex
                // lock and formatting each, on the thread that has to drain the
                // queue to recover in the first place. The counter below stays
                // exact and is what the stress report reads.
                {
                    let mut logged = stall_logged.lock().unwrap();
                    if !logged.get(layer).copied().unwrap_or(false) {
                        if let Some(slot) = logged.get_mut(layer) {
                            *slot = true;
                        }
                        warn!(
                            "layer {layer} stalled: delta base {} != applied {}; every later \
                             delta for this layer is dropped until the next resync (reported \
                             once per layer; see base_mismatch_drops for the count)",
                            delta.base_tick,
                            last_applied_layer.lock()[layer]
                        );
                    }
                }
                base_mismatch_drops.fetch_add(1, Ordering::Relaxed);
                if let Some(c) = base_mismatch_drops_by_layer.lock().unwrap().get_mut(layer) {
                    *c += 1;
                }
                // Bug №267 (A): the stalled layer asks the server for an
                // immediate resync instead of waiting out the scheduled
                // cadence. `arm` rate-limits itself; the receive loop drains
                // the arm right after this handler returns.
                resync_requests.arm(layer);
                return Ok(());
            }
            // Bug №269: these two are the client's view of "where the world
            // is", and they are read by `server_tick()`/`server_time()` and by
            // the stress harness, which counts an observed tick whenever the
            // value changes. They used to be overwritten from ANY layer's
            // delta, and the layers publish at different rates —
            // `LAYER_INTERVAL_TICKS = [2, u64::MAX, 1, 1]` — so a layer-0 delta
            // for tick 100 arriving after a layer-2 delta for tick 104 rewound
            // the global tick to 100. The harness then counted the rewind as
            // another observed tick, inflating the very metric used to judge
            // delivery. Per-layer ordering is enforced above against
            // `last_applied_layer`; the global pair only ever moves forward.
            {
                let mut current = server_tick.lock();
                advance_world_clock(
                    &mut current,
                    &mut server_time.lock(),
                    delta.server_tick as u64,
                    delta.server_time,
                );
            }

            // Bug №35: with no full state received yet (the one-time
            // State/StateFull on first contact was lost), an incremental delta
            // has nothing to sit on — wait for a layer resync instead. A
            // resync delta is a complete layer definition, so bootstrap an
            // empty snapshot and apply it (other layers heal on their own
            // resyncs).
            // Bug №637: the match result is already an owned copy (the buffer
            // hands out its own clone); the extra `latest.clone()` of the old
            // code was a second deep copy of every entity per applied delta.
            let mut base = match snapshot_buffer.get_latest() {
                Some(latest) => latest,
                None => {
                    if !delta.is_resync {
                        return Ok(());
                    }
                    let boot = Snapshot {
                        tick: Tick(delta.base_tick as u64),
                        time: delta.server_time,
                        entities: Vec::new(),
                        projectiles: Vec::new(),
                        events: Vec::new(),
                    };
                    snapshot_buffer.write_snapshot(boot.clone());
                    boot
                }
            };
            {
                if delta.is_resync {
                    // Full layer replace: drop everything this layer previously
                    // held so entities destroyed while we were desynced do not
                    // linger as ghosts.
                    base.entities
                        .retain(|e| entity_layer(e.entity_type) as usize != layer);
                    if layer == LAYER_PROJECTILE as usize {
                        base.projectiles.clear();
                    }
                }

                // Bug №637: `apply_delta` consumes the base (its vectors move
                // into the result), and the two storage sites each take an
                // owned copy; the third use — the `StateUpdate` event below —
                // moves the original into the slot. Three full copies exist by
                // storage contract (buffer, interpolator, latest-state slot);
                // the old code made five per applied delta.
                let target = base.apply_delta(&delta);
                snapshot_buffer.write_snapshot(target.clone());
                snapshot_interpolator.lock().add_snapshot(target.clone());
                last_applied_layer.lock()[layer] = delta.server_tick as u64;

                // Bug №274: count the delta HERE — at the moment its base
                // advances, which is the one place that cannot be undone and
                // cannot coalesce. `latest_state` cannot serve as the counter:
                // it is a single overwrite slot, so when several layers' deltas
                // land in one server tick they overwrite each other and the
                // consumer observes one update instead of four. Measured on the
                // real harness, that made a perfectly healthy 1-bot client
                // report 6.10 Hz while its layers had reached the server's own
                // tick with zero stalls.
                applied_delta_count.fetch_add(1, Ordering::Relaxed);
                if let Some(counts) = applied_delta_count_by_layer.lock().unwrap().get_mut(layer) {
                    *counts += 1;
                }

                // Bug №36: emit events only for entities this delta actually
                // carried, never re-emit the whole buffer.
                let changed: HashSet<EntityId> = delta
                    .created
                    .iter()
                    .map(|e| e.entity_id)
                    .chain(delta.updated.iter().map(|u| u.entity_id))
                    .collect();
                let changed_projectiles: HashSet<EntityId> = delta
                    .projectile_created
                    .iter()
                    .map(|p| p.entity_id)
                    .chain(delta.projectile_updated.iter().map(|u| u.entity_id))
                    .collect();

                if delta.is_resync {
                    let dead: Vec<EntityId> = {
                        let states = entity_states.read();
                        states
                            .iter()
                            .filter(|(id, ent)| {
                                entity_layer(ent.entity_type) as usize == layer
                                    && !target.entities.iter().any(|e| &e.entity_id == *id)
                            })
                            .map(|(id, _)| *id)
                            .collect()
                    };
                    for id in dead {
                        entity_states.write().remove(&id);
                        event_sender.send_delta(ClientEvent::EntityRemoved { entity_id: id });
                    }
                }

                {
                    let mut states = entity_states.write();
                    for id in &changed {
                        if let Some(entity) = target.entities.iter().find(|e| e.entity_id == *id) {
                            states.insert(*id, entity.clone());
                            event_sender.send_delta(ClientEvent::EntityUpdate {
                                entity_id: *id,
                                snapshot: entity.clone(),
                            });
                        }
                    }
                }
                for entity_id in &delta.destroyed {
                    entity_states.write().remove(entity_id);
                    event_sender.send_delta(ClientEvent::EntityRemoved {
                        entity_id: *entity_id,
                    });
                }

                {
                    let mut states = projectile_states.write();
                    if delta.is_resync && layer == LAYER_PROJECTILE as usize {
                        let dead: Vec<EntityId> = states
                            .keys()
                            .copied()
                            .filter(|id| !target.projectiles.iter().any(|p| &p.entity_id == id))
                            .collect();
                        for id in dead {
                            states.remove(&id);
                            event_sender
                                .send_delta(ClientEvent::ProjectileRemoved { projectile_id: id });
                        }
                    }
                    for id in &changed_projectiles {
                        if let Some(proj) = target.projectiles.iter().find(|p| p.entity_id == *id) {
                            states.insert(*id, proj.clone());
                            event_sender.send_delta(ClientEvent::ProjectileUpdate {
                                projectile_id: *id,
                                snapshot: proj.clone(),
                            });
                        }
                    }
                }
                for entity_id in &delta.projectile_destroyed {
                    projectile_states.write().remove(entity_id);
                    event_sender.send_delta(ClientEvent::ProjectileRemoved {
                        projectile_id: *entity_id,
                    });
                }

                *latest_state.lock() = Some(ClientEvent::StateUpdate { snapshot: target });
            }
        }
        Ok(())
    }

    fn start_send_loop(&self) {
        let socket = self.socket.clone();
        let connection = self.connection.clone();
        let running = self.running.clone();
        let current_tick = self.current_tick.clone();
        let input_sequence = self.input_sequence.clone();
        let pending_inputs = self.pending_inputs.clone();
        let bandwidth_tracker = self.bandwidth_tracker.clone();
        // Bug №169: the send loop needs the limiter too, otherwise the client's
        // outbound rate stays uncapped.
        let limiter = self.bandwidth_limiter.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        // Bug №700: the seal and its counter are the send loop's, too.
        let auth_key = self.auth_key.clone();
        let wire_seq = self.wire_seq.clone();
        // Bug №31: a degenerate tick_rate would panic on 1000/0 or busy-loop the
        // send task (interval 0 ms). Clamp to the sane 1..=1000 ticks/sec.
        let tick_rate = self.config.tick_rate.clamp(1, 1000);

        let handle = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_millis(1000 / tick_rate as u64));

            while *running.lock() {
                interval.tick().await;

                let tick = {
                    let mut ct = current_tick.lock();
                    *ct = ct.next();
                    *ct
                };

                let (pending, heartbeat, retransmits): (
                    Option<(PacketHeader, InputPacket)>,
                    Option<OutgoingPacket>,
                    Vec<OutgoingPacket>,
                ) = {
                    let conn_guard = connection.lock();
                    match conn_guard.as_ref() {
                        Some(conn) if conn.state() == ConnectionState::Connected => {
                            // Bug №77: carry the server's sequence high-water
                            // mark so it can GC its reliable queue instead of
                            // pinning it at the window cap.
                            // Bug №167: it is the real SACK state, not the last
                            // sequence we happened to see.
                            let (ack, ack_bitfield) = conn.ack_info();

                            let input = Self::build_input_packet(tick, conn.session());
                            let seq = {
                                let mut iseq = input_sequence.lock();
                                *iseq = iseq.wrapping_add(1);
                                *iseq
                            };

                            // Bug №82: store under input.tick — the very value
                            // the server echoes back in ack.tick (main.rs).
                            // Bug №171: bounded, the ack may never arrive.
                            Self::track_pending_input(&pending_inputs, &input);

                            let header = PacketHeader::new(
                                PacketType::Input,
                                ChannelType::UnreliableSequenced,
                                seq,
                                ack,
                                ack_bitfield,
                                0,
                            );

                            // Bug №12: the client pings the server itself and
                            // stamps the moment here; the HeartbeatAck handler
                            // measures RTT against this stamp.
                            let heartbeat = if conn.should_send_heartbeat() {
                                conn.update_heartbeat();
                                let hseq = conn.next_sequence();
                                let hb_header = PacketHeader::new(
                                    PacketType::Heartbeat,
                                    ChannelType::ReliableOrdered,
                                    hseq,
                                    ack,
                                    ack_bitfield,
                                    0,
                                );
                                OutgoingPacket::new(
                                    hb_header,
                                    HeartbeatPacket {
                                        client_time: 0.0,
                                        server_time: 0.0,
                                        // Bug №216: the beat proves the session;
                                        // a beat without the session the accept
                                        // carried keeps no connection alive.
                                        session: conn.session(),
                                    },
                                )
                            } else {
                                None
                            };

                            // Bug №215: mirror the just-built heartbeat into
                            // the ledger — the payload here is exactly what
                            // the wire sees.
                            if let Some(hb) = heartbeat.as_ref() {
                                conn.register_reliable_send(&hb.header, &hb.payload);
                            }

                            // Bug №215: retransmit pass on the same cadence as
                            // the input send; the ledger clamps per-pass count.
                            let retransmits = conn.collect_retransmits();

                            (Some((header, input)), heartbeat, retransmits)
                        }
                        _ => (None, None, Vec::new()),
                    }
                };

                if let Some((header, input)) = pending {
                    let _ = Self::send_packet_static(
                        &socket,
                        &bandwidth_tracker,
                        &limiter,
                        limit_enabled,
                        &header,
                        &input,
                        &auth_key,
                        &wire_seq,
                    )
                    .await;
                }
                if let Some(hb) = heartbeat {
                    let _ = Self::send_raw(
                        &socket,
                        &bandwidth_tracker,
                        &limiter,
                        limit_enabled,
                        &hb,
                        &auth_key,
                        &wire_seq,
                    )
                    .await;
                }
                // Bug №215: retransmits go through the plain raw path — the
                // ledger entry already exists, so no registration on this
                // side; `collect_retransmits` stamped the send itself.
                for packet in &retransmits {
                    let _ = Self::send_raw(
                        &socket,
                        &bandwidth_tracker,
                        &limiter,
                        limit_enabled,
                        packet,
                        &auth_key,
                        &wire_seq,
                    )
                    .await;
                }
            }
        });

        *self.send_handle.lock() = Some(handle);
    }

    fn build_input_packet(tick: Tick, session: u128) -> InputPacket {
        InputPacket {
            tick: tick.value() as u32,
            delta_time: TICK_DURATION.as_secs_f32(),
            move_forward: 0.0,
            move_right: 0.0,
            move_up: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            actions: InputActions::NONE,
            station_interaction: None,
            // Bug №216: the session echo rides every input.
            session,
        }
    }

    pub fn send_input(&self, mut input: InputPacket) {
        // Bug №216: the session echo overwrites whatever the caller
        // filled — the session belongs to the connection, not to callers,
        // so a caller that never negotiated cannot poison the field.
        input.session = self
            .connection
            .lock()
            .as_ref()
            .map(|conn| conn.session())
            .unwrap_or(SESSION_UNSET);

        let seq = {
            let mut iseq = self.input_sequence.lock();
            *iseq = iseq.wrapping_add(1);
            *iseq
        };

        // Bug №82: keyed by input.tick, the value the server ack echoes.
        // Bug №171: bounded — see track_pending_input.
        Self::track_pending_input(&self.pending_inputs, &input);

        // Bug №77: fill real acks so the server can GC its reliable queue.
        // Bug №167: real SACK state (contiguous mark + gap bits).
        let (ack, ack_bitfield) = self
            .connection
            .lock()
            .as_ref()
            .map(|c| c.ack_info())
            .unwrap_or((0, 0));

        let header = PacketHeader::new(
            PacketType::Input,
            ChannelType::UnreliableSequenced,
            seq,
            ack,
            ack_bitfield,
            0,
        );

        // Bug №172: queue instead of spawning a task per input.
        match bincode::serialize(&input) {
            Ok(payload) => self.enqueue(header, payload),
            Err(e) => warn!("send_input: serialize failed: {e}"),
        }
    }

    pub fn send_command(&self, command: ServerCommand) {
        if let Some(conn) = self.connection.lock().as_mut() {
            // Single transport sequence per datagram: command_id is derived
            // from the same header sequence (no SACK hole, no correlation drift).
            let seq = conn.next_sequence();
            let command_id = seq as u64;

            let packet = CommandPacket {
                command_id,
                command,
                // Bug №216: the session echo rides every command.
                session: conn.session(),
            };
            // Bug №77: ack the server's sequence high-water mark too.
            // Bug №167: real SACK state.
            let (ack, ack_bitfield) = conn.ack_info();
            let header = PacketHeader::new(
                PacketType::Command,
                ChannelType::ReliableOrdered,
                seq,
                ack,
                ack_bitfield,
                0,
            );

            // Bug №172: queue instead of spawning a task per command.
            match bincode::serialize(&packet) {
                Ok(payload) => {
                    // Bug №215: the command rides ReliableOrdered — mirror it
                    // into the ledger BEFORE the queue serializes the send, so
                    // a burst cannot strand an un-mirrored command.
                    conn.register_reliable_send(&header, &payload);
                    self.enqueue(header, payload);
                }
                Err(e) => warn!("send_command: serialize failed: {e}"),
            }
        }
    }

    pub fn get_interpolated_snapshot(&self, target_time: f64) -> Option<Snapshot> {
        self.snapshot_interpolator.lock().interpolate(target_time)
    }

    pub fn get_entity_state(&self, entity_id: EntityId) -> Option<EntitySnapshot> {
        self.entity_states.read().get(&entity_id).cloned()
    }

    pub fn get_projectile_state(&self, entity_id: EntityId) -> Option<ProjectileSnapshot> {
        self.projectile_states.read().get(&entity_id).cloned()
    }

    pub fn current_tick(&self) -> Tick {
        *self.current_tick.lock()
    }

    /// Bug #267: deltas dropped for base mismatch since connect. See the field.
    pub fn base_mismatch_drops(&self) -> u64 {
        self.base_mismatch_drops.load(Ordering::Relaxed)
    }

    /// Per-layer breakdown of `base_mismatch_drops`.
    pub fn base_mismatch_drops_by_layer(&self) -> Vec<u64> {
        self.base_mismatch_drops_by_layer.lock().unwrap().clone()
    }

    /// Bug №267 (A): resync requests this client put on the wire. Compare
    /// with `base_mismatch_drops`: every stalled layer should generate one
    /// request per cooldown, and the count shows the mechanism is live.
    pub fn resync_requests_sent(&self) -> u64 {
        self.resync_requests_sent.load(Ordering::Relaxed)
    }

    /// Bug №698: the world's static description, as the server told it.
    ///
    /// `None` until the server's description arrives — which it does right
    /// after the accept, before the first snapshot.
    pub fn map_description(&self) -> Option<rfs_core::world::MapDescription> {
        self.map_description.read().clone()
    }

    /// The last tick applied per layer. Both drop counters measure zero, so the
    /// ~20 Hz vs 30 Hz gap is not client-side loss. Per-layer values pin down
    /// which layers actually carry data and how often they advance.
    pub fn applied_layer_ticks(&self) -> [u64; LAYER_COUNT] {
        let guard = self.last_applied_layer.lock();
        *guard
    }

    /// Bug №274: how many deltas were APPLIED, counted at the point of
    /// application — never coalesced, never sampled.
    ///
    /// This exists because `applied rate`, measured from the `latest_state`
    /// slot, does not measure delivery. `StateUpdate` is written to a single
    /// overwrite slot, so when several layers' deltas land in one server tick
    /// they overwrite each other and the consumer sees one, not four. Measured
    /// on the real harness: 1 bot reported 6.10 Hz while its layers had actually
    /// reached the server's own tick with zero stalls — the network was fine
    /// and the number was an artefact of coalescing.
    ///
    /// The rate derived from THIS counter is the honest delivery figure, and it
    /// is independent of how fast the consumer polls.
    pub fn applied_delta_count(&self) -> u64 {
        self.applied_delta_count.load(Ordering::Relaxed)
    }

    /// Bug №274: `applied_delta_count` broken down per layer, so a slow layer
    /// can be told apart from a slow client.
    pub fn applied_delta_count_by_layer(&self) -> Vec<u64> {
        self.applied_delta_count_by_layer.lock().unwrap().clone()
    }

    /// Bug #267 follow-up: deltas dropped purely as stale/out-of-order
    /// (their tick was already applied), the drop path that base_mismatch
    /// (measured 0) did not cover.
    pub fn stale_delta_drops(&self) -> u64 {
        self.stale_delta_drops.load(Ordering::Relaxed)
    }

    pub fn get_stale_delta_drops(&self) -> u64 {
        self.stale_delta_drops.load(Ordering::Relaxed)
    }

    pub fn get_session_resets(&self) -> u64 {
        self.session_resets.load(Ordering::Relaxed)
    }

    pub fn get_stale_full_state_drops(&self) -> u64 {
        self.stale_full_state_drops.load(Ordering::Relaxed)
    }

    pub fn get_match_id(&self) -> Option<Uuid> {
        *self.match_id.lock()
    }

    pub fn server_tick(&self) -> Tick {
        *self.server_tick.lock()
    }

    pub fn server_time(&self) -> f64 {
        *self.server_time.lock()
    }

    pub fn rtt(&self) -> Duration {
        *self.rtt.lock()
    }

    pub fn connection_id(&self) -> Option<u32> {
        *self.connection_id.lock()
    }

    pub fn is_connected(&self) -> bool {
        self.connection
            .lock()
            .as_ref()
            .map(|c| c.state() == ConnectionState::Connected)
            .unwrap_or(false)
    }

    pub fn get_event_receiver(&self) -> Option<mpsc::Receiver<ClientEvent>> {
        self.event_receiver.lock().take()
    }

    /// Bug №175: take the newest coalesced snapshot, if one is pending.
    ///
    /// Drained after the bounded queue, so the consumer always ends up with
    /// the most recent world state even if intermediate ones were replaced.
    pub fn take_latest_state(&self) -> Option<ClientEvent> {
        self.latest_state.lock().take()
    }

    /// Bug №175: events the bounded queue refused. A non-zero value means the
    /// consumer is falling behind; it is reported, never silently absorbed.
    pub fn events_dropped(&self) -> u64 {
        self.events_dropped.load(Ordering::Relaxed)
    }

    /// Events refused because the receiver was gone.
    ///
    /// Counted apart from `events_dropped` because this is terminal, not
    /// backpressure: it means nothing was listening any more. A non-zero value
    /// while the process is otherwise idle points at a dropped receiver rather
    /// than a slow one.
    pub fn event_channel_closed(&self) -> u64 {
        self.events_closed.load(Ordering::Relaxed)
    }

    pub fn bandwidth_stats(&self) -> BandwidthStats {
        self.bandwidth_tracker.stats()
    }

    pub fn pending_input_count(&self) -> usize {
        self.pending_inputs.lock().len()
    }

    pub fn last_acknowledged_tick(&self) -> u32 {
        *self.last_acknowledged_tick.lock()
    }
}

#[cfg(test)]
mod resync_request_tests {
    //! Bug №267 (A): the client-side arming of a resync request.
    //!
    //! A stalled layer must ask once (not once per rejected delta), the ask
    //! must survive until the receive loop drains it, and the drain must hand
    //! back exactly the layers that were armed.

    use super::*;

    #[test]
    fn the_first_arm_fires_immediately() {
        let channel = ResyncRequestChannel::new();
        assert!(
            channel.arm(0),
            "a fresh client must not wait out the cooldown before its first ask"
        );
    }

    #[test]
    fn the_cooldown_blocks_an_immediate_re_arm() {
        let channel = ResyncRequestChannel::new();
        assert!(channel.arm(0));
        assert!(
            !channel.arm(0),
            "a long stall must ask once per cooldown, not once per rejected delta"
        );
        assert!(channel.arm(1), "the cooldown is per layer");
    }

    #[test]
    fn an_armed_layer_is_due_exactly_once() {
        let channel = ResyncRequestChannel::new();
        channel.arm(2);
        assert_eq!(channel.take_due(), vec![2]);
        assert!(
            channel.take_due().is_empty(),
            "a drained arm must not come back on its own"
        );
    }

    #[test]
    fn a_layer_outside_the_range_cannot_be_armed() {
        let channel = ResyncRequestChannel::new();
        assert!(!channel.arm(LAYER_COUNT));
        assert!(!channel.arm(usize::MAX));
        assert!(channel.take_due().is_empty());
    }

    #[test]
    fn several_layers_can_be_due_at_once() {
        let channel = ResyncRequestChannel::new();
        channel.arm(0);
        channel.arm(3);
        let mut due = channel.take_due();
        due.sort_unstable();
        assert_eq!(due, vec![0, 3]);
    }
}

#[cfg(test)]
mod input_ack_tests {
    //! Bug №71: the client frees a pending input only when the server answers.
    //! A REJECTED input (unknown player, missing ship) must free it too —
    //! otherwise the pending map grows for every input the server refused.

    use super::*;

    fn input_packet(tick: u32) -> InputPacket {
        InputPacket {
            tick,
            delta_time: 0.033,
            move_forward: 0.0,
            move_right: 0.0,
            move_up: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            actions: InputActions::NONE,
            station_interaction: None,
            // The session attach happens at the send sites; the field itself
            // is the session the server minted (bug №216).
            session: SESSION_UNSET,
        }
    }

    fn sink() -> (EventSink, mpsc::Receiver<ClientEvent>) {
        let (tx, rx) = mpsc::channel(16);
        (EventSink::new(tx, Arc::new(AtomicU64::new(0))), rx)
    }

    #[test]
    fn a_rejected_input_ack_clears_the_pending_entry() {
        let pending: Arc<Mutex<HashMap<u32, InputPacket>>> = Arc::new(Mutex::new(HashMap::new()));
        let last_ack: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
        let (sink, _rx) = sink();
        pending.lock().insert(5, input_packet(5));

        NetClient::apply_input_ack(
            &InputAckPacket {
                tick: 5,
                accepted: false,
            },
            &pending,
            &last_ack,
            &sink,
        );

        assert!(
            pending.lock().is_empty(),
            "a rejected input must not stay pending — the map grows without bound"
        );
    }

    #[test]
    fn a_rejected_ack_does_not_move_the_high_water_mark() {
        let pending: Arc<Mutex<HashMap<u32, InputPacket>>> = Arc::new(Mutex::new(HashMap::new()));
        let last_ack: Arc<Mutex<u32>> = Arc::new(Mutex::new(100));
        let (sink, _rx) = sink();
        pending.lock().insert(7, input_packet(7));

        NetClient::apply_input_ack(
            &InputAckPacket {
                tick: 7,
                accepted: false,
            },
            &pending,
            &last_ack,
            &sink,
        );

        assert_eq!(
            *last_ack.lock(),
            100,
            "a rejection must not move the acknowledged high-water mark"
        );
    }
}

#[cfg(test)]
mod disconnect_tests {
    //! Bug №68: a client that goes away must free its slot at once.
    //!
    //! The bug had two halves: the client sent the Disconnect in a spawned
    //! task that was aborted microseconds later (so the datagram almost never
    //! left), and the server dropped the Disconnect into the `_ => {}` arm —
    //! refreshing only `last_received`, which EXTENDED the timeout instead of
    //! ending the session. The slot was then released only by the 10 s sweep,
    //! and the comment claiming an immediate release was a lie.

    use super::*;
    use crate::{NetServer, ServerConfig};
    use std::time::Duration;

    async fn connected_pair() -> (NetServer, NetClient, mpsc::Receiver<ClientEvent>) {
        let server = NetServer::new(ServerConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            ..ServerConfig::default()
        })
        .await
        .expect("server bind");
        let addr = server.local_addr().expect("server local_addr");
        server.start();

        let client = NetClient::new(ClientConfig {
            server_addr: addr,
            player_name: "disconnect-test".to_string(),
            ..ClientConfig::default()
        })
        .await
        .expect("client create");
        client.connect();
        let rx = client.get_event_receiver().expect("event receiver");
        (server, client, rx)
    }

    async fn wait_until_connected(rx: &mut mpsc::Receiver<ClientEvent>, client: &NetClient) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(!left.is_zero(), "Connected: timed out");
            match tokio::time::timeout(left, rx.recv()).await {
                Ok(Some(ClientEvent::Connected { .. })) => return,
                Ok(Some(_)) => continue,
                Ok(None) => panic!("Connected: event channel closed"),
                Err(_) => panic!("Connected: timed out, dropped={}", client.events_dropped()),
            }
        }
    }

    #[tokio::test]
    async fn a_client_disconnect_frees_the_slot_without_waiting_for_the_timeout() {
        let (server, client, mut rx) = connected_pair().await;
        wait_until_connected(&mut rx, &client).await;

        // The server registers the session only after the handshake, so wait
        // for it instead of assuming it is already there.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while server.connection_count() == 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the server never registered the session"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(server.connection_count(), 1);

        client.disconnect(DisconnectReason::ClientQuit);

        // The 10 s connection timeout must not be the mechanism here: the
        // Disconnect has to do it, and it has to do it now.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while server.connection_count() != 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the slot was not freed by the Disconnect — it is waiting for the timeout"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            server.connection_count(),
            0,
            "a disconnected client must not hold a slot"
        );
    }
}

#[cfg(test)]
mod event_delivery_tests {
    //! End-to-end coverage for the client event path.
    //!
    //! Regression guard: `EventSink::send` is `async`, so a call site that
    //! forgets `.await` still compiles — it just drops the future, and the
    //! event is lost with no error anywhere. Every test below drives real UDP
    //! traffic and then asserts the event actually arrived at the receiver, so
    //! a missing `await` fails the build rather than passing silently.

    use super::*;
    use crate::{NetServer, ServerConfig};
    use rfs_core::math::{Transform, Vec3f};
    use std::time::Duration;

    /// A real client/server pair over loopback UDP, plus the event receiver.
    ///
    /// `get_event_receiver` *takes* the receiver, so it can only be called
    /// once per client — the caller keeps it for the whole test.
    async fn connected_pair() -> (NetServer, NetClient, mpsc::Receiver<ClientEvent>) {
        let server = NetServer::new(ServerConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            ..ServerConfig::default()
        })
        .await
        .expect("server bind");
        let addr = server.local_addr().expect("server local_addr");
        server.start();

        let client = NetClient::new(ClientConfig {
            server_addr: addr,
            player_name: "event-test".to_string(),
            ..ClientConfig::default()
        })
        .await
        .expect("client create");
        client.connect();
        let rx = client.get_event_receiver().expect("event receiver");
        (server, client, rx)
    }

    /// Wait for the first event satisfying `want`, skipping the rest.
    ///
    /// Panics on timeout rather than returning `None`, so a lost event fails
    /// the test instead of being silently skipped.
    async fn wait_for<F>(
        rx: &mut mpsc::Receiver<ClientEvent>,
        client: &NetClient,
        what: &str,
        mut want: F,
    ) -> ClientEvent
    where
        F: FnMut(&ClientEvent) -> bool,
    {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !left.is_zero(),
                "{what}: timed out, dropped={}",
                client.events_dropped()
            );
            match tokio::time::timeout(left, rx.recv()).await {
                Ok(Some(ev)) => {
                    if want(&ev) {
                        return ev;
                    }
                }
                Ok(None) => panic!(
                    "{what}: event channel closed, dropped={}",
                    client.events_dropped()
                ),
                Err(_) => panic!("{what}: timed out, dropped={}", client.events_dropped()),
            }
        }
    }

    fn is_connected(ev: &ClientEvent) -> bool {
        matches!(ev, ClientEvent::Connected { .. })
    }

    fn is_disconnected(ev: &ClientEvent) -> bool {
        matches!(ev, ClientEvent::Disconnected { .. })
    }

    #[tokio::test]
    async fn a_connected_event_reaches_the_receiver() {
        let (server, client, mut rx) = connected_pair().await;
        let ev = wait_for(&mut rx, &client, "Connected", is_connected).await;
        match ev {
            ClientEvent::Connected { connection_id, .. } => {
                assert_ne!(connection_id, 0, "the server must assign a real id");
            }
            other => panic!("expected Connected, got {other:?}"),
        }
        assert_eq!(
            client.events_dropped(),
            0,
            "nothing should be dropped on a clean connect"
        );
        server.stop();
    }

    /// A **server-initiated** disconnect. This drives the `send(...).await` in
    /// the receive loop, which is the exact call site whose missing `.await`
    /// silently swallowed every event.
    ///
    /// A locally requested `client.disconnect()` is deliberately *not* used
    /// here: it is silent by design (the caller asked for it), so it cannot
    /// prove anything about the receive-loop path.
    #[tokio::test]
    async fn a_server_initiated_disconnect_reaches_the_receiver() {
        let (server, client, mut rx) = connected_pair().await;
        let connection_id = match wait_for(&mut rx, &client, "Connected", is_connected).await {
            ClientEvent::Connected { connection_id, .. } => connection_id,
            other => panic!("expected Connected, got {other:?}"),
        };

        assert!(
            server.disconnect_client(connection_id, DisconnectReason::Kicked),
            "the server must have had that connection to drop"
        );

        match wait_for(&mut rx, &client, "Disconnected", is_disconnected).await {
            ClientEvent::Disconnected { reason } => {
                assert_eq!(
                    reason,
                    DisconnectReason::Kicked,
                    "the reason must survive the wire"
                );
            }
            other => panic!("expected Disconnected, got {other:?}"),
        }
        server.stop();
    }

    /// Bug №610: stopping the server has to be something the client *learns*.
    /// `stop()` used to abort the outbox writer first and queue the farewell
    /// `Disconnect(ServerShutdown)` afterwards, into a channel with no reader
    /// left — so the one packet that says "the match is over" was the one packet
    /// guaranteed not to arrive, and the client saw a silent socket instead.
    /// This is the whole point of wiring Ctrl-C to `stop()` in the first place,
    /// so it is pinned end to end rather than through a mock.
    #[tokio::test]
    async fn a_stopping_server_tells_the_client_why() {
        let (server, client, mut rx) = connected_pair().await;
        wait_for(&mut rx, &client, "Connected", is_connected).await;

        server.stop_gracefully().await;

        match wait_for(&mut rx, &client, "Disconnected", is_disconnected).await {
            ClientEvent::Disconnected { reason } => {
                assert_eq!(
                    reason,
                    DisconnectReason::ServerShutdown,
                    "the client must be told the match ended by the server, not by a dead socket"
                );
            }
            other => panic!("expected Disconnected after a server shutdown, got {other:?}"),
        }
    }

    /// `Full` and `Closed` must not be conflated.
    ///
    /// The two demand opposite responses: `Full` means a lagging consumer that
    /// is still there (keep going, the next state packet supersedes it), while
    /// `Closed` means nobody is listening (stop). A sink that reported both as
    /// one value would let a dead client look like a slow one forever.
    #[tokio::test]
    async fn a_full_queue_is_not_reported_as_a_closed_channel() {
        let dropped = Arc::new(AtomicU64::new(0));
        let (tx, mut rx) = mpsc::channel(1);
        let sink = EventSink::new(tx, dropped.clone());

        // One event fits.
        assert_eq!(
            sink.try_send(ClientEvent::Error {
                error: "first".into()
            }),
            Disposition::Delivered
        );
        // The next has nowhere to go: transient backpressure.
        assert_eq!(
            sink.try_send(ClientEvent::Error {
                error: "second".into()
            }),
            Disposition::Full
        );
        assert!(
            !sink.channel_is_closed(),
            "a full queue is not a closed channel"
        );
        assert_eq!(sink.closed.load(Ordering::Relaxed), 0);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);

        // Draining proves the consumer was alive all along.
        assert!(matches!(rx.recv().await, Some(ClientEvent::Error { .. })));
        assert_eq!(
            sink.try_send(ClientEvent::Error {
                error: "third".into()
            }),
            Disposition::Delivered
        );

        // Drop the consumer: now, and only now, it is terminal.
        drop(rx);
        assert_eq!(
            sink.try_send(ClientEvent::Error {
                error: "fourth".into()
            }),
            Disposition::Closed
        );
        assert!(
            sink.channel_is_closed(),
            "a dropped receiver must be reported as closed"
        );
    }

    /// Bug №636 harness: a client driven by hand-crafted packets from a fake
    /// UDP "server" socket. Unlike `connected_pair`, the session identity
    /// (`match_id`) is fully under test control — that identity is exactly what
    /// the session-reset decision in the accept handler keys off, so a real
    /// server would hide the one thing being tested.
    type CraftedSession = (
        NetClient,
        UdpSocket,
        mpsc::Receiver<ClientEvent>,
        SocketAddr,
    );

    async fn crafted_session() -> CraftedSession {
        let server = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("fake server bind");
        let server_addr = server.local_addr().expect("fake server addr");
        let client = NetClient::new(ClientConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            server_addr,
            player_name: "session-reset-test".to_string(),
            ..ClientConfig::default()
        })
        .await
        .expect("client create");
        client.connect();
        let rx = client.get_event_receiver().expect("event receiver");

        // The first datagram the fake server sees is the client's
        // ConnectRequest; answering it is both the handshake and how the test
        // learns the client's address — the same two jobs a real server does.
        let mut buf = [0u8; 2048];
        let (_, client_addr) = server.recv_from(&mut buf).await.expect("connect request");
        (client, server, rx, client_addr)
    }

    /// Hand-crafted UDP datagram in the exact wire format `deserialize_packet`
    /// expects: bincode header (with payload_size), then the bincode body.
    fn craft<T: Serializable>(
        packet_type: PacketType,
        channel: ChannelType,
        sequence: u32,
        packet: &T,
    ) -> Vec<u8> {
        serialize_packet(
            packet,
            PacketHeader::new(packet_type, channel, sequence, 0, 0, 0),
        )
        .expect("serialize crafted packet")
    }

    /// One entity, enough for the state path: a full state is authoritative,
    /// so whatever the test sends is what the mirror holds.
    fn craft_entity(id: u64) -> EntityState {
        EntityState {
            entity_id: EntityId(id),
            entity_type: EntityType::Ship,
            transform: Transform::from_position(Vec3f::new(1.0, 0.0, 0.0)),
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: 100.0,
            max_health: 100.0,
            flags: EntityFlags::NONE,
            ship_data: None,
            station_data: None,
            player_data: None,
        }
    }

    fn craft_full_state(tick: u32, ids: &[u64]) -> StatePacket {
        StatePacket {
            server_tick: tick,
            server_time: f64::from(tick) / 30.0,
            entities: ids.iter().copied().map(craft_entity).collect(),
            projectiles: Vec::new(),
        }
    }

    /// Poll for a state that no event announces (counters, layer clocks).
    async fn wait_until<F: FnMut() -> bool>(what: &str, mut check: F) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if check() {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "{what}: client never reached the expected state"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn send_to(server: &UdpSocket, client_addr: SocketAddr, bytes: &[u8]) {
        // Bug №700: the wire frames EVERY datagram with the 24-byte auth
        // block in front, and the tests run the unsigned dialect: a zero
        // block with no MAC, exactly what `unseal(None, ...)` strips.
        let mut framed = vec![0u8; crate::auth::AUTH_WIRE_LEN];
        framed.extend_from_slice(bytes);
        server
            .send_to(&framed, client_addr)
            .await
            .expect("send crafted packet");
    }

    /// Bug №636: a reconnect to a *different* session must throw the previous
    /// session's applied world away. Pre-fix, the new session's full state
    /// (ticks near zero) lost to the stale-guard (`incoming <= max_applied`)
    /// forever, and the client listened to a ghost world with no warning.
    ///
    /// Also pins the counter-behaviour of the reset: the first session counts
    /// as a reset, a *duplicate* Accept for the same session must NOT count —
    /// a retransmitted handshake would otherwise wipe a healthy world.
    #[tokio::test]
    async fn a_connect_accept_for_a_new_session_discards_the_previous_world() {
        let (client, server, mut rx, client_addr) = crafted_session().await;
        let seq = Arc::new(AtomicU64::new(2));
        let next_seq = |seq: &AtomicU64| seq.fetch_add(1, Ordering::Relaxed) as u32;

        // Session 1: accept, then a full state that pins every layer clock.
        let m1 = Uuid::new_v4();
        let accept1 = ConnectAcceptPacket {
            server_tick: 12,
            server_time: 0.4,
            assigned_client_id: 7,
            match_id: m1,
            tick_rate: 30,
            // Bug №216: the session was not the test subject here.
            session: 1,
        };
        send_to(
            &server,
            client_addr,
            &craft(
                PacketType::ConnectAccept,
                ChannelType::ReliableOrdered,
                next_seq(&seq),
                &accept1,
            ),
        )
        .await;
        let connected = wait_for(&mut rx, &client, "Connected(session1)", is_connected).await;
        match connected {
            ClientEvent::Connected { match_id, .. } => {
                assert_eq!(match_id, m1, "the client must be attached to session 1");
            }
            other => panic!("expected Connected, got {other:?}"),
        }
        assert_eq!(client.get_match_id(), Some(m1));

        send_to(
            &server,
            client_addr,
            &craft(
                PacketType::StateFull,
                ChannelType::Unreliable,
                next_seq(&seq),
                &craft_full_state(12, &[777]),
            ),
        )
        .await;
        wait_until("full state 12 applied", || {
            client.applied_layer_ticks().iter().all(|&t| t == 12)
        })
        .await;
        assert_eq!(
            client.get_session_resets(),
            1,
            "the first session counts as one reset"
        );

        // A retransmitted Accept for the SAME session must not wipe the world.
        send_to(
            &server,
            client_addr,
            &craft(
                PacketType::ConnectAccept,
                ChannelType::ReliableOrdered,
                next_seq(&seq),
                &accept1,
            ),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            client.applied_layer_ticks().iter().all(|&t| t == 12),
            "a duplicate Accept for the same session must preserve the applied world"
        );
        assert_eq!(
            client.get_session_resets(),
            1,
            "a duplicate Accept must not count as a reset"
        );

        // Session 2: a *new* match id. The accept carries no world of its own —
        // the old world (ticks pinned at 12) must be discarded here, not by the
        // next full state.
        let m2 = Uuid::new_v4();
        let accept2 = ConnectAcceptPacket {
            server_tick: 5,
            server_time: 0.1,
            assigned_client_id: 7,
            match_id: m2,
            tick_rate: 30,
            // Bug №216: the session was not the test subject here.
            session: 2,
        };
        send_to(
            &server,
            client_addr,
            &craft(
                PacketType::ConnectAccept,
                ChannelType::ReliableOrdered,
                next_seq(&seq),
                &accept2,
            ),
        )
        .await;
        wait_until("session 2 accepted", || client.get_match_id() == Some(m2)).await;
        wait_until("layer clocks reset to zero", || {
            client.applied_layer_ticks().iter().all(|&t| t == 0)
        })
        .await;
        assert_eq!(
            client.get_session_resets(),
            2,
            "accepting a second session resets the world once"
        );

        // The proof that the world really moved: session 2's low-tick full
        // state now applies. Pre-fix this is a ghost: tick 8 loses to the
        // session-1 clock (12), every state is dropped, stale_full_state_drops
        // climbs, and the client listens to the dead session forever.
        send_to(
            &server,
            client_addr,
            &craft(
                PacketType::StateFull,
                ChannelType::Unreliable,
                next_seq(&seq),
                &craft_full_state(8, &[888]),
            ),
        )
        .await;
        wait_until("session 2 full state 8 applied", || {
            client.applied_layer_ticks().iter().all(|&t| t == 8)
        })
        .await;
        assert_eq!(
            client.get_stale_full_state_drops(),
            0,
            "session 2 states must not be dropped as stale"
        );
    }

    /// Bug №636: the stale-guard for full states used to `return Ok(())` in
    /// silence — the drop path №267's counter work could never cover, and the
    /// reason shadow №№571-636: "a client that resets on every reconnect never
    /// accumulates a world" could only be found by luck.
    #[tokio::test]
    async fn a_stale_full_state_is_counted_not_dropped_in_silence() {
        let (client, server, mut rx, client_addr) = crafted_session().await;
        let seq = Arc::new(AtomicU64::new(2));
        let next_seq = |seq: &AtomicU64| seq.fetch_add(1, Ordering::Relaxed) as u32;

        let accept = ConnectAcceptPacket {
            server_tick: 500,
            server_time: 16.0,
            assigned_client_id: 9,
            match_id: Uuid::new_v4(),
            tick_rate: 30,
            // Bug №216: the session was not the test subject here.
            session: 3,
        };
        send_to(
            &server,
            client_addr,
            &craft(
                PacketType::ConnectAccept,
                ChannelType::ReliableOrdered,
                next_seq(&seq),
                &accept,
            ),
        )
        .await;
        wait_for(&mut rx, &client, "Connected", is_connected).await;

        send_to(
            &server,
            client_addr,
            &craft(
                PacketType::StateFull,
                ChannelType::Unreliable,
                next_seq(&seq),
                &craft_full_state(500, &[77]),
            ),
        )
        .await;
        wait_until("full state 500 applied", || {
            client.applied_layer_ticks().iter().all(|&t| t == 500)
        })
        .await;

        // Tick 100 arrived after 500 was applied: genuinely stale, must be
        // refused — but every other drop path on this stream counts its
        // refusals, and so does this one.
        send_to(
            &server,
            client_addr,
            &craft(
                PacketType::StateFull,
                ChannelType::Unreliable,
                next_seq(&seq),
                &craft_full_state(100, &[78]),
            ),
        )
        .await;
        wait_until("stale full state counted", || {
            client.get_stale_full_state_drops() == 1
        })
        .await;
        assert!(
            client.applied_layer_ticks().iter().all(|&t| t == 500),
            "a stale full state must not rewind the applied world"
        );
    }

    /// Bug №34 residual, closed by №269's `advance_world_clock`: an older
    /// tick from a slower layer arriving after a newer one must not rewind
    /// the shared clock — neither the tick nor the derived time.
    #[test]
    fn an_older_tick_never_rewinds_the_world_clock() {
        let mut tick = Tick(100);
        let mut time = 100.5;

        assert!(
            !advance_world_clock(&mut tick, &mut time, 90, 90.25),
            "an older tick is refused, not applied"
        );
        assert_eq!(tick, Tick(100), "the tick stays put");
        assert_eq!(time, 100.5, "the derived time must not drift either");

        // Equal ticks are also refused: the guard is strictly-greater.
        assert!(!advance_world_clock(&mut tick, &mut time, 100, 100.5));
        assert_eq!(tick, Tick(100));
        assert_eq!(time, 100.5);
    }

    #[test]
    fn a_newer_tick_moves_the_clock_and_its_time_together() {
        let mut tick = Tick(0);
        let mut time = 0.0;

        assert!(advance_world_clock(&mut tick, &mut time, 12, 41.0));
        assert_eq!(tick, Tick(12));
        assert_eq!(time, 41.0);
    }
}

#[cfg(test)]
mod auth_pair_tests {
    //! Bug №700: the whole datagram path, keyed. The unit tests of `auth.rs`
    //! prove the block arithmetic; these prove the loop — a signed client
    //! reaches a signed server, and nobody else does.

    use super::*;
    use crate::{NetServer, ServerConfig};
    use std::time::Duration;

    async fn signed_pair(
        server_token: Option<String>,
        client_token: Option<String>,
    ) -> (NetServer, NetClient, mpsc::Receiver<ClientEvent>) {
        let server = NetServer::new(ServerConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            ..ServerConfig::default()
        })
        .await
        .expect("server bind");
        server.set_auth_token(server_token.clone());
        let addr = server.local_addr().expect("server local_addr");
        server.start();

        let client = NetClient::new(ClientConfig {
            server_addr: addr,
            player_name: "auth-test".to_string(),
            auth_token: client_token,
            ..ClientConfig::default()
        })
        .await
        .expect("client create");
        client.connect();
        let rx = client.get_event_receiver().expect("event receiver");
        (server, client, rx)
    }

    #[tokio::test]
    async fn a_client_with_the_configured_secret_connects() {
        let (server, client, mut rx) = signed_pair(
            Some("the shared secret".into()),
            Some("the shared secret".into()),
        )
        .await;

        // A signed handshake means every datagram grew by 24 bytes and now
        // travels through seal/unseal on both sides; if any of that framing
        // is off, no Connected ever arrives and the pull below says so.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !left.is_zero(),
                "Connected: timed out — the signed handshake broke the wire, dropped={}",
                client.events_dropped()
            );
            match tokio::time::timeout(left, rx.recv()).await {
                Ok(Some(ClientEvent::Connected { .. })) => break,
                Ok(Some(_)) => continue,
                Ok(None) => panic!("Connected: event channel closed"),
                Err(_) => panic!("Connected: timed out, dropped={}", client.events_dropped()),
            }
        }
        assert_eq!(
            server.connection_count(),
            1,
            "a signed client reaches a signed server"
        );
        client.disconnect(DisconnectReason::ClientQuit);
    }

    #[tokio::test]
    async fn a_client_with_a_foreign_secret_never_connects() {
        let (server, client, mut rx) =
            signed_pair(Some("secret-a".into()), Some("secret-b".into())).await;

        // No reject is sent for an unverifiable datagram: that would turn the
        // authentication itself into a tool for flooding honest peers. The
        // client keeps retransmitting; the server keeps dropping. Before the
        // 5 s connect timeout fires, nothing about this should look like a
        // connection.
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert_eq!(
            server.connection_count(),
            0,
            "a datagram signed with a different secret must not open a connection"
        );
        match rx.try_recv() {
            Ok(ClientEvent::Connected { .. }) | Ok(ClientEvent::ConnectionFailed { .. }) => {
                panic!("a foreign secretary must neither connect nor close the handshake")
            }
            _ => {}
        }
        client.disconnect(DisconnectReason::ClientQuit);
    }

    #[tokio::test]
    async fn an_unsigned_client_connects_to_an_unsigned_server() {
        let (server, client, mut rx) = signed_pair(None, None).await;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !left.is_zero(),
                "Connected: timed out — the framing change broke the unsigned dialect, dropped={}",
                client.events_dropped()
            );
            match tokio::time::timeout(left, rx.recv()).await {
                Ok(Some(ClientEvent::Connected { .. })) => break,
                Ok(Some(_)) => continue,
                Ok(None) => panic!("Connected: event channel closed"),
                Err(_) => panic!("Connected: timed out, dropped={}", client.events_dropped()),
            }
        }
        assert_eq!(
            server.connection_count(),
            1,
            "an unsigned client and server must still talk: the block is framing now, not a switch"
        );
        client.disconnect(DisconnectReason::ClientQuit);
    }
}

#[cfg(test)]
mod map_description_tests {
    //! Bug №699, recovered by SBF3 from the chat record after the worktree
    //! restore burned the originals: the connected client must hear about the
    //! world the server loaded, and a server that has no world must not invent
    //! one.

    use super::*;
    use crate::{NetServer, ServerConfig};
    use rfs_core::math::Vec3f;
    use rfs_core::world::{MapDescription, MapObstacleDescription, ObstacleKind};
    use std::time::Duration;

    /// One-structure world: enough to compare name, bounds and the structure
    /// itself, and small enough that no fragmentation is involved.
    fn one_structure() -> MapDescription {
        MapDescription {
            name: "smoke-world".to_string(),
            bounds_min: Vec3f::new(-1000.0, -1000.0, -1000.0),
            bounds_max: Vec3f::new(1000.0, 1000.0, 1000.0),
            obstacles: vec![MapObstacleDescription {
                id: 100_001,
                kind: ObstacleKind::Structure,
                min: Vec3f::new(300.0, -20.0, -20.0),
                max: Vec3f::new(340.0, 20.0, 20.0),
            }],
        }
    }

    /// The map-description send sits directly behind the ConnectAccept, so the
    /// same connection path as the auth pair tests is what carries it.
    async fn pair(world: Option<MapDescription>) -> (NetServer, NetClient) {
        let server = NetServer::new(ServerConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            ..ServerConfig::default()
        })
        .await
        .expect("server bind");
        if let Some(world) = world {
            server.set_world_description(world);
        }
        let addr = server.local_addr().expect("server local_addr");
        server.start();

        let client = NetClient::new(ClientConfig {
            server_addr: addr,
            player_name: "map-test".to_string(),
            ..ClientConfig::default()
        })
        .await
        .expect("client create");
        client.connect();
        (server, client)
    }

    #[tokio::test]
    async fn a_connected_client_receives_the_world_description() {
        // Held for the whole test: the server owns its sockets, so binding
        // with `_` would drop it mid-handshake.
        let (_server, client) = pair(Some(one_structure())).await;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let received = loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !left.is_zero(),
                "map description: timed out, the description never arrived"
            );
            if let Some(description) = client.map_description() {
                break description;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };

        assert_eq!(received.name, "smoke-world");
        assert_eq!(received.bounds_min, Vec3f::new(-1000.0, -1000.0, -1000.0));
        assert_eq!(received.bounds_max, Vec3f::new(1000.0, 1000.0, 1000.0));
        assert_eq!(received.obstacles.len(), 1, "exactly the loaded structure");
        let structure = &received.obstacles[0];
        assert_eq!(structure.id, 100_001);
        assert_eq!(structure.kind, ObstacleKind::Structure);
        assert_eq!(structure.min, Vec3f::new(300.0, -20.0, -20.0));
        assert_eq!(structure.max, Vec3f::new(340.0, 20.0, 20.0));

        client.disconnect(DisconnectReason::ClientQuit);
    }

    #[tokio::test]
    async fn a_server_without_a_world_does_not_invent_one() {
        let (server, client) = pair(None).await;

        // The description rides behind ConnectAccept, so give the wire ample
        // time to run the whole handshake and its retries before trusting the
        // absence: a None at 100 ms would prove nothing.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            client.map_description().is_none(),
            "a server with no world must not describe one"
        );
        // No world does not mean no clients: connecting worked before №699 and
        // keeps working — the absence is only about the description.
        assert_eq!(server.connection_count(), 1);
        assert!(server.world_description().is_none());
        client.disconnect(DisconnectReason::ClientQuit);
    }
}
