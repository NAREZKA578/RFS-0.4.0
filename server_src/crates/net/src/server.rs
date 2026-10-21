use crate::bandwidth::{Admission, BandwidthStats, BandwidthTracker};
use crate::connection::{
    is_reliable_channel, Connection, ConnectionConfig, ConnectionState, OutgoingPacket,
    ReliableArrival,
};
use crate::interest::InterestManager;
use crate::snapshot::{
    entity_layer, DeltaCompressor, EntitySnapshot, LayerBase, ProjectileSnapshot, Snapshot,
    SnapshotBuffer, LAYER_COUNT, LAYER_INTERVAL_TICKS, LAYER_RESYNC_TICKS,
};
use bytes::BytesMut;
use parking_lot::{Mutex, RwLock};
use rayon::prelude::*;
use rfs_core::entity::{PlayerEntity, ShipEntity};
use rfs_core::packet::*;
use rfs_core::spatial::InterestConfig;
use rfs_core::time::{Tick, TICK_DURATION, TICK_RATE};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Wire cap for a single bincode message (DoS: Vec len prefix could OOM).
const WIRE_LIMIT: u64 = 1024 * 1024;

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

pub struct NetServer {
    socket: Arc<UdpSocket>,
    config: ServerConfig,
    connections: Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
    connection_by_addr: Arc<RwLock<HashMap<SocketAddr, u32>>>,
    next_connection_id: Arc<Mutex<u32>>,
    snapshot_buffer: Arc<SnapshotBuffer>,
    /// Bug №266: how many snapshot rounds the state loop has run, and how many
    /// packets those rounds put on the wire.
    ///
    /// The simulation runs at `tick_rate` and the loop costs far less than its
    /// budget, yet clients only learn about ticks at a lower rate than the
    /// server numbers them. The only place that gap can come from is this
    /// publication step, and until it was counted the server had no way to say
    /// which half of it was a missed round and which half was a layer that is
    /// simply not sent on this tick.
    snapshot_rounds: Arc<AtomicU64>,
    snapshot_packets: Arc<AtomicU64>,
    /// Bug №274: deltas actually sent, per layer. The send-side half of the
    /// delivery measurement — the client's `applied_delta_count_by_layer` is
    /// the receive half, and a per-layer shortfall means different things on
    /// each side.
    sent_deltas_by_layer: Arc<[AtomicU64; LAYER_COUNT]>,
    delta_compressor: Arc<DeltaCompressor>,
    client_layers: Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
    interest_manager: Arc<InterestManager>,
    bandwidth_tracker: Arc<BandwidthTracker>,
    tick_rate: u32,
    /// The authoritative tick of the world, published by the simulation loop.
    ///
    /// Bug №268: this used to be a second, independent counter advanced by the
    /// send loop itself. Nothing tied it to the simulation, so every snapshot
    /// was stamped with a tick the world had not reached — and when the main
    /// loop overran its budget, `MissedTickBehavior::Skip` dropped simulation
    /// ticks silently while this counter kept climbing at a steady 30 Hz. The
    /// send loop now only reads it; `publish_tick` is the single writer.
    current_tick: Arc<Mutex<Tick>>,
    server_time: Arc<Mutex<f64>>,
    running: Arc<Mutex<bool>>,
    send_handle: Mutex<Option<JoinHandle<()>>>,
    recv_handle: Mutex<Option<JoinHandle<()>>>,
    /// Bug №613: both channels were `unbounded_channel`. An unbounded queue is
    /// an OOM waiting for a producer that outruns the consumer: every inbound
    /// datagram is turned into at least one event, and a client that floods can
    /// outpace the tick loop indefinitely. Both are bounded now, and the dropped
    /// counters say so out loud instead of the memory simply growing.
    event_sender: mpsc::Sender<ServerEvent>,
    event_receiver: Mutex<Option<mpsc::Receiver<ServerEvent>>>,
    match_id: Uuid,
    /// Per-address Connect token buckets (bug №27).
    connect_limits: Arc<Mutex<HashMap<SocketAddr, ConnectLimit>>>,
    /// Bug №172: single ordered outbox. Ack/event/disconnect packets used to
    /// spawn one task each, so the executor queue grew without bound and the
    /// UDP order between concurrently spawned tasks was never guaranteed.
    /// Everything now queues here and a single writer task drains it in order.
    outbox: mpsc::Sender<OutboxItem>,
    outbox_handle: Mutex<Option<JoinHandle<()>>>,
    /// Held until the server starts, then moved into the writer task.
    outbox_rx: Mutex<Option<mpsc::Receiver<OutboxItem>>>,
    /// Events dropped because the consumer fell behind (Bug №613).
    dropped_events: Arc<AtomicU64>,
    /// Outbox entries dropped for the same reason (Bug №613).
    dropped_outbox: Arc<AtomicU64>,
    /// Bug №699: the world's static description, sent to every client when it
    /// connects. The layers carry ships, players, stations, compartments and
    /// projectiles — and no structure, so a client that never heard of the
    /// world could not draw it and could not explain a shell vanishing at an
    /// invisible rock.
    world_description: Arc<RwLock<Option<rfs_core::world::MapDescription>>>,
    /// Bug №216: the secret a client has to prove it with.
    ///
    /// `None` means the server asks for no secret — every client is
    /// authenticated by default, and the server says so in its log.
    auth_token: Arc<RwLock<Option<String>>>,
}

/// One queued datagram awaiting the writer task.
struct OutboxItem {
    conn: Arc<Connection>,
    packet: OutgoingPacket,
}

/// Bug №613: the event queue is bounded, so "the channel is full" is a real
/// outcome and not a hypothetical. Every send therefore goes through here: a
/// full queue drops the event and says so, instead of `let _ = …send()` which
/// made a dropped event look exactly like a delivered one. A closed channel is
/// not counted — nothing is waiting to read it any more, so the number would
/// only ever grow for a server that is already going down.
fn emit_event(tx: &mpsc::Sender<ServerEvent>, dropped: &AtomicU64, event: ServerEvent) {
    match tx.try_send(event) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            dropped.fetch_add(1, Ordering::Relaxed);
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {}
    }
}

/// Bug №267 (A): does this layer's pass have to be a full resync?
///
/// Two independent reasons produce one: the scheduled cadence (bug №173's
/// healing path — the client holds a base, so a full layer has to be sent
/// when the content is gone) and an explicit client request, which bypasses
/// the cadence because the client's base mismatch is a live stall.
///
/// Extracted from the snapshot pass so the decision is testable on its own:
/// the pass is one of the largest functions in the server, and a rule buried
/// in it is a rule nobody can check.
fn layer_needs_resync(
    layer: usize,
    since: u64,
    content_empty: bool,
    base_empty: bool,
    requested: bool,
) -> bool {
    if requested {
        return true;
    }
    since >= LAYER_RESYNC_TICKS[layer] && (!content_empty || !base_empty)
}

