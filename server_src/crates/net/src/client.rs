use crate::connection::{channel_from_u8, is_reliable_channel, smooth_rtt, Connection, ConnectionConfig, ConnectionState, ConnectionError, OutgoingPacket};
use crate::snapshot::{Snapshot, SnapshotBuffer, SnapshotInterpolator, EntitySnapshot, ProjectileSnapshot, LAYER_COUNT, LAYER_PROJECTILE, entity_layer, MAX_ENTITIES_PER_SNAPSHOT, MAX_PROJECTILES_PER_SNAPSHOT};
use crate::bandwidth::{Admission, BandwidthTracker, BandwidthLimiter, BandwidthStats};
use rfs_core::packet::*;
use rfs_core::time::{Tick, TICK_RATE, TICK_DURATION};
use bytes::BytesMut;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn, error};
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
pub fn advance_world_clock(current: &mut Tick, current_time: &mut f64, incoming: u64, incoming_time: f64) -> bool {
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

fn deserialize_limited<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, bincode::Error> {
    use bincode::Options;
    bincode::DefaultOptions::new()
        .with_limit(WIRE_LIMIT)
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize(payload)
}

fn deserialize_full_state<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, bincode::Error> {
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
        Self { tx, dropped, closed: Arc::new(AtomicU64::new(0)) }
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
    outbox: mpsc::UnboundedSender<OutboxItem>,
    outbox_handle: Mutex<Option<JoinHandle<()>>>,
    outbox_rx: Mutex<Option<mpsc::UnboundedReceiver<OutboxItem>>>,
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
}

/// One queued client datagram awaiting the writer task.
struct OutboxItem {
    header: PacketHeader,
    payload: Vec<u8>,
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
        }
    }
}

#[derive(Debug, Clone)]
pub enum ClientEvent {
    Connected { connection_id: u32, server_tick: u64, match_id: Uuid },
    Disconnected { reason: DisconnectReason },
    ConnectionFailed { reason: ConnectRejectReason },
    StateUpdate { snapshot: Snapshot },
    EntityUpdate { entity_id: EntityId, snapshot: EntitySnapshot },
    EntityRemoved { entity_id: EntityId },
    ProjectileUpdate { projectile_id: EntityId, snapshot: ProjectileSnapshot },
    ProjectileRemoved { projectile_id: EntityId },
    GameEvent { event: GameEvent },
    CommandAck { command_id: u64, success: bool, error: Option<String> },
    InputAck { tick: u32, accepted: bool },
    BandwidthStats { stats: BandwidthStats },
    RttUpdate { rtt: Duration },
    Error { error: String },
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
        let (outbox, outbox_rx) = mpsc::unbounded_channel();

