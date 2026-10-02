use crate::connection::{is_reliable_channel, Connection, ConnectionConfig, ConnectionState, OutgoingPacket};
use crate::interest::{InterestManager};
use crate::snapshot::{Snapshot, SnapshotBuffer, DeltaCompressor, EntitySnapshot, ProjectileSnapshot, LayerBase, LAYER_COUNT, LAYER_INTERVAL_TICKS, LAYER_RESYNC_TICKS, entity_layer};
use crate::bandwidth::{Admission, BandwidthTracker, BandwidthStats};
use rfs_core::packet::*;
use rfs_core::entity::{ShipEntity, PlayerEntity};
use rfs_core::spatial::InterestConfig;
use rfs_core::time::{Tick, TICK_RATE, TICK_DURATION};
use bytes::BytesMut;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn, error};
use rayon::prelude::*;
use uuid::Uuid;

/// Wire cap for a single bincode message (DoS: Vec len prefix could OOM).
const WIRE_LIMIT: u64 = 1024 * 1024;

fn deserialize_limited<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, bincode::Error> {
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
    event_sender: mpsc::UnboundedSender<ServerEvent>,
    event_receiver: Mutex<Option<mpsc::UnboundedReceiver<ServerEvent>>>,
    match_id: Uuid,
    /// Per-address Connect token buckets (bug №27).
    connect_limits: Arc<Mutex<HashMap<SocketAddr, ConnectLimit>>>,
    /// Bug №172: single ordered outbox. Ack/event/disconnect packets used to
    /// spawn one task each, so the executor queue grew without bound and the
    /// UDP order between concurrently spawned tasks was never guaranteed.
    /// Everything now queues here and a single writer task drains it in order.
    outbox: mpsc::UnboundedSender<OutboxItem>,
    outbox_handle: Mutex<Option<JoinHandle<()>>>,
    /// Held until the server starts, then moved into the writer task.
    outbox_rx: Mutex<Option<mpsc::UnboundedReceiver<OutboxItem>>>,
}

/// One queued datagram awaiting the writer task.
struct OutboxItem {
    conn: Arc<Connection>,
    packet: OutgoingPacket,
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
        }
    }
}

#[derive(Debug, Clone)]
pub enum ServerEvent {
    ClientConnected { connection_id: u32, addr: SocketAddr },
    ClientDisconnected { connection_id: u32, reason: DisconnectReason },
    ClientTimeout { connection_id: u32 },
    ClientInput { connection_id: u32, input: InputPacket },
    ClientCommand { connection_id: u32, command: CommandPacket },
    PacketReceived { connection_id: u32, packet_type: PacketType },
    PacketSent { connection_id: u32, packet_type: PacketType, bytes: usize },
    BandwidthStats { connection_id: u32, stats: BandwidthStats },
    Error { connection_id: Option<u32>, error: String },
}

impl NetServer {
    pub async fn new(config: ServerConfig) -> Result<Self, std::io::Error> {
        let socket = Arc::new(UdpSocket::bind(config.bind_addr).await?);
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        // Bug №172: the outbox and the channel holding its receiving end. The
        // receiver is moved into the writer task when the server starts.
        let (outbox, outbox_rx) = mpsc::unbounded_channel();
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
            bandwidth_tracker: Arc::new(BandwidthTracker::new()),
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
        };