/// Bug №267 (A): apply an incoming resync request to the connection.
///
/// The layer index arrives from the wire, so it is bounds-checked here rather
/// than trusted. A malformed request must not be able to reach the layer
/// arrays.
fn apply_resync_request(conn: &Connection, payload: &[u8]) -> bool {
    match deserialize_limited::<ResyncRequestPacket>(payload) {
        Ok(request) => {
            if (request.layer as usize) < LAYER_COUNT {
                conn.request_resync(request.layer);
                true
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Token bucket for inbound Connects from one source address (bug №27).
/// Burst 4, refill 4/sec: a legitimate client (1 Connect + 250 ms retries
/// while the Accept is in flight) never notices; a sprayer is capped.
const CONNECT_BURST: f64 = 4.0;
const CONNECT_REFILL_PER_SEC: f64 = 4.0;
/// Upper bound for the limiter table itself; oldest entries are evicted past it.
const CONNECT_TABLE_CAP: usize = 4096;

/// Bug №171: how long a reliable packet may wait for bandwidth tokens before
/// the send path gives up and returns an error instead of silently discarding
/// a message that has no retransmit behind it. Bounded so one slow client
/// cannot stall the whole send loop.
const RELIABLE_ADMIT_BUDGET: Duration = Duration::from_millis(250);

/// Bug №219: how long a Connected connection may go without its game-layer
/// player being registered. The snapshot gate (send_snapshots, №164) holds
/// every byte back until `interest_manager.has_player` is true, so an
/// unregistered player is a client that receives nothing — and, without this
/// reaper, waits forever on a silent socket. Registration in the happy path
/// happens in milliseconds (a spawn on ClientConnected), so ten seconds is
/// far beyond any legitimate stall and short enough to notice a wiring hole
/// "forgot to add the player" during one run, not a session later.
const PLAYER_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct ConnectLimit {
    tokens: f64,
    last: Instant,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub max_connections: usize,
    pub connection_config: ConnectionConfig,
    pub interest_config: InterestConfig,
    pub snapshot_history: usize,
    pub tick_rate: u32,
    pub max_bandwidth_bps: f64,
    pub enable_bandwidth_limit: bool,
    /// Server-side rate tracker window. Passed down to every connection's
    /// own tracker (bug №671) — before that, `with_window` had no caller at
    /// all and the trackers carried a hardcoded second.
    pub bandwidth_window: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:7777".parse().unwrap(),
            max_connections: 256,
            connection_config: ConnectionConfig::default(),
            interest_config: InterestConfig::default(),
            snapshot_history: 128,
            tick_rate: TICK_RATE,
            max_bandwidth_bps: 1024.0 * 1024.0,
            enable_bandwidth_limit: false,
            bandwidth_window: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ServerEvent {
    ClientConnected {
        connection_id: u32,
        addr: SocketAddr,
    },
    ClientDisconnected {
        connection_id: u32,
        reason: DisconnectReason,
    },
    ClientTimeout {
        connection_id: u32,
    },
    ClientInput {
        connection_id: u32,
        input: InputPacket,
    },
    ClientCommand {
        connection_id: u32,
        command: CommandPacket,
    },
    PacketReceived {
        connection_id: u32,
        packet_type: PacketType,
    },
    PacketSent {
        connection_id: u32,
        packet_type: PacketType,
        bytes: usize,
    },
    BandwidthStats {
        connection_id: u32,
        stats: BandwidthStats,
    },
    Error {
        connection_id: Option<u32>,
        error: String,
    },
}

impl NetServer {
    pub async fn new(config: ServerConfig) -> Result<Self, std::io::Error> {
        let socket = Arc::new(UdpSocket::bind(config.bind_addr).await?);
        // Bug №613: bounded, with a measured cap. 4096 events is far more than a
        // healthy tick loop ever has in flight, and small enough that a flood is
        // dropped rather than buffered until the process dies.
        const EVENT_QUEUE_LIMIT: usize = 4096;
        const OUTBOX_QUEUE_LIMIT: usize = 8192;
        let (event_sender, event_receiver) = mpsc::channel(EVENT_QUEUE_LIMIT);
        // Bug №172: the outbox and the channel holding its receiving end. The
        // receiver is moved into the writer task when the server starts.
        let (outbox, outbox_rx) = mpsc::channel(OUTBOX_QUEUE_LIMIT);
        let match_id = Uuid::new_v4();

        let server = Self {
            socket,
            config: config.clone(),
            connections: Arc::new(RwLock::new(HashMap::new())),
            connection_by_addr: Arc::new(RwLock::new(HashMap::new())),
            next_connection_id: Arc::new(Mutex::new(1)),
            snapshot_buffer: Arc::new(SnapshotBuffer::new(config.snapshot_history)),
            snapshot_rounds: Arc::new(AtomicU64::new(0)),
            snapshot_packets: Arc::new(AtomicU64::new(0)),
            // Bug №274: per-layer send counter, see the field.
            sent_deltas_by_layer: Arc::new(std::array::from_fn(|_| AtomicU64::new(0))),
            delta_compressor: Arc::new(DeltaCompressor::new()),
            client_layers: Arc::new(RwLock::new(HashMap::new())),
            interest_manager: Arc::new(InterestManager::new(config.interest_config)),
            bandwidth_tracker: Arc::new(BandwidthTracker::with_window(config.bandwidth_window)),
            // Bug №31: tick_rate 0 panics on 1000/0 and >1000 busy-loops the
            // send task. Clamp to the sane interval of 1..=1000 ticks/sec.
            tick_rate: config.tick_rate.clamp(1, 1000),
            current_tick: Arc::new(Mutex::new(Tick(0))),
            server_time: Arc::new(Mutex::new(0.0)),
            running: Arc::new(Mutex::new(false)),
            send_handle: Mutex::new(None),
            recv_handle: Mutex::new(None),
            event_sender,
            event_receiver: Mutex::new(Some(event_receiver)),
            match_id,
            connect_limits: Arc::new(Mutex::new(HashMap::new())),
            outbox,
            outbox_handle: Mutex::new(None),
            outbox_rx: Mutex::new(Some(outbox_rx)),
            dropped_events: Arc::new(AtomicU64::new(0)),
            dropped_outbox: Arc::new(AtomicU64::new(0)),
            world_description: Arc::new(RwLock::new(None)),
            auth_token: Arc::new(RwLock::new(None)),
        };

        Ok(server)
    }

    pub fn match_id(&self) -> Uuid {
        self.match_id
    }

    /// Bug №699: the world this server runs, as the client is told about it.
    ///
    /// The server loads the map in the match binary; the network layer keeps
    /// the DESCRIPTION of it so it can hand it to every client that connects.
    pub fn set_world_description(&self, description: rfs_core::world::MapDescription) {
        *self.world_description.write() = Some(description);
    }

    /// The world description, if one was set.
    pub fn world_description(&self) -> Option<rfs_core::world::MapDescription> {
        self.world_description.read().clone()
    }

    /// Bug №216: the secret a client has to present.
    ///
    /// `None` (or an empty secret) means the server asks for none: every
    /// client is authenticated by default, and the caller is expected to say
    /// so out loud — a server that looks protected and is not is worse than
    /// one that says it is open.
    pub fn set_auth_token(&self, token: Option<String>) {
        *self.auth_token.write() = token.filter(|t| !t.is_empty());
    }

    pub fn current_tick(&self) -> Tick {
        *self.current_tick.lock()
    }

    /// Publish the tick the simulation has just finished (bug №268).
    ///
    /// Called once per simulation iteration, after that tick's state has been
    /// written into the interest manager, so a snapshot stamped with this tick
    /// is built from state that is at least this tick. The send loop is the
    /// only reader and stamps every packet with what it finds here.
    ///
    /// Monotonic on purpose: a caller that reports an older tick is ignored
    /// rather than allowed to rewind the world for every connected client.
    pub fn publish_tick(&self, tick: Tick) {
        let mut current = self.current_tick.lock();
        if tick > *current {
            *current = tick;
            *self.server_time.lock() = tick.value() as f64 * TICK_DURATION.as_secs_f64();
        }
    }

    pub fn server_time(&self) -> f64 {
        *self.server_time.lock()
    }

    pub fn connection_count(&self) -> usize {
        self.connections.read().len()
    }

    pub fn get_connection(&self, connection_id: u32) -> Option<Arc<Connection>> {
        self.connections.read().get(&connection_id).cloned()
    }

    pub fn get_connection_by_addr(&self, addr: SocketAddr) -> Option<Arc<Connection>> {
        // Bug №79: never hold the addr map and the connection map at once.
        // remove_connection takes them in the opposite order — nested locks
        // here are an ABBA deadlock (parking_lot is not reentrant).
        let id = self.connection_by_addr.read().get(&addr).copied()?;
        self.connections.read().get(&id).cloned()
    }

    pub fn start(&self) {
        *self.running.lock() = true;
        self.start_outbox_writer();
        self.start_receive_loop();
        self.start_send_loop();
        info!("NetServer started on {}", self.config.bind_addr);
    }

    pub fn stop(&self) {
        // Bug №610: the farewells have to be queued BEFORE the task that sends
        // them is aborted. The outbox has exactly one writer, and it was
        // aborted first, so every `Disconnect(ServerShutdown)` that
        // `disconnect_all` queued afterwards went into a channel with nobody
        // left reading it — the shutdown notice was dropped on the floor and
        // clients learned about the match ending from a silent socket.
        self.disconnect_all(DisconnectReason::ServerShutdown);
        *self.running.lock() = false;
        self.abort_tasks();
        info!("NetServer stopped");
    }

    /// `stop()`, but the outbox gets a bounded window to deliver the farewells.
    ///
    /// Bug №610: ordering alone is not enough — the farewell is queued, then the
    /// only task that can send it is aborted in the same breath. On a real
    /// socket that is a lost packet, which is exactly the case this path exists
    /// for. The pause is the drain window; it is short and it is paid once, at
    /// the end of a match.
    pub async fn stop_gracefully(&self) {
        const FLUSH_WINDOW: std::time::Duration = std::time::Duration::from_millis(100);
        self.disconnect_all(DisconnectReason::ServerShutdown);
        *self.running.lock() = false;
        tokio::time::sleep(FLUSH_WINDOW).await;
        self.abort_tasks();
        info!("NetServer stopped");
    }

    fn abort_tasks(&self) {
        if let Some(handle) = self.send_handle.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.recv_handle.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.outbox_handle.lock().take() {
            handle.abort();
        }
    }

    /// Bug №172: the single writer for every queued datagram.
    ///
    /// A FIFO channel plus one draining task replaces one spawned task per
    /// ack/event/disconnect. The executor queue no longer grows with traffic,
    /// and datagrams leave in the order they were queued — which the old
    /// racing tasks did not guarantee.
    fn start_outbox_writer(&self) {
        let Some(mut rx) = self.outbox_rx.lock().take() else {
            return;
        };
        let socket = self.socket.clone();
        let bandwidth = self.bandwidth_tracker.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;

        let handle = tokio::spawn(async move {
            while let Some(item) = rx.recv().await {
                let OutboxItem { conn, packet } = item;
                if let Err(e) =
                    Self::send_packet(&socket, &conn, &packet, &bandwidth, limit_enabled).await
                {
                    // An oversize or unadmittable packet is a per-packet
                    // problem; the connection keeps running.
                    warn!("outbox send failed for connection {}: {e}", conn.id);
                }
            }
        });
        *self.outbox_handle.lock() = Some(handle);
    }

    /// Bug №172: queue instead of spawning. Returns false only if the server
    /// has been dropped, which is the one case where a send is truly lost.
    ///
    /// Bug №613: the channel is bounded now, so `send` can also fail because it
    /// is FULL — a consumer that fell behind. That is a dropped packet, and it
    /// is counted: a silently dropped farewell or ack is the kind of failure
    /// that looks like a network problem for an hour.
    fn enqueue(&self, conn: Arc<Connection>, packets: Vec<OutgoingPacket>) {
        for packet in packets {
            match self.outbox.try_send(OutboxItem {
                conn: conn.clone(),
                packet,
            }) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    self.dropped_outbox.fetch_add(1, Ordering::Relaxed);
                    warn!("outbox full; dropping a packet for connection {}", conn.id);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    warn!("outbox closed; dropping packet for connection {}", conn.id);
                    return;
                }
            }
        }
    }

    fn start_receive_loop(&self) {
        let socket = self.socket.clone();
        let connections = self.connections.clone();
        let connection_by_addr = self.connection_by_addr.clone();
        let next_id = self.next_connection_id.clone();
        let config = self.config.clone();
        let bandwidth_tracker = self.bandwidth_tracker.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        let connect_limits = self.connect_limits.clone();
        let running = self.running.clone();
        let event_sender = self.event_sender.clone();
        // Bug №613: the drop counter travels with the sender into the task, or a
        // bounded queue would drop events with nothing left to report it.
        let dropped_events = self.dropped_events.clone();
        let current_tick = self.current_tick.clone();
        let server_time = self.server_time.clone();
        let match_id = self.match_id;
        let delta_compressor = self.delta_compressor.clone();
        let client_layers = self.client_layers.clone();
        // Bug №699: the world description travels into the task by value —
        // the task outlives this borrow.
        let world_description = self.world_description.clone();
        // Bug №216: the secret, the same way.
        let auth_token = self.auth_token.clone();

        let handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];

            while *running.lock() {
                match socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        let data = &buf[..len];
                        bandwidth_tracker.record_received(len);

                        if let Err(e) = Self::handle_received_packet(
                            &socket,
                            &connections,
                            &connection_by_addr,
                            &next_id,
                            &config,
                            &bandwidth_tracker,
                            limit_enabled,
                            &connect_limits,
                            &event_sender,
                            &dropped_events,
                            &current_tick,
                            &server_time,
                            match_id,
                            &delta_compressor,
                            &client_layers,
                            // Bug №699: the world description.
                            &world_description,
                            // Bug №216: the secret.
                            &auth_token,
                            addr,
                            data,
                        )
                        .await
                        {
                            warn!("Error handling packet from {}: {}", addr, e);
                            emit_event(
                                &event_sender,
                                &dropped_events,
                                ServerEvent::Error {
                                    connection_id: None,
                                    error: e.to_string(),
                                },
                            );
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

    /// Token-bucket admission for inbound Connects (bug №27).
    /// Returns true if this Connect may proceed, false if the sender is over rate.
    fn check_connect_rate(
        limits: &Arc<Mutex<HashMap<SocketAddr, ConnectLimit>>>,
        addr: SocketAddr,
    ) -> bool {
        let mut limits = limits.lock();
        let now = Instant::now();
        // Bound the table itself: evict entries idle > 60 s when over cap.
        if limits.len() > CONNECT_TABLE_CAP {
            limits.retain(|_, e| now.duration_since(e.last) < Duration::from_secs(60));
        }
        let entry = limits.entry(addr).or_insert(ConnectLimit {
            tokens: CONNECT_BURST,
            last: now,
        });
        entry.tokens = (entry.tokens
            + now.duration_since(entry.last).as_secs_f64() * CONNECT_REFILL_PER_SEC)
            .min(CONNECT_BURST);
        entry.last = now;
        if entry.tokens >= 1.0 {
            entry.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    // The per-connection shared state is passed explicitly rather than
    // bundled into a context struct, to keep the borrow scopes of the
    // individual locks narrow and obvious at each use site.
    #[allow(clippy::too_many_arguments)]
    async fn handle_received_packet(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        connection_by_addr: &Arc<RwLock<HashMap<SocketAddr, u32>>>,
        next_id: &Arc<Mutex<u32>>,
        config: &ServerConfig,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
        connect_limits: &Arc<Mutex<HashMap<SocketAddr, ConnectLimit>>>,
        event_sender: &mpsc::Sender<ServerEvent>,
        // Bug №613: the bounded event queue drops what it cannot hold, and
        // this is where that drop is counted.
        dropped_events: &AtomicU64,
        current_tick: &Arc<Mutex<Tick>>,
        server_time: &Arc<Mutex<f64>>,
        match_id: Uuid,
        delta_compressor: &Arc<DeltaCompressor>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        // Bug №699: the world description, handed to every client on connect.
        world_description: &Arc<RwLock<Option<rfs_core::world::MapDescription>>>,
        // Bug №216: the secret a client has to present.
        auth_token: &Arc<RwLock<Option<String>>>,
        addr: SocketAddr,
        data: &[u8],
    ) -> anyhow::Result<()> {
        if data.len() < HEADER_SIZE {
            return Err(anyhow::anyhow!(
                "Packet too small: {} < {}",
                data.len(),
                HEADER_SIZE
            ));
        }

        let (header, payload) = deserialize_packet(data)?;

        // Bug №176: the header carries a version, and a Connect carries
        // `protocol_version`, but neither was ever checked. A mismatched
        // client is admitted, and because the packet enums are positional the
        // two sides then disagree about what every subsequent field means.
        // Reject before any state is allocated.
        if header.version != PROTOCOL_VERSION {
            warn!(
                "rejecting packet from {addr}: header version {} != {PROTOCOL_VERSION}",
                header.version
            );
            return Ok(());
        }

        let connection_id = {
            let existing = connection_by_addr.read().get(&addr).copied();
            if let Some(id) = existing {
                id
            } else {
                // Bug №27: only a Connect opens a connection. Any other packet
                // from an unknown address is stray traffic (late duplicate,
                // scanner, spoof) — it must not allocate state or spawn a player.
                if PacketType::from_u8(header.packet_type) != Some(PacketType::Connect) {
                    return Ok(());
                }
                // ...and Connects themselves are rate-limited per address, so
                // one sender cannot spray registrations at full line rate.
                if !Self::check_connect_rate(connect_limits, addr) {
                    return Ok(());
                }

                // Bug №176: check the build before allocating a slot. A
                // mismatched client must not consume a connection id, and must
                // not get as far as ClientConnected.
                match deserialize_limited::<ConnectPacket>(payload) {
                    Ok(connect) if connect.protocol_version != PROTOCOL_VERSION => {
                        warn!(
                            "rejecting Connect from {addr}: protocol {} != {PROTOCOL_VERSION}",
                            connect.protocol_version
                        );
                        let reject = ConnectRejectPacket {
                            reason: ConnectRejectReason::VersionMismatch,
                        };
                        let header = PacketHeader::new(
                            PacketType::ConnectReject,
                            ChannelType::ReliableOrdered,
                            0,
                            0,
                            0,
                            0,
                        );
                        let packet_data = serialize_packet(&reject, header)?;
                        socket.send_to(&packet_data, addr).await?;
                        bandwidth.record_sent(packet_data.len());
                        return Ok(());
                    }
                    Ok(_) => {}
                    Err(e) => {
                        // An undecodable Connect is malformed, not a version
                        // problem — drop it silently rather than allocating.
                        warn!("rejecting malformed Connect from {addr}: {e}");
                        return Ok(());
                    }
                }

                // Bug №27: the cap check and the registration run under one
                // write lock, so a concurrent Connect cannot slip between the
                // len check and the insert (TOCTOU in the old read-then-write).
                let admitted: Option<(u32, Arc<Connection>)> = {
                    let mut conns = connections.write();
                    if conns.len() >= config.max_connections {
                        None
                    } else {
                        let id = {
                            let mut id_gen = next_id.lock();
                            let id = *id_gen;
                            *id_gen = id_gen.wrapping_add(1);
                            if *id_gen == 0 {
                                *id_gen = 1;
                            }
                            id
                        };

                        // Bug №176: hand each connection its own send budget
                        // instead of sharing one server-wide bucket.
                        let conn_config = ConnectionConfig {
                            max_bandwidth_bps: config.max_bandwidth_bps,
                            bandwidth_window: config.bandwidth_window,
                            ..config.connection_config.clone()
                        };
                        let conn = Connection::new(id, addr, conn_config);
                        conn.set_state(ConnectionState::Connected);
                        let conn_arc = Arc::new(conn);
                        conns.insert(id, conn_arc.clone());
                        connection_by_addr.write().insert(addr, id);
                        Some((id, conn_arc))
                    }
                };

                let Some((id, conn_arc)) = admitted else {
                    let reject = ConnectRejectPacket {
                        reason: ConnectRejectReason::ServerFull,
                    };
                    let header = PacketHeader::new(
                        PacketType::ConnectReject,
                        ChannelType::ReliableOrdered,
                        0,
                        0,
                        0,
                        0,
                    );
                    let packet_data = serialize_packet(&reject, header)?;
                    socket.send_to(&packet_data, addr).await?;
                    bandwidth.record_sent(packet_data.len());
                    return Ok(());
                };

                let accept = {
                    let tick = current_tick.lock().value();
                    let time = *server_time.lock();
                    conn_arc.send_connect_accept(tick, time, match_id)
                };

                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::ClientConnected {
                        connection_id: id,
                        addr,
                    },
                );

                if let Some(accept) = accept {
                    Self::send_packet(socket, &conn_arc, &accept, bandwidth, limit_enabled)
                        .await
                        .map(|_| ())?;
                }

                id
            }
        };

        let conn = connections.read().get(&connection_id).cloned();

        if let Some(conn) = conn {
            // Bug №215: the outcome carries both the reply packets and the
            // reliable-arrival verdict the dispatch below needs.
            let outcome = conn.handle_packet(header, payload)?;

            for packet in outcome.outgoing {
                Self::send_packet(socket, &conn, &packet, bandwidth, limit_enabled)
                    .await
                    .map(|_| ())?;
            }

            match PacketType::from_u8(header.packet_type) {
                Some(PacketType::Input) => {
                    if let Ok(input) = deserialize_limited::<InputPacket>(payload) {
                        emit_event(
                            event_sender,
                            dropped_events,
                            ServerEvent::ClientInput {
                                connection_id,
                                input,
                            },
                        );
                    }
                }
                Some(PacketType::Command) => {
                    // Bug №215: only a genuinely NEW reliable datagram applies.
                    // A retransmitted Command arrives because our answer ack
                    // was lost — executing it twice would be manifestly worse
                    // than the answer's delay, which the ledger (№215) now
                    // bounds instead of forever.
                    if outcome.reliable_arrival != Some(ReliableArrival::New) {
                        return Ok(());
                    }
                    if let Ok(command) = deserialize_limited::<CommandPacket>(payload) {
                        emit_event(
                            event_sender,
                            dropped_events,
                            ServerEvent::ClientCommand {
                                connection_id,
                                command,
                            },
                        );
                    }
                }
                Some(PacketType::Connect) => {
                    // Duplicate Connect: our Accept never made it back.
                    // Registration is keyed by addr, so re-sending is idempotent.
                    // Bug №176: a repeat Connect must still be version-checked —
                    // it is a fresh ConnectPacket, and accepting a mismatched
                    // build here would admit it after the first pass.
                    match deserialize_limited::<ConnectPacket>(payload) {
                        Ok(connect) if connect.protocol_version != PROTOCOL_VERSION => {
                            let reject = ConnectRejectPacket {
                                reason: ConnectRejectReason::VersionMismatch,
                            };
                            let header = PacketHeader::new(
                                PacketType::ConnectReject,
                                ChannelType::ReliableOrdered,
                                0,
                                0,
                                0,
                                0,
                            );
                            let packet_data = serialize_packet(&reject, header)?;
                            socket.send_to(&packet_data, addr).await?;
                            bandwidth.record_sent(packet_data.len());
                            return Ok(());
                        }
                        _ => {}
                    }
                    let accept = {
                        let tick = current_tick.lock().value();
                        let time = *server_time.lock();
                        conn.send_connect_accept(tick, time, match_id)
                    };
                    if let Some(accept) = accept {
                        Self::send_packet(socket, &conn, &accept, bandwidth, limit_enabled)
                            .await
                            .map(|_| ())?;
                    }
                    // Bug №216: when the match asks for a secret, the world is
                    // held back until the client proves itself. The accept
                    // still goes out — the client has to know the server is
                    // there before it can answer.
                    let secret = auth_token.read().clone();
                    if secret.is_some() {
                        conn.set_authenticated(false);
                    } else {
                        // Bug №699: the world, right after the accept and before
                        // any snapshot. The client cannot draw a world it was
                        // never told about, and a snapshot that arrives first
                        // would put entities in a void the client has no
                        // description of.
                        let description = world_description.read().clone();
                        if let Some(description) = description {
                            let packets = conn.send_map_description(&description);
                            for packet in packets {
                                Self::send_packet(socket, &conn, &packet, bandwidth, limit_enabled)
                                    .await
                                    .map(|_| ())?;
                            }
                        }
                    }
                }
                Some(PacketType::Auth) => {
                    // Bug №216: the client's proof. A wrong secret ends the
                    // session by name; a right one opens the world.
                    let secret = auth_token.read().clone();
                    let Some(expected) = secret else {
                        // The server asks for no secret: nothing to prove.
                        return Ok(());
                    };
                    let offered = deserialize_limited::<AuthPacket>(payload)
                        .map(|auth| auth.token)
                        .unwrap_or_default();
                    if offered == expected {
                        conn.set_authenticated(true);
                        // Bug №699: the world, now that the client may have it.
                        let description = world_description.read().clone();
                        if let Some(description) = description {
                            let packets = conn.send_map_description(&description);
                            for packet in packets {
                                Self::send_packet(socket, &conn, &packet, bandwidth, limit_enabled)
                                    .await
                                    .map(|_| ())?;
                            }
                        }
                    } else {
                        warn!(
                            "rejecting {addr}: wrong secret ({} bytes offered)",
                            offered.len()
                        );
                        let reason = DisconnectReason::Unauthorized;
                        let header = PacketHeader::new(
                            PacketType::Disconnect,
                            ChannelType::ReliableOrdered,
                            0,
                            0,
                            0,
                            0,
                        );
                        let packet = DisconnectPacket { reason };
                        if let Ok(bytes) = serialize_packet(&packet, header) {
                            let _ = socket.send_to(&bytes, addr).await;
                            bandwidth.record_sent(bytes.len());
                        }
                        Self::remove_connection(
                            connection_by_addr,
                            connections,
                            delta_compressor,
                            client_layers,
                            connection_id,
                            addr,
                        );
                    }
                }
                Some(PacketType::Fragment) => {
                    if let Some((orig_type, bytes)) = conn.reassemble_fragment(payload) {
                        match orig_type {
                            PacketType::Input => {
                                if let Ok(input) = deserialize_limited::<InputPacket>(&bytes) {
                                    emit_event(
                                        event_sender,
                                        dropped_events,
                                        ServerEvent::ClientInput {
                                            connection_id,
                                            input,
                                        },
                                    );
                                }
                            }
                            PacketType::Command => {
                                if let Ok(command) = deserialize_limited::<CommandPacket>(&bytes) {
                                    emit_event(
                                        event_sender,
                                        dropped_events,
                                        ServerEvent::ClientCommand {
                                            connection_id,
                                            command,
                                        },
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some(PacketType::ResyncRequest) => {
                    // Bug №267 (A): a stalled layer asks for an immediate
                    // resync. One bit per connection is enough; the snapshot
                    // pass consumes it on the next tick.
                    apply_resync_request(&conn, payload);
                }
                Some(PacketType::Disconnect) => {
                    // Bug №68: a client Disconnect used to fall into `_ => {}` —
                    // only `last_received` was refreshed (extending the timeout!)
                    // and the player slot stayed occupied until the 10 s sweep.
                    // Remove the connection right here, idempotently.
                    let reason = deserialize_limited::<DisconnectPacket>(payload)
                        .map(|d| d.reason)
                        .unwrap_or(DisconnectReason::ClientQuit);
                    if Self::remove_connection(
                        connection_by_addr,
                        connections,
                        delta_compressor,
                        client_layers,
                        connection_id,
                        addr,
                    ) {
                        emit_event(
                            event_sender,
                            dropped_events,
                            ServerEvent::ClientDisconnected {
                                connection_id,
                                reason,
                            },
                        );
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn send_packet(
        socket: &Arc<UdpSocket>,
        conn: &Connection,
        packet: &OutgoingPacket,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
    ) -> anyhow::Result<bool> {
        let total_size = HEADER_SIZE + packet.payload.len();
        // Bug №30: the MTU comes from the connection's own config — the same
        // value the packet builders fragment against — not a stray constant.
        let max = conn.config.max_packet_size;
        if total_size > max {
            return Err(anyhow::anyhow!(
                "Packet too large: {} > {}",
                total_size,
                max
            ));
        }
        // Bug №39: the limiter gates the wire.
        // Bug №171: dropping is only acceptable for unreliable traffic, whose
        // next update supersedes it. A reliable packet with no retransmit
        // queue behind it (the Channel layer is still unwired) would be lost
        // permanently, so it waits for tokens up to a bounded budget and
        // reports an error if even that is not enough.
        if limit_enabled {
            let reliable = is_reliable_channel(packet.header.channel);
            let mut budget = RELIABLE_ADMIT_BUDGET;
            loop {
                match conn.bandwidth_limiter().admit(total_size, reliable) {
                    Admission::Send => break,
                    Admission::Drop => return Ok(false),
                    Admission::Retry(wait) => {
                        if wait >= budget {
                            return Err(anyhow::anyhow!(
                                "limiter: reliable {} packet ({total_size} B) not admitted \
                                 within {RELIABLE_ADMIT_BUDGET:?}",
                                packet.header.channel
                            ));
                        }
                        tokio::time::sleep(wait).await;
                        budget -= wait;
                    }
                }
            }
        }

        let mut buffer = BytesMut::with_capacity(total_size);
        buffer.extend_from_slice(&bincode::serialize(&packet.header)?);
        buffer.extend_from_slice(&packet.payload);

        socket.send_to(&buffer, conn.addr).await?;
        bandwidth.record_sent(total_size);
        Ok(true)
    }

    fn start_send_loop(&self) {
        let socket = self.socket.clone();
        let connections = self.connections.clone();
        let connection_by_addr = self.connection_by_addr.clone();
        let client_layers = self.client_layers.clone();
        let snapshot_buffer = self.snapshot_buffer.clone();
        // Bug №266: counted so the publication rate is measurable, not assumed.
        let snapshot_rounds = self.snapshot_rounds.clone();
        let snapshot_packets = self.snapshot_packets.clone();
        // Bug №274: shared per-layer send counter.
        let sent_deltas_by_layer = self.sent_deltas_by_layer.clone();
        let delta_compressor = self.delta_compressor.clone();
        let interest_manager = self.interest_manager.clone();
        let bandwidth_tracker = self.bandwidth_tracker.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        let event_sender = self.event_sender.clone();
        let running = self.running.clone();
        // Bug №613: as in the receive loop, the drop counter goes with the
        // sender into the task.
        let dropped_events = self.dropped_events.clone();
        let current_tick = self.current_tick.clone();
        let server_time = self.server_time.clone();
        let tick_rate = self.tick_rate;

        let handle = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_millis(1000 / tick_rate as u64));
            // Bug №268: this loop no longer owns a tick counter. It reads the
            // tick the simulation published and stamps every packet with it, so
            // a client is never told the world advanced past the state it was
            // actually built from.
            //
            // Snapshot work is skipped when the simulation has not reached a
            // new tick. That is the honest behaviour and it is also the cheap
            // one: the world did not move, so there is nothing new to say, and
            // re-sending the same tick would burn the tick budget saying so.
            // The wall-clock cadence is unchanged, so per-layer rates measured
            // in ticks (`LAYER_INTERVAL_TICKS`) still hold: when the simulation
            // keeps up, consecutive rounds differ by one tick and layer 0 still
            // goes out every second tick; when it falls behind, every round is
            // a fresh tick and the layers publish at whatever the simulation
            // actually achieved. Heartbeats and timeout reaping are NOT part of
            // that skip — see the notes at their call sites.
            //
            // Bug #267 follow-up (REVERTED): setting MissedTickBehavior::Delay
            // here to clamp runaway publish rounds regressed the healthy 50-bot
            // case from 18.5 to 0.82 Hz applied — the multi-round behaviour
            // (rounds_per_tick 4-5.5 under flood) is a symptom of the connect
            // storm, not its cause; throttling it starves clients even when the
            // loop can keep up. Left at the default Burst. Skipping unchanged
            // ticks is not that throttle: it removes rounds that carried no new
            // state in the first place. The real cost during the flood is
            // sim+sync 126-144ms with only 13-73 players registered: connect
            // handling / interest / event drain, not network serialization.
            // Queue: make Connect admission and the first-full per client cheap
            // and staggered so 200 joins do not dominate the tick budget.
            let mut last_published: Option<Tick> = None;
            // Timeout reaping is wall-clock work, not simulation work: it must
            // keep running at ~1 Hz even when the simulation is far behind.
            // Bug №268: this used to be derived from the tick counter, so a slow
            // simulation silently stopped reaping dead connections.
            let mut rounds: u64 = 0;

            while *running.lock() {
                interval.tick().await;
                rounds += 1;

                // Bug №28: reap silent connections once a second so dead
                // clients stop consuming snapshots, interest and ids.
                //
                // Bug №268: this runs BEFORE the unchanged-tick skip on
                // purpose. Reaping is wall-clock work — a connection that has
                // stopped answering is dead whether or not the world moved —
                // so it must not be tied to the simulation keeping up. Sitting
                // after the skip, a lagging simulation would also stop
                // reaping, and dead clients would keep consuming interest and
                // snapshot work forever, which is the exact leak the sweep
                // exists to prevent.
                if rounds.is_multiple_of(tick_rate as u64) {
                    Self::sweep_timeouts(
                        &connections,
                        &connection_by_addr,
                        &delta_compressor,
                        &client_layers,
                        &event_sender,
                        &dropped_events,
                    );
                }

                let tick = *current_tick.lock();
                if last_published != Some(tick) {
                    last_published = Some(tick);
                    let time = *server_time.lock();

                    let snapshot = Self::build_snapshot(tick, time, &interest_manager);
                    snapshot_buffer.write_snapshot(snapshot.clone());
                    snapshot_rounds.fetch_add(1, Ordering::Relaxed);

                    Self::send_snapshots(
                        &socket,
                        &connections,
                        &client_layers,
                        &snapshot_buffer,
                        &delta_compressor,
                        &interest_manager,
                        &bandwidth_tracker,
                        limit_enabled,
                        tick,
                        &snapshot_packets,
                        &sent_deltas_by_layer,
                    )
                    .await;
                }

                // Heartbeats are liveness, not state. Bug №268: this used to
                // sit behind the snapshot work, so skipping a round also
                // skipped the heartbeat — and since the timeout sweep runs
                // unconditionally, a lagging simulation would have made every
                // connected client look dead and reaped it. The two must be
                // independent: the world can stand still while the server
                // still has to prove it is there.
                Self::send_heartbeats(&socket, &connections, &bandwidth_tracker, limit_enabled)
                    .await;

                // Bug №215: retransmit pass immediately after the heartbeat
                // pass. Wall-clock work next to wall-clock work; neither is
                // part of the unchanged-tick skip above.
                Self::send_retransmits(&socket, &connections, &bandwidth_tracker, limit_enabled)
                    .await;
            }
        });

        *self.send_handle.lock() = Some(handle);
    }

    /// Remove every connection that stopped answering (bug №28). Removal and
    /// event emission go through one helper so each timeout is reported
    /// exactly once even if a manual disconnect races it (bug №32).
    fn sweep_timeouts(
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        connection_by_addr: &Arc<RwLock<HashMap<SocketAddr, u32>>>,
        delta_compressor: &Arc<DeltaCompressor>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        event_sender: &mpsc::Sender<ServerEvent>,
        // Bug №613: the bounded event queue drops what it cannot hold, and
        // this is where that drop is counted.
        dropped_events: &AtomicU64,
    ) {
        let timed_out: Vec<(u32, SocketAddr)> = {
            let conns = connections.read();
            conns
                .iter()
                .filter(|(_, conn)| conn.is_timed_out())
                .map(|(id, conn)| (*id, conn.addr))
                .collect()
        };
        // Bug №219: Connected, but the game layer never registered the
        // player (failed spawn, entity cap, a forgotten add_player
        // registration). The snapshot gate keeps every state byte back in
        // exactly this state, so the client sits on a silent socket for the
        // whole session unless this pass ends it.
        let unregistered: Vec<(u32, SocketAddr)> = {
            let conns = connections.read();
            let layers = client_layers.read();
            conns
                .iter()
                .filter(|(_, conn)| {
                    conn.state() == ConnectionState::Connected
                        && !layers.contains_key(&conn.id)
                        && conn.age_connected() >= PLAYER_WAIT_TIMEOUT
                })
                .map(|(id, conn)| (*id, conn.addr))
                .collect()
        };
        // Bug №272: this sweep runs once a second, so it is also where the
        // per-connection reliable-window overflow gets reported — in bulk,
        // once per connection, instead of once per dropped entry. See
        // `Connection::log_pending_ack_summary` for why that moved.
        {
            let conns = connections.read();
            let mut dropped_total = 0u64;
            let mut worst: Option<(u32, u64)> = None;
            for (id, conn) in conns.iter() {
                let drops = conn.pending_ack_drops_since(0);
                if drops > 0 {
                    dropped_total += drops;
                    if worst.is_none_or(|(_, d)| drops > d) {
                        worst = Some((*id, drops));
                    }
                }
            }
            if let Some((id, _)) = worst {
                warn!(
                    "reliable windows: {dropped_total} unacked entries aged out across {} \
                     connections (worst: conn {id}); no retransmit layer exists, so these \
                     were never recoverable — see bug №272",
                    conns.len(),
                );
            }
        }
        for (id, addr) in timed_out {
            if Self::remove_connection(
                connection_by_addr,
                connections,
                delta_compressor,
                client_layers,
                id,
                addr,
            ) {
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::ClientDisconnected {
                        connection_id: id,
                        reason: DisconnectReason::Timeout,
                    },
                );
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::ClientTimeout { connection_id: id },
                );
            }
        }
        for (id, addr) in unregistered {
            warn!(
                "connection {id}: player never registered within {PLAYER_WAIT_TIMEOUT:?}; \
                 disconnecting (the layer gate holds all state until has_player turns \
                 true — bug №219)"
            );
            // Same one-shot helper as the timeout sweep (bug №32): the removal
            // either happens here exactly once, or never because a manual
            // disconnect raced it.
            if Self::remove_connection(
                connection_by_addr,
                connections,
                delta_compressor,
                client_layers,
                id,
                addr,
            ) {
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::Error {
                        connection_id: Some(id),
                        error: format!("player never registered within {PLAYER_WAIT_TIMEOUT:?}"),
                    },
                );
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::ClientDisconnected {
                        connection_id: id,
                        reason: DisconnectReason::Timeout,
                    },
                );
                emit_event(
                    event_sender,
                    dropped_events,
                    ServerEvent::ClientTimeout { connection_id: id },
                );
            }
        }
    }

    /// Erase a connection from every table. Returns true if it was present —
    /// callers emit disconnect events only then, so a timeout racing a manual
    /// disconnect cannot double-report (bug №32).
    fn remove_connection(
        connection_by_addr: &Arc<RwLock<HashMap<SocketAddr, u32>>>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        delta_compressor: &Arc<DeltaCompressor>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        connection_id: u32,
        addr: SocketAddr,
    ) -> bool {
        let removed = connections.write().remove(&connection_id);
        connection_by_addr.write().remove(&addr);
        delta_compressor.remove_client(connection_id);
        client_layers.write().remove(&connection_id);
        removed.is_some()
    }

    fn build_snapshot(tick: Tick, time: f64, interest_manager: &Arc<InterestManager>) -> Snapshot {
        let mut entities = Vec::new();
        let mut projectiles = Vec::new();

        for ship in interest_manager.get_ships() {
            entities.push(EntitySnapshot::from(&ship));
        }
        for station in interest_manager.get_stations() {
            entities.push(EntitySnapshot::from(&station));
        }
        for player in interest_manager.get_players() {
            entities.push(EntitySnapshot::from(&player));
        }
        for compartment in interest_manager.get_compartments() {
            entities.push(EntitySnapshot::from(&compartment));
        }
        for projectile in interest_manager.get_projectiles() {
            projectiles.push(ProjectileSnapshot::from(&projectile));
        }

        Snapshot {
            tick,
            time,
            entities,
            projectiles,
            events: Vec::new(),
        }
    }

    // The per-connection shared state is passed explicitly rather than
    // bundled into a context struct, to keep the borrow scopes of the
    // individual locks narrow and obvious at each use site.
    #[allow(clippy::too_many_arguments)]
    async fn send_snapshots(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        snapshot_buffer: &Arc<SnapshotBuffer>,
        delta_compressor: &Arc<DeltaCompressor>,
        interest_manager: &Arc<InterestManager>,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
        current_tick: Tick,
        snapshot_packets: &Arc<AtomicU64>,
        // Bug №274: passed so the send path can record what it actually put on
        // the wire, per layer.
        sent_deltas_by_layer: &Arc<[AtomicU64; LAYER_COUNT]>,
    ) {
        let snapshot = snapshot_buffer.get_latest().unwrap_or_else(|| Snapshot {
            tick: current_tick,
            time: current_tick.value() as f64 * TICK_DURATION.as_secs_f64(),
            entities: Vec::new(),
            projectiles: Vec::new(),
            events: Vec::new(),
        });

        // Bug #267: at 50+ clients the per-client filter+diff+serialize inside
        // the tick budget is the wall (applied rate collapsed to 0.86 Hz at 200
        // bots; the payload cache alone only reached 3.76 Hz). Two levers land
        // here: a layer-delta payload cache across clients sharing a base tick,
        // and running the per-client work across CPU cores with rayon. The
        // cache is mutex-wrapped — content-equal hits are short and only the
        // shared layers hit, so contention is minimal and the interest-varying
        // player layer runs in parallel. The final UDP flush stays sequential:
        // socket sends are cheap; the CPU work was the bottleneck.
        //
        // Optimization: cache serialized payloads by content hash, not by
        // client ID. Clients on the same ship see identical layer content, so
        // this allows payload reuse across clients. The cache key is now
        // (layer, resync, base_tick, target_tick, content_hash) where
        // content_hash is computed from the actual entity/projectile data.
        type DeltaCacheKey = (u8, bool, u64, u64, u64);
        type DeltaCacheValue = (
            Vec<EntitySnapshot>,
            Vec<ProjectileSnapshot>,
            Vec<EntitySnapshot>,
            Vec<ProjectileSnapshot>,
            Vec<u8>,
        );
        let delta_cache: std::sync::Mutex<HashMap<DeltaCacheKey, DeltaCacheValue>> =
            std::sync::Mutex::new(HashMap::new());

        // Bug №29: snapshot the client list first, then take the layers lock        // per client and only for map access. Serialization, fragmentation
        // and delta diffing (the expensive parts at 200 clients) run without
        // any global lock held, so disconnects/timeouts never stall behind them.
        let clients: Vec<(u32, Arc<Connection>)> = {
            let conns = connections.read();
            conns.iter().map(|(id, conn)| (*id, conn.clone())).collect()
        };

        let pending_batches: Vec<Vec<(Arc<Connection>, OutgoingPacket)>> = clients
            .par_iter()
            .map(|(client_id, conn)| {
                let mut out: Vec<(Arc<Connection>, OutgoingPacket)> = Vec::new();
                if conn.state() != ConnectionState::Connected {
                    return out;
                }

                // Bug №164: the first full used to be built the moment the
                // connection appeared, which is before the game layer has
                // registered the new player. `should_replicate_entity` answers
                // `false` for an unknown viewer, so the client was locked into an
                // empty base and saw nothing until the first resync (up to 5 s).
                // Hold the first full back until the player is tracked, and
                // refresh the interest sets so the very first full is complete.
                let initialized = client_layers.read().contains_key(client_id);
                if !initialized {
                    if !interest_manager.has_player(*client_id) {
                        return out;
                    }
                    // This is a one-shot per client (the next round finds it in
                    // `client_layers`). Keep the cloning wrapper: one client, once.
                    interest_manager.compute_interest(*client_id);
                }

                let filtered = Self::filter_snapshot(&snapshot, interest_manager, *client_id);
                let tick = filtered.tick.value();

                if !initialized {
                    let mut layers = client_layers.write();
                    layers.entry(*client_id).or_insert_with(|| {
                        // First contact: full state now, layer bases start from it.
                        for packet in conn.send_state(filtered.to_state_packet()) {
                            out.push((conn.clone(), packet));
                        }
                        Self::split_layers(&filtered, tick)
                    });
                }

                for layer in 0..LAYER_COUNT {
                    let layer_u8 = layer as u8;
                    let (base_entities, base_projectiles, last_sent_tick) = {
                        let layers = client_layers.read();
                        match layers.get(client_id) {
                            Some(entry) => (
                                entry[layer].entities.clone(),
                                entry[layer].projectiles.clone(),
                                entry[layer].last_sent_tick,
                            ),
                            None => continue,
                        }
                    };
                    let since = tick.saturating_sub(last_sent_tick);

                    // Current content of this layer.
                    let cur_entities: Vec<EntitySnapshot> = filtered
                        .entities
                        .iter()
                        .filter(|e| entity_layer(e.entity_type) == layer_u8)
                        .cloned()
                        .collect();
                    let cur_projectiles: Vec<ProjectileSnapshot> =
                        if layer_u8 == crate::snapshot::LAYER_PROJECTILE {
                            filtered.projectiles.clone()
                        } else {
                            Vec::new()
                        };
                    let content_empty = cur_entities.is_empty() && cur_projectiles.is_empty();
                    let base_empty = base_entities.is_empty() && base_projectiles.is_empty();

                    // Layer 1 has no fixed rate: it goes out on change, plus resync.
                    let scheduled = since >= LAYER_INTERVAL_TICKS[layer];
                    // Bug №173: resync used to require `!content_empty`, so a layer
                    // that had emptied out could never be resynced again. Its
                    // destroy deltas were then rejected by the client for a base
                    // mismatch and the destroyed entities lingered as ghosts
                    // forever. A resync is exactly the mechanism that prunes an
                    // emptied layer, so allow it whenever the client still holds
                    // base content for the layer — only skip it when both sides
                    // are already empty, where there is nothing to heal.
                    //
                    // Bug №267 (A): an explicit ResyncRequest bypasses the
                    // cadence — the client's base mismatch is a live stall, and
                    // waiting out 15 ticks on a known-stalled layer is the delay
                    // the packet exists to remove. Consumed here, so one request
                    // drives exactly one pass.
                    let requested = conn.take_resync_request(layer);
                    let resync =
                        layer_needs_resync(layer, since, content_empty, base_empty, requested);
                    let on_change = LAYER_INTERVAL_TICKS[layer] == u64::MAX;
                    if !scheduled && !resync && !on_change {
                        continue;
                    }

                    // Compute content hash for cache key. This allows payload reuse
                    // across clients on the same ship with identical layer content.
                    let content_hash = {
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        use std::hash::{Hash, Hasher};
                        for e in &cur_entities {
                            e.entity_id.hash(&mut hasher);
                            e.transform.position.x.to_bits().hash(&mut hasher);
                            e.transform.position.y.to_bits().hash(&mut hasher);
                            e.transform.position.z.to_bits().hash(&mut hasher);
                        }
                        for p in &cur_projectiles {
                            p.entity_id.hash(&mut hasher);
                            p.position.x.to_bits().hash(&mut hasher);
                            p.position.y.to_bits().hash(&mut hasher);
                            p.position.z.to_bits().hash(&mut hasher);
                        }
                        for e in &base_entities {
                            e.entity_id.hash(&mut hasher);
                        }
                        for p in &base_projectiles {
                            p.entity_id.hash(&mut hasher);
                        }
                        hasher.finish()
                    };
                    let key = (layer_u8, resync, last_sent_tick, tick, content_hash);
                    let cached = delta_cache.lock().unwrap().get(&key).and_then(
                        |(b_e, b_p, c_e, c_p, bytes)| {
                            if *b_e == base_entities
                                && *b_p == base_projectiles
                                && *c_e == cur_entities
                                && *c_p == cur_projectiles
                            {
                                Some(bytes.clone())
                            } else {
                                None
                            }
                        },
                    );
                    if let Some(bytes) = cached {
                        for packet in conn.send_state_delta_bytes(bytes) {
                            out.push((conn.clone(), packet));
                        }
                        // The cached payload was serialized for another client, so
                        // `create_delta` never ran for this one and its base ring
                        // would stay behind the tick just sent. That makes the next
                        // real diff declare a base tick the client has already
                        // applied, which it rejects as a base mismatch. Move the
                        // ring to the same tick the cached delta targets.
                        delta_compressor.advance_base(*client_id, layer_u8, tick);
                        // Bug №274: a cached payload is still a delta that went on
                        // the wire to THIS client. It was left uncounted at first,
                        // which made the send-side figure disagree with the
                        // client's apply count by 4-8x on the layers that hit the
                        // cache most (1 and 2 — the ones whose interest sets differ
                        // per client), and a disagreement like that looks like loss
                        // on the wire when it is really a counter that missed the
                        // fast path. Counted here, next to the send, so both
                        // branches are covered by construction.
                        sent_deltas_by_layer[layer_u8 as usize].fetch_add(1, Ordering::Relaxed);
                    } else {
                        let cached_base_entities = base_entities.clone();
                        let cached_base_projectiles = base_projectiles.clone();
                        let base_snapshot = Snapshot {
                            tick: Tick(last_sent_tick),
                            time: filtered.time,
                            entities: if resync { Vec::new() } else { base_entities },
                            projectiles: if resync { Vec::new() } else { base_projectiles },
                            events: Vec::new(),
                        };
                        let target_snapshot = Snapshot {
                            tick: filtered.tick,
                            time: filtered.time,
                            entities: cur_entities.clone(),
                            projectiles: cur_projectiles.clone(),
                            events: Vec::new(),
                        };
                        let delta = delta_compressor.create_delta(
                            *client_id,
                            layer_u8,
                            &base_snapshot,
                            &target_snapshot,
                        );
                        if delta.is_empty() {
                            continue;
                        }
                        // Bug №23/№78: a resync diffed against an empty base is a full
                        // layer replacement. Flag it so the client applies it even after
                        // a lost delta left its base behind, and prunes the old layer.
                        let mut state_delta = delta.to_state_delta();
                        state_delta.is_resync = resync;
                        let bytes = match conn.serialize_state_delta(state_delta) {
                            Some(b) => b,
                            None => continue,
                        };
                        delta_cache.lock().unwrap().insert(
                            key,
                            (
                                cached_base_entities,
                                cached_base_projectiles,
                                cur_entities.clone(),
                                cur_projectiles.clone(),
                                bytes.clone(),
                            ),
                        );
                        for packet in conn.send_state_delta_bytes(bytes) {
                            out.push((conn.clone(), packet));
                        }
                        // Bug №274: count deltas actually put on the wire, per
                        // layer. The client's `applied_delta_count_by_layer` is the
                        // matching receive-side number, and the two are only
                        // comparable as a pair. Without the send side, a per-layer
                        // shortfall reads as "the client dropped it" when the truth
                        // can be "the server never sent it" — measured: layer 2 at
                        // 96 deltas where its 1-tick interval implies 900, and
                        // that turned out to be the server diffing an unchanged
                        // layer to nothing, not a client-side loss.
                        sent_deltas_by_layer[layer_u8 as usize].fetch_add(1, Ordering::Relaxed);
                    }
                    {
                        let mut layers = client_layers.write();
                        if let Some(entry) = layers.get_mut(client_id) {
                            entry[layer] = LayerBase {
                                entities: cur_entities,
                                projectiles: cur_projectiles,
                                last_sent_tick: tick,
                            };
                        }
                    }
                }
                out
            })
            .collect();

        // Bug №266: packets actually put on the wire for this round. Counted
        // before the sends so a round that emits nothing is still visible — an
        // empty round and a missing round look identical from the client side,
        // which is exactly the ambiguity this counter removes.
        snapshot_packets.fetch_add(
            pending_batches.iter().map(|b| b.len() as u64).sum(),
            Ordering::Relaxed,
        );
        for batch in pending_batches {
            for (conn, packet) in batch {
                let _ = Self::send_packet(socket, &conn, &packet, bandwidth, limit_enabled).await;
            }
        }
    }

    /// Split a filtered snapshot into per-layer base content.
    fn split_layers(filtered: &Snapshot, tick: u64) -> [LayerBase; LAYER_COUNT] {
        let mut out: [LayerBase; LAYER_COUNT] = Default::default();
        for entity in &filtered.entities {
            let layer = entity_layer(entity.entity_type) as usize;
            if layer < LAYER_COUNT {
                out[layer].entities.push(entity.clone());
            }
        }
        out[crate::snapshot::LAYER_PROJECTILE as usize].projectiles = filtered.projectiles.clone();
        for base in out.iter_mut() {
            base.last_sent_tick = tick;
        }
        out
    }

    fn filter_snapshot(
        snapshot: &Snapshot,
        interest_manager: &InterestManager,
        player_id: u32,
    ) -> Snapshot {
        Snapshot {
            tick: snapshot.tick,
            time: snapshot.time,
            entities: snapshot
                .entities
                .iter()
                .filter(|e| interest_manager.should_replicate_entity(player_id, e.entity_id))
                .cloned()
                .collect(),
            projectiles: snapshot
                .projectiles
                .iter()
                .filter(|p| interest_manager.should_replicate_entity(player_id, p.entity_id))
                .cloned()
                .collect(),
            events: snapshot.events.clone(),
        }
    }

    async fn send_heartbeats(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
    ) {
        let mut pending_heartbeats: Vec<(Arc<Connection>, OutgoingPacket)> = Vec::new();

        {
            let conns = connections.read();

            for conn in conns.values() {
                if conn.state() == ConnectionState::Connected && conn.should_send_heartbeat() {
                    if let Some(packet) = conn.send_heartbeat() {
                        // Bug №176: the send stamp was never refreshed on the
                        // server, so `last_heartbeat` stayed at construction
                        // time and the measured RTT grew without bound.
                        conn.update_heartbeat();
                        pending_heartbeats.push((conn.clone(), packet));
                    }
                }
            }
        }

        for (conn, packet) in pending_heartbeats {
            let _ = Self::send_packet(socket, &conn, &packet, bandwidth, limit_enabled).await;
        }
    }

    /// Bug №215: drain the per-connection reliable ledgers. This runs on the
    /// send-loop cadence (tick rate, ~30 Hz), NOT behind the unchanged-tick
    /// skip: a lost CommandAck must recover even when the world stands still.
    /// The ledger clamps each pass itself (`RETRANSMIT_MAX_PER_PASS`), so a
    /// burst of stale entries throttles across rounds instead of spending a
    /// whole tick writing to one socket.
    async fn send_retransmits(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        bandwidth: &BandwidthTracker,
        limit_enabled: bool,
    ) {
        let mut pending: Vec<(Arc<Connection>, Vec<OutgoingPacket>)> = Vec::new();

        {
            let conns = connections.read();

            for conn in conns.values() {
                if conn.state() != ConnectionState::Connected {
                    continue;
                }
                let retransmits = conn.collect_retransmits();
                if !retransmits.is_empty() {
                    pending.push((conn.clone(), retransmits));
                }
            }
        }

        for (conn, packets) in pending {
            for packet in &packets {
                let _ = Self::send_packet(socket, &conn, packet, bandwidth, limit_enabled).await;
            }
        }
    }

    pub fn add_entity_to_interest(&self, entity: ShipEntity) {
        self.interest_manager.add_ship(entity);
    }

    /// Bug №605: until now this had zero callers, so a hull the sim had
    /// dropped was never taken out of the interest grids. `main` calls it the
    /// moment the simulation no longer knows the ship, passing the interior
    /// that hung off it so the wreck does not keep replicating as orphaned
    /// compartments and stations.
    pub fn remove_entity_from_interest(
        &self,
        entity_id: EntityId,
        compartment_ids: &[EntityId],
        station_ids: &[EntityId],
    ) {
        self.interest_manager
            .retire_ship(entity_id, compartment_ids, station_ids);
    }

    pub fn update_entity_interest(&self, entity: ShipEntity) {
        self.interest_manager.update_ship(entity);
    }

    pub fn add_player(&self, player: PlayerEntity, ship_id: EntityId) {
        self.interest_manager.add_player(player, ship_id);
    }

    pub fn remove_player(&self, player_id: u32, entity_id: EntityId) {
        self.interest_manager.remove_player(player_id, entity_id);
    }

    pub fn update_player(&self, player: PlayerEntity) {
        self.interest_manager.update_player(player);
    }

    /// The entities an event is about, for interest filtering.
    ///
    /// Every variant of `GameEvent` names the entity it happened to, and an
    /// event about a subject the viewer cannot see is exactly the leak Bug №635
    /// describes. The two global variants — team score and match state — name no
    /// entity at all; an empty list means "everyone", which is the only sensible
    /// answer for something that has no position in the world.
    fn event_subjects(event: &GameEvent) -> SmallVec<[EntityId; 2]> {
        let mut subjects: SmallVec<[EntityId; 2]> = SmallVec::new();
        match event {
            GameEvent::ShipHit {
                target,
                projectile,
                hit_compartment,
                ..
            } => {
                subjects.push(*target);
                subjects.push(*projectile);
                if let Some(compartment) = hit_compartment {
                    subjects.push(*compartment);
                }
            }
            GameEvent::ShipSunk { ship, .. } => subjects.push(*ship),
            GameEvent::ShipRam { rammer, target, .. } => {
                subjects.push(*rammer);
                subjects.push(*target);
            }
            GameEvent::CompartmentFlooded { compartment, .. }
            | GameEvent::CompartmentSealed { compartment }
            | GameEvent::CompartmentBreached { compartment }
            | GameEvent::FireStarted { compartment, .. }
            | GameEvent::FireExtinguished { compartment } => subjects.push(*compartment),
            GameEvent::StationOccupied { station, player }
            | GameEvent::StationVacated { station, player } => {
                subjects.push(*station);
                subjects.push(*player);
            }
            GameEvent::PlayerSpawned { player, ship, .. } => {
                subjects.push(*player);
                subjects.push(*ship);
            }
            GameEvent::PlayerDied { player, killer } => {
                subjects.push(*player);
                if let Some(killer) = killer {
                    subjects.push(*killer);
                }
            }
            GameEvent::ProjectileFired { projectile, .. }
            | GameEvent::ProjectileImpact { projectile, .. } => subjects.push(*projectile),
            GameEvent::BulkheadStateChanged { bulkhead, .. } => subjects.push(*bulkhead),
            GameEvent::PumpStateChanged { pump, .. } => subjects.push(*pump),
            GameEvent::TeamScoreChanged { .. } | GameEvent::MatchStateChanged { .. } => {}
            GameEvent::ChatReceived {
                player, channel, ..
            } => {
                // Bug №634: "All" names no subject — everyone on the server is
                // in the chat. The narrower channels are about the sender, so
                // interest decides: the same-ship viewers can always see the
                // player, far-away viewers cannot. (Team/Whisper never reach
                // the wire as broadcasts — the command path refuses them.)
                match channel {
                    ChatChannel::All => {}
                    _ => subjects.push(*player),
                }
            }
        }
        subjects
    }

    /// Fan an event out to the connections that can actually see what it is
    /// about.
    ///
    /// Bug №635: this used to send to every Connected connection while
    /// snapshots were built per interest. Two costs, both real. The fan-out ran
    /// as O(connections x events) whatever a client could physically see, and
    /// the event itself leaked the state of ships the snapshot deliberately
    /// omitted — a hull at the far end of the map was invisible to the client
    /// and fully described by its events.
    ///
    /// The rule now is the one the snapshot path already keeps: a client is told
    /// about an entity exactly when that entity would be in its snapshot.
    /// `should_replicate_subject` is permissive on purpose (see there) so an
    /// event cannot be silenced by an id the manager has never heard of, and an
    /// event with no subject goes to everyone.
    ///
    /// A viewer that is not tracked yet gets nothing, by the same rule that
    /// holds its first full state back (Bug №164): an event describes a world,
    /// and a client with no world yet should not be handed pieces of one.
    ///
    /// Whether a viewer should receive `event` at all.
    ///
    /// Kept separate from the fan-out so the policy is testable without a
    /// socket: `broadcast_event` decides *who* to write to, this decides *what*
    /// each of them is allowed to learn. One visible subject is enough — a
    /// player who can see the ship that was hit needs the hit even if the shell
    /// that made it has already flown out of range, and a player who can see
    /// the shell needs the impact. Requiring *all* subjects would fail both.
    fn event_reaches(interest: &InterestManager, viewer_player_id: u32, event: &GameEvent) -> bool {
        if !interest.has_player(viewer_player_id) {
            return false;
        }
        let subjects = Self::event_subjects(event);
        if subjects.is_empty() {
            // Nothing is named: match and team state describes the world, not
            // an entity, so every tracked viewer hears it (Bug №635).
            return true;
        }
        subjects
            .iter()
            .any(|s| interest.should_replicate_subject(viewer_player_id, *s))
    }

    pub fn broadcast_event(&self, event: GameEvent) {
        // Bug №172: queue instead of spawning. A mass event to 256 conns is
        // now N queue pushes drained by the single writer task, not a task per
        // connection racing each other onto the wire.
        let conns: Vec<(u32, Arc<Connection>)> = {
            let conns = self.connections.read();
            conns
                .iter()
                .filter(|(_, c)| c.state() == ConnectionState::Connected)
                .map(|(client_id, conn)| (*client_id, conn.clone()))
                .collect()
        };
        for (client_id, conn) in conns {
            if !Self::event_reaches(&self.interest_manager, client_id, &event) {
                continue;
            }
            let packets = conn.send_event(EventPacket {
                events: vec![event.clone()],
            });
            if !packets.is_empty() {
                self.enqueue(conn, packets);
            }
        }
    }

    pub fn send_command_ack(
        &self,
        connection_id: u32,
        command_id: u64,
        success: bool,
        error: Option<String>,
    ) {
        if let Some(conn) = self.get_connection(connection_id) {
            let packets = conn.send_command_ack(command_id, success, error);
            if !packets.is_empty() {
                self.enqueue(conn, packets);
            }
        }
    }

    pub fn send_input_ack(&self, connection_id: u32, ack: InputAckPacket) {
        if let Some(conn) = self.get_connection(connection_id) {
            let packets = conn.send_input_ack(ack);
            if !packets.is_empty() {
                self.enqueue(conn, packets);
            }
        }
    }

    /// Disconnect a client. Returns true if this call actually removed the
    /// connection — callers emit disconnect events only then, so running this
    /// against an already-removed id (or racing the timeout sweep) cannot
    /// double-report (bug №32, №80).
    pub fn disconnect_client(&self, connection_id: u32, reason: DisconnectReason) -> bool {
        let Some(conn) = self.get_connection(connection_id) else {
            return false;
        };

        // Bug №172: queue the farewell instead of spawning a task for it.
        let header = PacketHeader::new(
            PacketType::Disconnect,
            ChannelType::ReliableOrdered,
            conn.next_sequence(),
            0,
            0,
            0,
        );
        if let Some(out) = OutgoingPacket::new(header, DisconnectPacket { reason }) {
            self.enqueue(conn.clone(), vec![out]);
        }

        if Self::remove_connection(
            &self.connection_by_addr,
            &self.connections,
            &self.delta_compressor,
            &self.client_layers,
            connection_id,
            conn.addr,
        ) {
            emit_event(
                &self.event_sender,
                &self.dropped_events,
                ServerEvent::ClientDisconnected {
                    connection_id,
                    reason,
                },
            );
            true
        } else {
            false
        }
    }

    pub fn disconnect_all(&self, reason: DisconnectReason) {
        let ids: Vec<u32> = self.connections.read().keys().copied().collect();
        for id in ids {
            self.disconnect_client(id, reason);
        }
    }

    pub fn check_timeouts(&self) {
        let mut timed_out = Vec::new();
        {
            let conns = self.connections.read();
            for (id, conn) in conns.iter() {
                if conn.is_timed_out() {
                    timed_out.push(*id);
                }
            }
        }

        for id in timed_out {
            // Bug №80: emit ClientTimeout only if we actually removed the
            // connection — the sweep loop or a manual disconnect may have
            // raced us, and double-emission breaks the №32 uniqueness contract.
            if self.disconnect_client(id, DisconnectReason::Timeout) {
                emit_event(
                    &self.event_sender,
                    &self.dropped_events,
                    ServerEvent::ClientTimeout { connection_id: id },
                );
            }
        }
    }

    pub fn get_event_receiver(&self) -> Option<mpsc::Receiver<ServerEvent>> {
        self.event_receiver.lock().take()
    }

    /// Events dropped because the consumer fell behind (Bug №613).
    ///
    /// Exists so a bounded channel is not a new silent failure: the whole point
    /// of bounding the queue is that something has to be dropped, and a drop
    /// nobody can see has simply traded an OOM for a mystery.
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    /// Outbox entries dropped for the same reason (Bug №613).
    pub fn dropped_outbox(&self) -> u64 {
        self.dropped_outbox.load(Ordering::Relaxed)
    }

    pub fn bandwidth_stats(&self) -> BandwidthStats {
        self.bandwidth_tracker.stats()
    }

    /// Snapshot rounds run and packets emitted, for the publication-rate metric.
    ///
    /// Bug №266. Monotonic totals, so the caller differences them over a window
    /// to get a rate — an absolute count says nothing about rate.
    pub fn snapshot_totals(&self) -> (u64, u64) {
        (
            self.snapshot_rounds.load(Ordering::Relaxed),
            self.snapshot_packets.load(Ordering::Relaxed),
        )
    }

    /// Bug №274: deltas actually sent, per layer. Read together with the
    /// client's `applied_delta_count_by_layer`: sent-vs-applied separates "the
    /// server had nothing new to send" from "the client dropped it".
    pub fn sent_deltas_by_layer(&self) -> Vec<u64> {
        self.sent_deltas_by_layer
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .collect()
    }

    pub fn interest_manager(&self) -> Arc<InterestManager> {
        self.interest_manager.clone()
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn interest_stats(&self) -> crate::interest::InterestStats {
        self.interest_manager.stats()
    }
}

#[cfg(test)]
mod intro_timeout_tests {
    use super::*;

    fn conn(id: u32, addr: SocketAddr, age: Option<Duration>) -> Connection {
        let c = match age {
            Some(a) => Connection::new_backdated(id, addr, ConnectionConfig::default(), a),
            None => Connection::new(id, addr, ConnectionConfig::default()),
        };
        c.set_state(ConnectionState::Connected);
        c
    }

    /// Bug №219: a Connected client whose game-layer player was never
    /// registered receives no state bytes at all — the snapshot gate waits
    /// for `has_player` forever. The sweep must end that session after
    /// `PLAYER_WAIT_TIMEOUT` and report it, while fresh connections and
    /// registered ones stay untouched.
    #[test]
    fn a_userless_connected_client_is_reaped_after_the_wait() {
        let connections = Arc::new(RwLock::new(HashMap::new()));
        let by_addr = Arc::new(RwLock::new(HashMap::new()));
        let delta = Arc::new(DeltaCompressor::default());
        let layers = Arc::new(RwLock::new(HashMap::new()));
        let (tx, mut rx) = mpsc::channel(64);
        let dropped = AtomicU64::new(0);

        let fresh_addr: SocketAddr = "127.0.0.1:40001".parse().unwrap();
        let stale_addr: SocketAddr = "127.0.0.1:40002".parse().unwrap();
        let registered_addr: SocketAddr = "127.0.0.1:40003".parse().unwrap();

        let fresh = Arc::new(conn(9, fresh_addr, None));
        let stale = Arc::new(conn(
            5,
            stale_addr,
            Some(PLAYER_WAIT_TIMEOUT + Duration::from_secs(1)),
        ));
        let registered = Arc::new(conn(
            7,
            registered_addr,
            Some(PLAYER_WAIT_TIMEOUT + Duration::from_secs(1)),
        ));
        // The registered one has its layer base initialized: even though it
        // is old, the gate opened long ago, so it is not a candidate.
        layers.write().insert(7, Default::default());

        connections.write().insert(9, fresh.clone());
        connections.write().insert(5, stale.clone());
        connections.write().insert(7, registered.clone());
        by_addr.write().insert(fresh_addr, 9);
        by_addr.write().insert(stale_addr, 5);
        by_addr.write().insert(registered_addr, 7);

        NetServer::sweep_timeouts(&connections, &by_addr, &delta, &layers, &tx, &dropped);

        assert_eq!(
            dropped.load(Ordering::Relaxed),
            0,
            "queue of 64 must hold all events"
        );

        // The unregistered stale one is gone; the other two remain.
        assert!(
            !connections.read().contains_key(&5),
            "stale userless conn reaped"
        );
        assert!(
            connections.read().contains_key(&9),
            "a fresh waiting conn stays"
        );
        assert!(
            connections.read().contains_key(&7),
            "a registered conn stays"
        );

        // The report names the cause, then the formal pair of lifecycle
        // events — three events for the reaped connection exactly.
        let mut kinds: Vec<String> = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            match ev {
                ServerEvent::Error {
                    connection_id: Some(5),
                    error,
                } => {
                    assert!(
                        error.contains("player never registered"),
                        "the error must name the cause, got: {error}"
                    );
                    kinds.push("Error".into());
                }
                ServerEvent::ClientDisconnected {
                    connection_id: 5, ..
                } => {
                    kinds.push("Disconnected".into());
                }
                ServerEvent::ClientTimeout { connection_id: 5 } => {
                    kinds.push("Timeout".into());
                }
                _ => {} // events for 9/7 would also be a failure below
            }
        }
        assert_eq!(
            kinds,
            ["Error", "Disconnected", "Timeout"],
            "exactly the reaped session's lifecycle must be reported, got {kinds:?}"
        );
    }
}

#[cfg(test)]
mod resync_request_tests {
    //! Bug №267 (A): the client's resync ask must reach the layer, and it must
    //! bypass the cadence — the whole point of the packet is that a stalled
    //! layer is healed now, not in 15 ticks.

    use super::*;

    fn conn() -> Connection {
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        Connection::new(1, addr, ConnectionConfig::default())
    }

    #[test]
    fn an_explicit_request_resyncs_immediately_regardless_of_the_cadence() {
        // The cadence has not elapsed at all: without the request the layer
        // would sit and wait.
        assert!(
            !layer_needs_resync(0, 0, false, false, false),
            "without a request the cadence decides"
        );
        assert!(
            layer_needs_resync(0, 0, false, false, true),
            "an explicit request must resync the layer at once"
        );
    }

    #[test]
    fn the_cadence_still_heals_an_emptied_layer_that_holds_a_base() {
        // Bug №173's path: the content is gone, the client still holds a base.
        assert!(layer_needs_resync(
            0,
            LAYER_RESYNC_TICKS[0],
            true,
            false,
            false
        ));
        // Nothing to heal: both sides are empty.
        assert!(!layer_needs_resync(
            0,
            LAYER_RESYNC_TICKS[0],
            true,
            true,
            false
        ));
        // Not due yet.
        assert!(!layer_needs_resync(0, 0, false, false, false));
    }

    #[test]
    fn an_incoming_request_arms_the_asked_layer() {
        let conn = conn();
        let payload = bincode::serialize(&ResyncRequestPacket { layer: 2 }).unwrap();
        assert!(apply_resync_request(&conn, &payload));
        assert!(conn.take_resync_request(2), "layer 2 must be armed");
        assert!(!conn.take_resync_request(0), "no other layer may be armed");
    }

    #[test]
    fn a_request_for_a_layer_outside_the_range_is_refused() {
        let conn = conn();
        let payload = bincode::serialize(&ResyncRequestPacket {
            layer: LAYER_COUNT as u8,
        })
        .unwrap();
        assert!(
            !apply_resync_request(&conn, &payload),
            "a layer index from the wire must be bounds-checked"
        );
        for layer in 0..LAYER_COUNT {
            assert!(!conn.take_resync_request(layer));
        }
    }

    #[test]
    fn a_malformed_request_arms_nothing() {
        let conn = conn();
        assert!(!apply_resync_request(&conn, &[0xff, 0xff, 0xff, 0xff]));
        for layer in 0..LAYER_COUNT {
            assert!(!conn.take_resync_request(layer));
        }
    }
}

#[cfg(test)]
mod bounded_queue_tests {
    use super::*;

    /// Bug №613: the event queue was an `unbounded_channel`, so a producer
    /// outrunning the tick loop grew memory without limit — the failure mode is
    /// an OOM, not a slowdown. Bounding it introduces a new one: a full queue
    /// drops an event. That drop is counted here, because a bounded queue whose
    /// drops are invisible has merely traded an OOM for a mystery.
    #[test]
    fn a_full_event_queue_drops_and_counts() {
        let dropped = AtomicU64::new(0);
        let (tx, rx) = mpsc::channel(2);

        // Two fit; the third has nowhere to go.
        for id in 1..=3 {
            emit_event(
                &tx,
                &dropped,
                ServerEvent::Error {
                    connection_id: Some(id),
                    error: format!("flood {id}"),
                },
            );
        }
        assert_eq!(
            dropped.load(Ordering::Relaxed),
            1,
            "the event the full queue dropped was not counted"
        );

        // A closed channel is NOT counted: nothing is waiting to read it any
        // more, so the number would only grow for a server already going down.
        drop(rx);
        emit_event(
            &tx,
            &dropped,
            ServerEvent::Error {
                connection_id: Some(4),
                error: "after close".into(),
            },
        );
        assert_eq!(
            dropped.load(Ordering::Relaxed),
            1,
            "a closed channel must not inflate the backpressure counter"
        );
    }

    /// The two queues are bounded for the same reason, so the cap has to be a
    /// number rather than a default: a test that cannot fill the queue cannot
    /// tell a bound from no bound.
    #[test]
    fn the_queues_really_are_bounded() {
        let (tx, _rx) = mpsc::channel::<u8>(1);
        assert!(tx.try_send(1).is_ok());
        assert!(
            matches!(tx.try_send(2), Err(mpsc::error::TrySendError::Full(_))),
            "a channel of capacity 1 accepted a second item, so it is not bounded"
        );
    }
}

/// Bug №635: `broadcast_event` sent every event to every connection while
/// snapshots were built per interest.
///
/// These test the policy, not the fan-out. `event_reaches` is the whole decision
/// and is deliberately callable without a socket; the loop that applies it is
/// three lines of plumbing around it.
#[cfg(test)]
mod event_interest_tests {
    use super::*;
    use rfs_core::entity::PlayerPosture;
    use rfs_core::math::{Transform, Vec3f};

    const VIEWER: u32 = 1;
    const VIEWER_SHIP: u64 = 10;
    const VIEWER_PLAYER_ENTITY: u64 = 20;
    const FAR_SHIP: u64 = 30;
    const FAR_PLAYER_ENTITY: u64 = 40;
    const BULKHEAD: u64 = 50;

    fn ship(id: u64, x: f32) -> ShipEntity {
        ShipEntity {
            entity_id: EntityId::new(id),
            ship_class_id: 0,
            transform: Transform::from_position(Vec3f::new(x, 0.0, 0.0)),
            velocity: Vec3f::ZERO,
            angular_velocity: Vec3f::ZERO,
            health: 100.0,
            max_health: 100.0,
            flags: EntityFlags::empty(),
            fuel: 50.0,
            max_fuel: 50.0,
            speed: 0.0,
            max_speed: 10.0,
            heading: 0.0,
            rudder_angle: 0.0,
            throttle: 0.0,
            last_hit: None,
            compartments: Vec::new(),
            stations: Vec::new(),
            team: 0,
        }
    }

    fn player(entity_id: u64, player_id: u32, ship_id: u64, name: &str) -> PlayerEntity {
        PlayerEntity {
            entity_id: EntityId::new(entity_id),
            player_id,
            name: name.to_string(),
            team: 0,
            transform: Transform::from_position(Vec3f::new(0.0, 0.0, 0.0)),
            velocity: Vec3f::ZERO,
            posture: PlayerPosture::Standing,
            health: 100.0,
            max_health: 100.0,
            stamina: 100.0,
            max_stamina: 100.0,
            current_station: None,
            current_ship: Some(EntityId::new(ship_id)),
            input_sequence: 0,
            last_acknowledged_tick: 0,
        }
    }

    /// The viewer at the origin on its own ship, plus a second hull and crewman
    /// parked far beyond `ship_radius` (5000 by default), so anything about them
    /// is invisible by construction.
    fn world() -> InterestManager {
        let mgr = InterestManager::new(InterestConfig::default());
        mgr.add_ship(ship(VIEWER_SHIP, 0.0));
        mgr.add_player(
            player(VIEWER_PLAYER_ENTITY, VIEWER, VIEWER_SHIP, "viewer"),
            EntityId::new(VIEWER_SHIP),
        );
        mgr.add_ship(ship(FAR_SHIP, 20_000.0));
        mgr.add_player(
            player(FAR_PLAYER_ENTITY, 2, FAR_SHIP, "far"),
            EntityId::new(FAR_SHIP),
        );
        mgr.compute_interest(VIEWER);
        mgr
    }

    fn ship_sunk(ship_id: u64) -> GameEvent {
        GameEvent::ShipSunk {
            ship: EntityId::new(ship_id),
            position: Vec3f::ZERO,
        }
    }

    /// Every variant has to name the entity it happened to, or the filter has
    /// nothing to filter on and the leak comes straight back. The two global
    /// variants name none — they cannot be filtered, and they are not meant to
    /// be.
    #[test]
    fn every_event_variant_names_its_subject() {
        let id = EntityId::new(1);
        let empty = |e: GameEvent| NetServer::event_subjects(&e).is_empty();

        assert!(empty(GameEvent::TeamScoreChanged { team: 0, score: 1 }));
        assert!(empty(GameEvent::MatchStateChanged {
            state: MatchState::Waiting
        }));

        let named = [
            GameEvent::ShipHit {
                target: id,
                projectile: id,
                position: Vec3f::ZERO,
                normal: Vec3f::ZERO,
                damage: 1.0,
                penetration: 1.0,
                hit_compartment: None,
            },
            ship_sunk(1),
            GameEvent::CompartmentFlooded {
                compartment: id,
                water_level: 1.0,
            },
            GameEvent::CompartmentSealed { compartment: id },
            GameEvent::CompartmentBreached { compartment: id },
            GameEvent::StationOccupied {
                station: id,
                player: id,
            },
            GameEvent::StationVacated {
                station: id,
                player: id,
            },
            GameEvent::PlayerSpawned {
                player: id,
                ship: id,
                position: Vec3f::ZERO,
            },
            GameEvent::PlayerDied {
                player: id,
                killer: None,
            },
            GameEvent::ProjectileFired {
                projectile: id,
                position: Vec3f::ZERO,
                velocity: Vec3f::ZERO,
                weapon: id,
            },
            GameEvent::ProjectileImpact {
                projectile: id,
                position: Vec3f::ZERO,
                normal: Vec3f::ZERO,
            },
            GameEvent::BulkheadStateChanged {
                bulkhead: id,
                is_sealed: true,
            },
            GameEvent::PumpStateChanged {
                pump: id,
                is_active: true,
            },
            GameEvent::FireStarted {
                compartment: id,
                intensity: 1.0,
            },
            GameEvent::FireExtinguished { compartment: id },
        ];
        for event in named {
            assert!(
                !NetServer::event_subjects(&event).is_empty(),
                "{event:?} named no subject, so it could never be filtered"
            );
        }
    }

    /// The defect itself: a hull the client's snapshot does not contain must not
    /// be described by its events. Before №635 this reached the viewer.
    #[test]
    fn an_event_about_a_ship_outside_the_viewer_interest_is_not_sent() {
        let mgr = world();
        assert!(
            !mgr.should_replicate_entity(VIEWER, EntityId::new(FAR_SHIP)),
            "the fixture is not actually invisible, so this proves nothing"
        );
        assert!(
            !NetServer::event_reaches(&mgr, VIEWER, &ship_sunk(FAR_SHIP)),
            "a ship beyond interest leaked to the viewer through its events"
        );
    }

    /// The other half: the same event about a hull the viewer can see still
    /// arrives. Without this the filter could be made to pass by dropping
    /// everything.
    #[test]
    fn an_event_about_a_ship_the_viewer_can_see_is_sent() {
        let mgr = world();
        assert!(
            mgr.should_replicate_entity(VIEWER, EntityId::new(VIEWER_SHIP)),
            "the fixture's own ship is not visible, so this proves nothing"
        );
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &ship_sunk(VIEWER_SHIP)),
            "an event about a visible ship was dropped"
        );
    }

    /// Bug №634: chat is routed by its channel, not by the entity in it.
    /// `All` names no subject — everyone on the server is in the chat. A
    /// narrower channel is about the sender: the crew that can see the
    /// player hears them, interest decides; a far player's narrow chat must
    /// not reach a viewer who cannot see them. (The command path refuses
    /// Team/Whisper outright, so Ship is the only narrow channel that
    /// reaches the wire as an event.)
    #[test]
    fn chat_events_are_routed_by_their_channel() {
        let mgr = world();
        let author = EntityId::new(VIEWER_PLAYER_ENTITY);
        let all_chat = GameEvent::ChatReceived {
            player: author,
            message: "hello all".into(),
            channel: ChatChannel::All,
        };
        let ship_chat = GameEvent::ChatReceived {
            player: author,
            message: "hello ship".into(),
            channel: ChatChannel::Ship,
        };
        let far_chat = GameEvent::ChatReceived {
            player: EntityId::new(FAR_PLAYER_ENTITY),
            message: "hello ship".into(),
            channel: ChatChannel::Ship,
        };

        assert!(
            NetServer::event_subjects(&all_chat).is_empty(),
            "all-chat named a subject, so interest would filter what cannot be filtered"
        );
        assert!(
            NetServer::event_subjects(&ship_chat).contains(&author),
            "ship-chat named no subject, so it would broadcast to the whole server"
        );
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &all_chat),
            "a viewer must receive all-chat"
        );
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &ship_chat),
            "a crewmate must receive the ship channel of a player they can see"
        );
        assert!(
            !NetServer::event_reaches(&mgr, VIEWER, &far_chat),
            "a far player's narrow chat leaked through the event path"
        );
    }

    /// `entity_layers` never holds bulkheads or pumps, so a plain
    /// `should_replicate_entity` answers `false` for them and the whole
    /// category goes silent. This is the carve-out that keeps that from
    /// shipping unnoticed.
    #[test]
    fn an_event_about_an_id_the_manager_does_not_track_is_not_silenced() {
        let mgr = world();
        let event = GameEvent::BulkheadStateChanged {
            bulkhead: EntityId::new(BULKHEAD),
            is_sealed: true,
        };
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &event),
            "an event about an untracked id was silently dropped"
        );
    }

    /// The same idea, checked from the other side: the manager must not report
    /// a bulkhead as invisible just because it does not know it.
    #[test]
    fn an_untracked_id_is_not_reported_as_invisible() {
        let mgr = world();
        assert!(
            mgr.should_replicate_subject(VIEWER, EntityId::new(BULKHEAD)),
            "an id the manager has never seen was treated as hidden"
        );
        assert!(
            !mgr.should_replicate_subject(VIEWER, EntityId::new(FAR_SHIP)),
            "a tracked ship outside interest was treated as visible"
        );
    }

    /// One visible subject is enough. A player who can see the hull that was
    /// hit needs the hit even when the shell has already flown out of range.
    #[test]
    fn one_visible_subject_is_enough() {
        let mgr = world();
        let event = GameEvent::PlayerSpawned {
            // Own crewman (visible) aboard a hull nobody can see.
            player: EntityId::new(VIEWER_PLAYER_ENTITY),
            ship: EntityId::new(FAR_SHIP),
            position: Vec3f::ZERO,
        };
        assert!(
            NetServer::event_reaches(&mgr, VIEWER, &event),
            "an event was dropped although one of its subjects was visible"
        );

        // Neither subject visible ⇒ dropped.
        let event = GameEvent::PlayerSpawned {
            player: EntityId::new(FAR_PLAYER_ENTITY),
            ship: EntityId::new(FAR_SHIP),
            position: Vec3f::ZERO,
        };
        assert!(
            !NetServer::event_reaches(&mgr, VIEWER, &event),
            "an event about two invisible subjects reached the viewer"
        );
    }

    /// Match and team state name no entity and go to everyone who has a world.
    #[test]
    fn an_event_with_no_subject_reaches_a_tracked_viewer() {
        let mgr = world();
        let event = GameEvent::MatchStateChanged {
            state: MatchState::Starting,
        };
        assert!(NetServer::event_reaches(&mgr, VIEWER, &event));
        assert!(NetServer::event_reaches(
            &mgr,
            VIEWER,
            &GameEvent::TeamScoreChanged { team: 0, score: 3 }
        ));
    }

    /// A connection the game layer has not registered yet has no world, and
    /// Bug №164 already holds its first full state back for that reason. Events
    /// follow the same rule instead of describing a world it has not been given.
    #[test]
    fn a_viewer_the_game_layer_has_not_registered_yet_hears_nothing() {
        let mgr = world();
        let unregistered = 99;
        assert!(!mgr.has_player(unregistered));
        assert!(!NetServer::event_reaches(
            &mgr,
            unregistered,
            &ship_sunk(VIEWER_SHIP)
        ));
        assert!(
            !NetServer::event_reaches(
                &mgr,
                unregistered,
                &GameEvent::MatchStateChanged {
                    state: MatchState::Starting,
                }
            ),
            "an untracked viewer was handed a match event"
        );
    }
}