        let client = Self {
            socket,
            config: config.clone(),
            server_addr: config.server_addr,
            connection: Arc::new(Mutex::new(None)),
            connection_id: Arc::new(Mutex::new(None)),
            connect_attempt: Arc::new(Mutex::new(None)),
            snapshot_buffer: Arc::new(SnapshotBuffer::new(config.snapshot_history)),
            snapshot_interpolator: Arc::new(Mutex::new(SnapshotInterpolator::new(config.snapshot_history))),
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
                        0, 0, 0, 0,
                    );
                    header.payload_size = len;
                    match bincode::serialize(&header) {
                        Ok(head) => {
                            let total = HEADER_SIZE + payload.len();
                            if total > MAX_PACKET_SIZE {
                                warn!("disconnect: {total} exceeds the MTU, not sending");
                            } else {
                                let mut buffer =
                                    BytesMut::with_capacity(total);
                                buffer.extend_from_slice(&head);
                                buffer.extend_from_slice(&payload);
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

        let handle = tokio::spawn(async move {
            while let Some(OutboxItem { header, payload }) = rx.recv().await {
                let out = OutgoingPacket { header, payload };
                if let Err(e) = Self::send_raw(&socket, &bandwidth, &limiter, limit_enabled, &out)
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
            warn!("outbox: payload {} exceeds the u16 header size", payload.len());
            return;
        };
        header.payload_size = len;
        if self.outbox.send(OutboxItem { header, payload }).is_err() {
            warn!("client outbox closed; dropping a queued packet");
        }
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
            0, 0, 0, 0,
        );

        let socket = self.socket.clone();
        let bandwidth = self.bandwidth_tracker.clone();
        let limiter = self.bandwidth_limiter.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        let attempt = self.connect_attempt.clone();
        let running = self.running.clone();
        let event_sender = self.event_sender.clone();
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
                    event_sender.send_critical(ClientEvent::Error {
                        error: "connect timeout: no ConnectAccept".into(),
                    }).await;
                    break;
                }
                let _ = Self::send_packet_static(&socket, &bandwidth, &limiter, limit_enabled, &header, &packet).await;
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
    ) -> anyhow::Result<()> {
        // Bug №81: a serialization failure must surface — not silently turn
        // into an empty payload the peer drops as invalid.
        let payload = bincode::serialize(packet)?;
        // The header length field is a u16: refuse to truncate instead of
        // lying about the size on the wire.
        let payload_len = u16::try_from(payload.len())
            .map_err(|_| anyhow::anyhow!("payload {} bytes exceeds u16 header", payload.len()))?;
        let total_size = HEADER_SIZE + payload.len();
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

        let mut buffer = BytesMut::with_capacity(total_size);
        buffer.extend_from_slice(&bincode::serialize(&header)?);
        buffer.extend_from_slice(&payload);

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
    ) -> anyhow::Result<()> {
        let total_size = out.total_size();
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

        let mut buffer = BytesMut::with_capacity(total_size);
        buffer.extend_from_slice(&bincode::serialize(&out.header)?);
        buffer.extend_from_slice(&out.payload);

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
        let connect_attempt = self.connect_attempt.clone();

        let handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];

            while *running.lock() {
                match socket.recv(&mut buf).await {
                    Ok(len) => {
                        let data = &buf[..len];
                        bandwidth_tracker.record_received(len);

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
                            &running,
                            &bandwidth_tracker,
                            &limiter,
                            limit_enabled,
                            &connect_attempt,
                            data,
                        ).await {
                            warn!("Error handling packet: {}", e);
                            event_sender.send_critical(ClientEvent::Error {
                                error: e.to_string(),
                            }).await;
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
        running: &Arc<Mutex<bool>>,
        bandwidth: &BandwidthTracker,
        // Bug №169: the HeartbeatAck answer leaves through the limiter too.
        limiter: &BandwidthLimiter,
        limit_enabled: bool,
        connect_attempt: &Arc<Mutex<Option<Instant>>>,
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
        {
            let guard = connection.lock();
            if let Some(conn) = guard.as_ref() {
                conn.record_remote(header.sequence, header.ack, header.ack_bitfield);
                conn.track_received_reliable(header.sequence, channel_from_u8(header.channel));
            }
        }

        let packet_type = match PacketType::from_u8(header.packet_type) {
            Some(packet_type) => packet_type,
            None => return Err(ConnectionError::InvalidPacketType(header.packet_type)),
        };

        match packet_type {
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
                        Connection::new(accept.assigned_client_id, config.server_addr, config.connection_config.clone())
                    });
                    conn.set_state(ConnectionState::Connected);
                    conn.set_client_id(Some(accept.assigned_client_id));
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

                if !already {
                    event_sender.send_critical(ClientEvent::Connected {
                        connection_id: accept.assigned_client_id,
                        server_tick: accept.server_tick,
                        match_id: accept.match_id,
                    }).await;
                }
            }
            PacketType::ConnectReject => {
                let reject: ConnectRejectPacket = deserialize_limited(payload)?;
                *connect_attempt.lock() = None;
                event_sender.send_critical(ClientEvent::ConnectionFailed { reason: reject.reason }).await;
            }
            PacketType::Disconnect => {
                let disconnect: DisconnectPacket = deserialize_limited(payload)?;
                *running.lock() = false;
                event_sender.send_critical(ClientEvent::Disconnected { reason: disconnect.reason }).await;
            }
            PacketType::Heartbeat => {
                let _hb: HeartbeatPacket = deserialize_limited(payload)?;
                // The server pings us to measure ITS RTT on our ack — no send
                // stamp exists here, so we measure nothing and just answer.
                // Bug №176: this used to go out with sequence 0 and an empty
                // bitfield, so the server's own ack bookkeeping learned
                // nothing from our answers. Bug №167: carry the real SACK
                // state, and a real sequence, like every other send site.
                let (ack, ack_bitfield, ack_seq) = {
                    let guard = connection.lock();
                    match guard.as_ref() {
                        Some(conn) => {
                            let (a, b) = conn.ack_info();
                            (a, b, conn.next_sequence())
                        }
                        None => (0, 0, 0),
                    }
                };
                let ack_header = PacketHeader::new(
                    PacketType::HeartbeatAck,
                    ChannelType::ReliableOrdered,
                    ack_seq, ack, ack_bitfield, 0,
                );
                let ack_packet = HeartbeatPacket { client_time: 0.0, server_time: 0.0 };
                let _ = Self::send_packet_static(socket, bandwidth, limiter, limit_enabled, &ack_header, &ack_packet).await;
            }
            PacketType::HeartbeatAck => {
                // Bug №12: measure RTT against the last heartbeat WE sent
                // (stamped in start_send_loop), never Instant::now().elapsed()
                // which is always ~0. No nested lock: conn and rtt are distinct.
                let sample = connection.lock().as_ref()
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
                    event_sender.send_critical(ClientEvent::RttUpdate { rtt: smoothed }).await;
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
                                &base_mismatch_drops,
                                &base_mismatch_drops_by_layer,
                                &stall_logged,
                                &stale_delta_drops,
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
                                &base_mismatch_drops,
                                &base_mismatch_drops_by_layer,
                                &stall_logged,
                                &stale_delta_drops,
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
                                event_sender.send_critical(ClientEvent::CommandAck {
                                    command_id: ack.command_id,
                                    success: ack.success,
                                    error: ack.error,
                                }).await;
                            }
                        }
                        PacketType::InputAck => {
                            if let Ok(ack) = deserialize_limited::<InputAckPacket>(&bytes) {
                                Self::apply_input_ack(&ack, pending_inputs, last_ack_tick, event_sender);
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
            PacketType::InputAck => {
                let ack: InputAckPacket = deserialize_limited(payload)?;
                Self::apply_input_ack(&ack, pending_inputs, last_ack_tick, event_sender);
            }
            PacketType::CommandAck => {
                let ack: CommandAckPacket = deserialize_limited(payload)?;
                event_sender.send_critical(ClientEvent::CommandAck {
                    command_id: ack.command_id,
                    success: ack.success,
                    error: ack.error,
                }).await;
            }
            _ => {
                debug!("Unhandled packet type: {:?}", packet_type);
            }
        }
        
        Ok(())
    }

    /// Bug №163: shared by the direct and the fragmented receive paths.
    fn dispatch_events(
        packet: &EventPacket,
        event_sender: &EventSink,
    ) {
        for event in &packet.events {
            event_sender.send_delta(ClientEvent::GameEvent { event: event.clone() });
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
        event_sender.send_delta(ClientEvent::InputAck { tick: ack.tick, accepted: ack.accepted });
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
    ) -> Result<(), ConnectionError> {
        if packet_type == PacketType::State || packet_type == PacketType::StateFull {
            // Bug №168: full states may be up to MAX_REASSEMBLED_SIZE.
            let state: StatePacket = deserialize_full_state(payload)?;
            // Bug №20: refuse absurd states before they balloon client memory.
            if state.entities.len() > MAX_ENTITIES_PER_SNAPSHOT
                || state.projectiles.len() > MAX_PROJECTILES_PER_SNAPSHOT {
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
                || delta.projectile_destroyed.len() > MAX_PROJECTILES_PER_SNAPSHOT {
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
                base_mismatch_drops_by_layer
                    .lock()
                    .unwrap()
                    .get_mut(layer)
                    .map(|c| *c += 1);
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
            let latest = match snapshot_buffer.get_latest() {
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
                let mut base = latest.clone();
                if delta.is_resync {
                    // Full layer replace: drop everything this layer previously
                    // held so entities destroyed while we were desynced do not
                    // linger as ghosts.
                    base.entities.retain(|e| entity_layer(e.entity_type) as usize != layer);
                    if layer == LAYER_PROJECTILE as usize {
                        base.projectiles.clear();
                    }
                }

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
                let changed: HashSet<EntityId> = delta.created
                    .iter()
                    .map(|e| e.entity_id)
                    .chain(delta.updated.iter().map(|u| u.entity_id))
                    .collect();
                let changed_projectiles: HashSet<EntityId> = delta.projectile_created
                    .iter()
                    .map(|p| p.entity_id)
                    .chain(delta.projectile_updated.iter().map(|u| u.entity_id))
                    .collect();

                if delta.is_resync {
                    let dead: Vec<EntityId> = {
                        let states = entity_states.read();
                        states.iter()
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
                    event_sender.send_delta(ClientEvent::EntityRemoved { entity_id: *entity_id });
                }

                {
                    let mut states = projectile_states.write();
                    if delta.is_resync && layer == LAYER_PROJECTILE as usize {
                        let dead: Vec<EntityId> = states.keys()
                            .copied()
                            .filter(|id| !target.projectiles.iter().any(|p| &p.entity_id == id))
                            .collect();
                        for id in dead {
                            states.remove(&id);
                            event_sender.send_delta(ClientEvent::ProjectileRemoved { projectile_id: id });
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
                    event_sender.send_delta(ClientEvent::ProjectileRemoved { projectile_id: *entity_id });
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
        // Bug №31: a degenerate tick_rate would panic on 1000/0 or busy-loop the
        // send task (interval 0 ms). Clamp to the sane 1..=1000 ticks/sec.
        let tick_rate = self.config.tick_rate.clamp(1, 1000);

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1000 / tick_rate as u64));
            
            while *running.lock() {
                interval.tick().await;
                
                let tick = {
                    let mut ct = current_tick.lock();
                    *ct = ct.next();
                    *ct
                };
                
                let (pending, heartbeat): (Option<(PacketHeader, InputPacket)>, Option<OutgoingPacket>) = {
                    let conn_guard = connection.lock();
                    match conn_guard.as_ref() {
                        Some(conn) if conn.state() == ConnectionState::Connected => {
                            // Bug №77: carry the server's sequence high-water
                            // mark so it can GC its reliable queue instead of
                            // pinning it at the window cap.
                            // Bug №167: it is the real SACK state, not the last
                            // sequence we happened to see.
                            let (ack, ack_bitfield) = conn.ack_info();

                            let input = Self::build_input_packet(tick);
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
                                ack, ack_bitfield, 0,
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
                                    ack, ack_bitfield, 0,
                                );
                                OutgoingPacket::new(
                                    hb_header,
                                    HeartbeatPacket { client_time: 0.0, server_time: 0.0 },
                                )
                            } else {
                                None
                            };

                            (Some((header, input)), heartbeat)
                        }
                        _ => (None, None),
                    }
                };

                if let Some((header, input)) = pending {
                    let _ = Self::send_packet_static(&socket, &bandwidth_tracker, &limiter, limit_enabled, &header, &input).await;
                }
                if let Some(hb) = heartbeat {
                    let _ = Self::send_raw(&socket, &bandwidth_tracker, &limiter, limit_enabled, &hb).await;
                }
            }
        });
        
        *self.send_handle.lock() = Some(handle);
    }

    fn build_input_packet(tick: Tick) -> InputPacket {
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
        }
    }

    pub fn send_input(&self, input: InputPacket) {
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
            ack, ack_bitfield, 0,
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

            let packet = CommandPacket { command_id, command };
            // Bug №77: ack the server's sequence high-water mark too.
            // Bug №167: real SACK state.
            let (ack, ack_bitfield) = conn.ack_info();
            let header = PacketHeader::new(
                PacketType::Command,
                ChannelType::ReliableOrdered,
                seq,
                ack, ack_bitfield, 0,
            );

            // Bug №172: queue instead of spawning a task per command.
            match bincode::serialize(&packet) {
                Ok(payload) => self.enqueue(header, payload),
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
        self.connection.lock().as_ref().map(|c| c.state() == ConnectionState::Connected).unwrap_or(false)
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
            assert!(!left.is_zero(), "{what}: timed out, dropped={}", client.events_dropped());
            match tokio::time::timeout(left, rx.recv()).await {
                Ok(Some(ev)) => {
                    if want(&ev) {
                        return ev;
                    }
                }
                Ok(None) => panic!("{what}: event channel closed, dropped={}", client.events_dropped()),
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
        assert_eq!(client.events_dropped(), 0, "nothing should be dropped on a clean connect");
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
                assert_eq!(reason, DisconnectReason::Kicked, "the reason must survive the wire");
            }
            other => panic!("expected Disconnected, got {other:?}"),
        }
        server.stop();
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
        assert_eq!(sink.try_send(ClientEvent::Error { error: "first".into() }), Disposition::Delivered);
        // The next has nowhere to go: transient backpressure.
        assert_eq!(sink.try_send(ClientEvent::Error { error: "second".into() }), Disposition::Full);
        assert!(!sink.channel_is_closed(), "a full queue is not a closed channel");
        assert_eq!(sink.closed.load(Ordering::Relaxed), 0);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);

        // Draining proves the consumer was alive all along.
        assert!(matches!(rx.recv().await, Some(ClientEvent::Error { .. })));
        assert_eq!(sink.try_send(ClientEvent::Error { error: "third".into() }), Disposition::Delivered);

        // Drop the consumer: now, and only now, it is terminal.
        drop(rx);
        assert_eq!(sink.try_send(ClientEvent::Error { error: "fourth".into() }), Disposition::Closed);
        assert!(sink.channel_is_closed(), "a dropped receiver must be reported as closed");
    }
}
