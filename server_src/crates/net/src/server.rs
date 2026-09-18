use crate::connection::{Connection, ConnectionConfig, ConnectionState, OutgoingPacket};
use crate::interest::{InterestManager};
use crate::snapshot::{Snapshot, SnapshotBuffer, DeltaCompressor, EntitySnapshot, ProjectileSnapshot, LayerBase, LAYER_COUNT, LAYER_INTERVAL_TICKS, LAYER_RESYNC_TICKS, entity_layer};
use crate::bandwidth::{BandwidthTracker, BandwidthLimiter, BandwidthStats};
use rfs_core::packet::*;
use rfs_core::entity::{ShipEntity, PlayerEntity};
use rfs_core::spatial::InterestConfig;
use rfs_core::time::{Tick, TICK_RATE, TICK_DURATION};
use bytes::BytesMut;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn, error};
use uuid::Uuid;

pub struct NetServer {
    socket: Arc<UdpSocket>,
    config: ServerConfig,
    connections: Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
    connection_by_addr: Arc<RwLock<HashMap<SocketAddr, u32>>>,
    next_connection_id: Arc<Mutex<u32>>,
    snapshot_buffer: Arc<SnapshotBuffer>,
    delta_compressor: Arc<DeltaCompressor>,
    client_layers: Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
    interest_manager: Arc<InterestManager>,
    bandwidth_tracker: Arc<BandwidthTracker>,
    bandwidth_limiter: Arc<BandwidthLimiter>,
    tick_rate: u32,
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
}