        Ok(server)
    }

    pub fn match_id(&self) -> Uuid {
        self.match_id
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
        *self.running.lock() = false;

        if let Some(handle) = self.send_handle.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.recv_handle.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.outbox_handle.lock().take() {
            handle.abort();
        }

        self.disconnect_all(DisconnectReason::ServerShutdown);
        info!("NetServer stopped");
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
                if let Err(e) = Self::send_packet(&socket, &conn, &packet, &bandwidth, limit_enabled)
                    .await
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
    fn enqueue(&self, conn: Arc<Connection>, packets: Vec<OutgoingPacket>) {
        for packet in packets {
            if self.outbox.send(OutboxItem { conn: conn.clone(), packet }).is_err() {
                warn!("outbox closed; dropping packet for connection {}", conn.id);
                return;
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
        let current_tick = self.current_tick.clone();
        let server_time = self.server_time.clone();
        let match_id = self.match_id;
        let delta_compressor = self.delta_compressor.clone();
        let client_layers = self.client_layers.clone();

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
                            &current_tick,
                            &server_time,
                            match_id,
                            &delta_compressor,
                            &client_layers,
                            addr,
                            data,
                        ).await {
                            warn!("Error handling packet from {}: {}", addr, e);
                            let _ = event_sender.send(ServerEvent::Error {
                                connection_id: None,
                                error: e.to_string(),
                            });
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
        event_sender: &mpsc::UnboundedSender<ServerEvent>,
        current_tick: &Arc<Mutex<Tick>>,
        server_time: &Arc<Mutex<f64>>,
        match_id: Uuid,
        delta_compressor: &Arc<DeltaCompressor>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        addr: SocketAddr,
        data: &[u8],
    ) -> anyhow::Result<()> {
        if data.len() < HEADER_SIZE {
            return Err(anyhow::anyhow!("Packet too small: {} < {}", data.len(), HEADER_SIZE));
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
                            0, 0, 0, 0,
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
                            if *id_gen == 0 { *id_gen = 1; }
                            id
                        };

                        // Bug №176: hand each connection its own send budget
                        // instead of sharing one server-wide bucket.
                        let conn_config = ConnectionConfig {
                            max_bandwidth_bps: config.max_bandwidth_bps,
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
                    let reject = ConnectRejectPacket { reason: ConnectRejectReason::ServerFull };
                    let header = PacketHeader::new(
                        PacketType::ConnectReject,
                        ChannelType::ReliableOrdered,
                        0, 0, 0, 0,
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

                let _ = event_sender.send(ServerEvent::ClientConnected { connection_id: id, addr });

                if let Some(accept) = accept {
                    Self::send_packet(socket, &conn_arc, &accept, bandwidth, limit_enabled).await.map(|_| ())?;
                }

                id
            }
        };

        let conn = connections.read().get(&connection_id).cloned();

        if let Some(conn) = conn {
            let packets = conn.handle_packet(header, payload)?;
            
            for packet in packets {
                Self::send_packet(socket, &conn, &packet, bandwidth, limit_enabled).await.map(|_| ())?;
            }

            match PacketType::from_u8(header.packet_type) {
                Some(PacketType::Input) => {
                    if let Ok(input) = deserialize_limited::<InputPacket>(payload) {
                        let _ = event_sender.send(ServerEvent::ClientInput { connection_id, input });
                    }
                }
                Some(PacketType::Command) => {
                    if let Ok(command) = deserialize_limited::<CommandPacket>(payload) {
                        let _ = event_sender.send(ServerEvent::ClientCommand { connection_id, command });
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
                            let reject =
                                ConnectRejectPacket { reason: ConnectRejectReason::VersionMismatch };
                            let header = PacketHeader::new(
                                PacketType::ConnectReject,
                                ChannelType::ReliableOrdered,
                                0, 0, 0, 0,
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
                        Self::send_packet(socket, &conn, &accept, bandwidth, limit_enabled).await.map(|_| ())?;
                    }
                }
                Some(PacketType::Fragment) => {
                    if let Some((orig_type, bytes)) = conn.reassemble_fragment(payload) {
                        match orig_type {
                            PacketType::Input => {
                                if let Ok(input) = deserialize_limited::<InputPacket>(&bytes) {
                                    let _ = event_sender.send(ServerEvent::ClientInput { connection_id, input });
                                }
                            }
                            PacketType::Command => {
                                if let Ok(command) = deserialize_limited::<CommandPacket>(&bytes) {
                                    let _ = event_sender.send(ServerEvent::ClientCommand { connection_id, command });
                                }
                            }
                            _ => {}
                        }
                    }
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
                        let _ = event_sender.send(ServerEvent::ClientDisconnected {
                            connection_id,
                            reason,
                        });
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
            return Err(anyhow::anyhow!("Packet too large: {} > {}", total_size, max));
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
        let current_tick = self.current_tick.clone();
        let server_time = self.server_time.clone();
        let tick_rate = self.tick_rate;

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1000 / tick_rate as u64));
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
                if rounds % tick_rate as u64 == 0 {
                    Self::sweep_timeouts(
                        &connections,
                        &connection_by_addr,
                        &delta_compressor,
                        &client_layers,
                        &event_sender,
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
                ).await;
                }

                // Heartbeats are liveness, not state. Bug №268: this used to
                // sit behind the snapshot work, so skipping a round also
                // skipped the heartbeat — and since the timeout sweep runs
                // unconditionally, a lagging simulation would have made every
                // connected client look dead and reaped it. The two must be
                // independent: the world can stand still while the server
                // still has to prove it is there.
                Self::send_heartbeats(&socket, &connections, &bandwidth_tracker, limit_enabled).await;
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
        event_sender: &mpsc::UnboundedSender<ServerEvent>,
    ) {
        let timed_out: Vec<(u32, SocketAddr)> = {
            let conns = connections.read();
            conns
                .iter()
                .filter(|(_, conn)| conn.is_timed_out())
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
                    if worst.map_or(true, |(_, d)| drops > d) {
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
            if Self::remove_connection(connection_by_addr, connections, delta_compressor, client_layers, id, addr) {
                let _ = event_sender.send(ServerEvent::ClientDisconnected {
                    connection_id: id,
                    reason: DisconnectReason::Timeout,
                });
                let _ = event_sender.send(ServerEvent::ClientTimeout { connection_id: id });
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

    fn build_snapshot(
        tick: Tick,
        time: f64,
        interest_manager: &Arc<InterestManager>,
    ) -> Snapshot {
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
        let delta_cache: std::sync::Mutex<
            HashMap<(u8, bool, u64, u64, u64), (Vec<EntitySnapshot>, Vec<ProjectileSnapshot>, Vec<EntitySnapshot>, Vec<ProjectileSnapshot>, Vec<u8>)>,
        > = std::sync::Mutex::new(HashMap::new());

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
                let cur_entities: Vec<EntitySnapshot> = filtered.entities.iter()
                    .filter(|e| entity_layer(e.entity_type) == layer_u8)
                    .cloned()
                    .collect();
                let cur_projectiles: Vec<ProjectileSnapshot> = if layer_u8 == crate::snapshot::LAYER_PROJECTILE {
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
                let resync = since >= LAYER_RESYNC_TICKS[layer] && (!content_empty || !base_empty);
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
                let cached = delta_cache.lock().unwrap().get(&key).and_then(|(b_e, b_p, c_e, c_p, bytes)| {
                    if *b_e == base_entities
                        && *b_p == base_projectiles
                        && *c_e == cur_entities
                        && *c_p == cur_projectiles
                    {
                        Some(bytes.clone())
                    } else {
                        None
                    }
                });
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
                    sent_deltas_by_layer[layer_u8 as usize]
                        .fetch_add(1, Ordering::Relaxed);
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
                    let delta = delta_compressor.create_delta(*client_id, layer_u8, &base_snapshot, &target_snapshot);
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
                    sent_deltas_by_layer[layer_u8 as usize]
                        .fetch_add(1, Ordering::Relaxed);
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
            entities: snapshot.entities.iter()
                .filter(|e| interest_manager.should_replicate_entity(player_id, e.entity_id))
                .cloned()
                .collect(),
            projectiles: snapshot.projectiles.iter()
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

    pub fn add_entity_to_interest(&self, entity: ShipEntity) {
        self.interest_manager.add_ship(entity);
    }

    pub fn remove_entity_from_interest(&self, entity_id: EntityId) {
        self.interest_manager.remove_ship(entity_id);
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

    pub fn broadcast_event(&self, event: GameEvent) {
        let event_packet = EventPacket { events: vec![event] };
        // Bug №172: queue instead of spawning. A mass event to 256 conns is
        // now N queue pushes drained by the single writer task, not a task per
        // connection racing each other onto the wire.
        let conns: Vec<Arc<Connection>> = {
            let conns = self.connections.read();
            conns
                .values()
                .filter(|c| c.state() == ConnectionState::Connected)
                .cloned()
                .collect()
        };
        for conn in conns {
            let packets = conn.send_event(event_packet.clone());
            if !packets.is_empty() {
                self.enqueue(conn, packets);
            }
        }
    }

    pub fn send_command_ack(&self, connection_id: u32, command_id: u64, success: bool, error: Option<String>) {
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
            0, 0, 0,
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
            let _ = self.event_sender.send(ServerEvent::ClientDisconnected {
                connection_id,
                reason,
            });
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
                let _ = self.event_sender.send(ServerEvent::ClientTimeout { connection_id: id });
            }
        }
    }

    pub fn get_event_receiver(&self) -> Option<mpsc::UnboundedReceiver<ServerEvent>> {
        self.event_receiver.lock().take()
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

    pub fn interest_manager(&self) -> Arc<InterestManager> {        self.interest_manager.clone()
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn interest_stats(&self) -> crate::interest::InterestStats {
        self.interest_manager.stats()
    }
}