/// Token bucket for inbound Connects from one source address (bug №27).
/// Burst 4, refill 4/sec: a legitimate client (1 Connect + 250 ms retries
/// while the Accept is in flight) never notices; a sprayer is capped.
const CONNECT_BURST: f64 = 4.0;
const CONNECT_REFILL_PER_SEC: f64 = 4.0;
/// Upper bound for the limiter table itself; oldest entries are evicted past it.
const CONNECT_TABLE_CAP: usize = 4096;

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
        let match_id = Uuid::new_v4();
        
        let server = Self {
            socket,
            config: config.clone(),
            connections: Arc::new(RwLock::new(HashMap::new())),
            connection_by_addr: Arc::new(RwLock::new(HashMap::new())),
            next_connection_id: Arc::new(Mutex::new(1)),
            snapshot_buffer: Arc::new(SnapshotBuffer::new(config.snapshot_history)),
            delta_compressor: Arc::new(DeltaCompressor::new()),
            client_layers: Arc::new(RwLock::new(HashMap::new())),
            interest_manager: Arc::new(InterestManager::new(config.interest_config)),
            bandwidth_tracker: Arc::new(BandwidthTracker::new()),
            bandwidth_limiter: Arc::new(BandwidthLimiter::new(config.max_bandwidth_bps)),
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
        };
        
        Ok(server)
    }

    pub fn match_id(&self) -> Uuid {
        self.match_id
    }

    pub fn current_tick(&self) -> Tick {
        *self.current_tick.lock()
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
        
        self.disconnect_all(DisconnectReason::ServerShutdown);
        info!("NetServer stopped");
    }

    fn start_receive_loop(&self) {
        let socket = self.socket.clone();
        let connections = self.connections.clone();
        let connection_by_addr = self.connection_by_addr.clone();
        let next_id = self.next_connection_id.clone();
        let config = self.config.clone();
        let bandwidth_tracker = self.bandwidth_tracker.clone();
        let bandwidth_limiter = self.bandwidth_limiter.clone();
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
                            &bandwidth_limiter,
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

    async fn handle_received_packet(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        connection_by_addr: &Arc<RwLock<HashMap<SocketAddr, u32>>>,
        next_id: &Arc<Mutex<u32>>,
        config: &ServerConfig,
        bandwidth: &BandwidthTracker,
        limiter: &BandwidthLimiter,
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

                        let conn = Connection::new(id, addr, config.connection_config.clone());
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
                    Self::send_packet(socket, &conn_arc, &accept, bandwidth, limiter, limit_enabled).await.map(|_| ())?;
                }

                id
            }
        };

        let conn = connections.read().get(&connection_id).cloned();

        if let Some(conn) = conn {
            let packets = conn.handle_packet(header, payload)?;
            
            for packet in packets {
                Self::send_packet(socket, &conn, &packet, bandwidth, limiter, limit_enabled).await.map(|_| ())?;
            }

            match PacketType::from_u8(header.packet_type) {
                Some(PacketType::Input) => {
                    if let Ok(input) = bincode::deserialize::<InputPacket>(payload) {
                        let _ = event_sender.send(ServerEvent::ClientInput { connection_id, input });
                    }
                }
                Some(PacketType::Command) => {
                    if let Ok(command) = bincode::deserialize::<CommandPacket>(payload) {
                        let _ = event_sender.send(ServerEvent::ClientCommand { connection_id, command });
                    }
                }
                Some(PacketType::Connect) => {
                    // Duplicate Connect: our Accept never made it back.
                    // Registration is keyed by addr, so re-sending is idempotent.
                    let accept = {
                        let tick = current_tick.lock().value();
                        let time = *server_time.lock();
                        conn.send_connect_accept(tick, time, match_id)
                    };
                    if let Some(accept) = accept {
                        Self::send_packet(socket, &conn, &accept, bandwidth, limiter, limit_enabled).await.map(|_| ())?;
                    }
                }
                Some(PacketType::Fragment) => {
                    if let Some((orig_type, bytes)) = conn.reassemble_fragment(payload) {
                        match orig_type {
                            PacketType::Input => {
                                if let Ok(input) = bincode::deserialize::<InputPacket>(&bytes) {
                                    let _ = event_sender.send(ServerEvent::ClientInput { connection_id, input });
                                }
                            }
                            PacketType::Command => {
                                if let Ok(command) = bincode::deserialize::<CommandPacket>(&bytes) {
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
                    let reason = bincode::deserialize::<DisconnectPacket>(payload)
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
        limiter: &BandwidthLimiter,
        limit_enabled: bool,
    ) -> anyhow::Result<bool> {
        let total_size = HEADER_SIZE + packet.payload.len();
        // Bug №30: the MTU comes from the connection's own config — the same
        // value the packet builders fragment against — not a stray constant.
        let max = conn.config.max_packet_size;
        if total_size > max {
            return Err(anyhow::anyhow!("Packet too large: {} > {}", total_size, max));
        }
        // Bug №39: the limiter finally gates the wire. Drops are silent by
        // design (unreliable channel resends); oversize is still an error.
        if limit_enabled && !limiter.try_consume(total_size) {
            return Ok(false);
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
        let delta_compressor = self.delta_compressor.clone();
        let interest_manager = self.interest_manager.clone();
        let bandwidth_tracker = self.bandwidth_tracker.clone();
        let bandwidth_limiter = self.bandwidth_limiter.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;
        let event_sender = self.event_sender.clone();
        let running = self.running.clone();
        let current_tick = self.current_tick.clone();
        let server_time = self.server_time.clone();
        let tick_rate = self.tick_rate;

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1000 / tick_rate as u64));

            while *running.lock() {
                interval.tick().await;

                let tick = {
                    let mut ct = current_tick.lock();
                    *ct = ct.next();
                    *ct
                };

                let time = tick.value() as f64 * TICK_DURATION.as_secs_f64();
                *server_time.lock() = time;

                // Bug №28: reap silent connections once a second so dead
                // clients stop consuming snapshots, interest and ids.
                if tick.value() % tick_rate as u64 == 0 {
                    Self::sweep_timeouts(
                        &connections,
                        &connection_by_addr,
                        &delta_compressor,
                        &client_layers,
                        &event_sender,
                    );
                }

                let snapshot = Self::build_snapshot(tick, time, &interest_manager);
                snapshot_buffer.write_snapshot(snapshot.clone());

                Self::send_snapshots(
                    &socket,
                    &connections,
                    &client_layers,
                    &snapshot_buffer,
                    &delta_compressor,
                    &interest_manager,
                    &bandwidth_tracker,
                    &bandwidth_limiter,
                    limit_enabled,
                    tick,
                ).await;

                Self::send_heartbeats(&socket, &connections, &bandwidth_tracker, &bandwidth_limiter, limit_enabled).await;
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

    async fn send_snapshots(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        client_layers: &Arc<RwLock<HashMap<u32, [LayerBase; LAYER_COUNT]>>>,
        snapshot_buffer: &Arc<SnapshotBuffer>,
        delta_compressor: &Arc<DeltaCompressor>,
        interest_manager: &Arc<InterestManager>,
        bandwidth: &BandwidthTracker,
        limiter: &BandwidthLimiter,
        limit_enabled: bool,
        current_tick: Tick,
    ) {
        let snapshot = snapshot_buffer.get_latest().unwrap_or_else(|| Snapshot {
            tick: current_tick,
            time: current_tick.value() as f64 * TICK_DURATION.as_secs_f64(),
            entities: Vec::new(),
            projectiles: Vec::new(),
            events: Vec::new(),
        });

        let mut pending_sends: Vec<(Arc<Connection>, OutgoingPacket)> = Vec::new();

        // Bug №29: snapshot the client list first, then take the layers lock
        // per client and only for map access. Serialization, fragmentation
        // and delta diffing (the expensive parts at 200 clients) run without
        // any global lock held, so disconnects/timeouts never stall behind them.
        let clients: Vec<(u32, Arc<Connection>)> = {
            let conns = connections.read();
            conns.iter().map(|(id, conn)| (*id, conn.clone())).collect()
        };

        for (client_id, conn) in &clients {
            if conn.state() != ConnectionState::Connected {
                continue;
            }

            let filtered = Self::filter_snapshot(&snapshot, interest_manager, *client_id);
            let tick = filtered.tick.value();

            {
                let mut layers = client_layers.write();
                layers.entry(*client_id).or_insert_with(|| {
                    // First contact: full state now, layer bases start from it.
                    for packet in conn.send_state(filtered.to_state_packet()) {
                        pending_sends.push((conn.clone(), packet));
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

                // Layer 1 has no fixed rate: it goes out on change, plus resync.
                let scheduled = since >= LAYER_INTERVAL_TICKS[layer];
                let resync = since >= LAYER_RESYNC_TICKS[layer] && !content_empty;
                let on_change = LAYER_INTERVAL_TICKS[layer] == u64::MAX;
                if !scheduled && !resync && !on_change {
                    continue;
                }

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

                for packet in conn.send_state_delta(state_delta) {
                    pending_sends.push((conn.clone(), packet));
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
        }

        for (conn, packet) in pending_sends {
            let _ = Self::send_packet(socket, &conn, &packet, bandwidth, limiter, limit_enabled).await;
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
        limiter: &BandwidthLimiter,
        limit_enabled: bool,
    ) {
        let mut pending_heartbeats: Vec<(Arc<Connection>, OutgoingPacket)> = Vec::new();

        {
            let conns = connections.read();

            for conn in conns.values() {
                if conn.state() == ConnectionState::Connected && conn.should_send_heartbeat() {
                    if let Some(packet) = conn.send_heartbeat() {
                        pending_heartbeats.push((conn.clone(), packet));
                    }
                }
            }
        }

        for (conn, packet) in pending_heartbeats {
            let _ = Self::send_packet(socket, &conn, &packet, bandwidth, limiter, limit_enabled).await;
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
        let conns = self.connections.read();
        let limit_enabled = self.config.enable_bandwidth_limit;

        for conn in conns.values() {
            if conn.state() == ConnectionState::Connected {
                let packets = conn.send_event(event_packet.clone());
                if packets.is_empty() {
                    continue;
                }
                // Bug №32: one task per connection draining all its packets —
                // never one spawned task per (conn, fragment), which floods
                // the scheduler on mass events.
                let socket = self.socket.clone();
                let conn_clone = conn.clone();
                let bandwidth = self.bandwidth_tracker.clone();
                let limiter = self.bandwidth_limiter.clone();
                tokio::spawn(async move {
                    for packet in packets {
                        let _ = Self::send_packet(&socket, &conn_clone, &packet, &bandwidth, &limiter, limit_enabled).await;
                    }
                });
            }
        }
    }

    pub fn send_command_ack(&self, connection_id: u32, command_id: u64, success: bool, error: Option<String>) {
        if let Some(conn) = self.get_connection(connection_id) {
            let packets = conn.send_command_ack(command_id, success, error);
            if packets.is_empty() {
                return;
            }
            let socket = self.socket.clone();
            let conn_clone = conn.clone();
            let bandwidth = self.bandwidth_tracker.clone();
            let limiter = self.bandwidth_limiter.clone();
            let limit_enabled = self.config.enable_bandwidth_limit;
            tokio::spawn(async move {
                for packet in packets {
                    let _ = Self::send_packet(&socket, &conn_clone, &packet, &bandwidth, &limiter, limit_enabled).await;
                }
            });
        }
    }

    pub fn send_input_ack(&self, connection_id: u32, ack: InputAckPacket) {
        if let Some(conn) = self.get_connection(connection_id) {
            let packets = conn.send_input_ack(ack);
            if packets.is_empty() {
                return;
            }
            let socket = self.socket.clone();
            let conn_clone = conn.clone();
            let bandwidth = self.bandwidth_tracker.clone();
            let limiter = self.bandwidth_limiter.clone();
            let limit_enabled = self.config.enable_bandwidth_limit;
            tokio::spawn(async move {
                for packet in packets {
                    let _ = Self::send_packet(&socket, &conn_clone, &packet, &bandwidth, &limiter, limit_enabled).await;
                }
            });
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
        let socket = self.socket.clone();
        let conn_clone = conn.clone();
        let bandwidth = self.bandwidth_tracker.clone();
        let limiter = self.bandwidth_limiter.clone();
        let limit_enabled = self.config.enable_bandwidth_limit;

        tokio::spawn(async move {
            let packet = DisconnectPacket { reason };
            let header = PacketHeader::new(
                PacketType::Disconnect,
                ChannelType::ReliableOrdered,
                conn_clone.next_sequence(),
                0, 0, 0,
            );
            if let Some(out) = OutgoingPacket::new(header, packet) {
                let _ = Self::send_packet(&socket, &conn_clone, &out, &bandwidth, &limiter, limit_enabled).await;
            }
        });

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