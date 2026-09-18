use crate::connection::{Connection, ConnectionConfig, ConnectionState, ConnectionError, OutgoingPacket};
use crate::snapshot::{Snapshot, SnapshotBuffer, SnapshotInterpolator, EntitySnapshot, ProjectileSnapshot, LAYER_COUNT, LAYER_PROJECTILE, entity_layer, MAX_ENTITIES_PER_SNAPSHOT, MAX_PROJECTILES_PER_SNAPSHOT};
use crate::bandwidth::{BandwidthTracker, BandwidthLimiter, BandwidthStats};
use rfs_core::packet::*;
use rfs_core::time::{Tick, TICK_RATE, TICK_DURATION};
use bytes::BytesMut;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
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
    #[allow(dead_code)]
    bandwidth_limiter: BandwidthLimiter,
    current_tick: Arc<Mutex<Tick>>,
    server_tick: Arc<Mutex<Tick>>,
    server_time: Arc<Mutex<f64>>,
    rtt: Arc<Mutex<Duration>>,
    running: Arc<Mutex<bool>>,
    send_handle: Mutex<Option<JoinHandle<()>>>,
    recv_handle: Mutex<Option<JoinHandle<()>>>,
    event_sender: mpsc::UnboundedSender<ClientEvent>,
    event_receiver: Mutex<Option<mpsc::UnboundedReceiver<ClientEvent>>>,
    input_sequence: Arc<Mutex<u32>>,
    pending_inputs: Arc<Mutex<HashMap<u32, InputPacket>>>,
    last_acknowledged_tick: Arc<Mutex<u32>>,
    entity_states: Arc<RwLock<HashMap<EntityId, EntitySnapshot>>>,
    projectile_states: Arc<RwLock<HashMap<EntityId, ProjectileSnapshot>>>,
    /// Newest applied server tick per replication layer (plan §3.4). Layered
    /// deltas apply to the latest snapshot; stale ones are dropped.
    last_applied_layer: Arc<Mutex<[u64; LAYER_COUNT]>>,
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
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        
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
            bandwidth_limiter: BandwidthLimiter::new(config.max_bandwidth_bps),
            current_tick: Arc::new(Mutex::new(Tick(0))),
            server_tick: Arc::new(Mutex::new(Tick(0))),
            server_time: Arc::new(Mutex::new(0.0)),
            rtt: Arc::new(Mutex::new(Duration::from_millis(100))),
            running: Arc::new(Mutex::new(false)),
            send_handle: Mutex::new(None),
            recv_handle: Mutex::new(None),
            event_sender,
            event_receiver: Mutex::new(Some(event_receiver)),
            input_sequence: Arc::new(Mutex::new(0)),
            pending_inputs: Arc::new(Mutex::new(HashMap::new())),
            last_acknowledged_tick: Arc::new(Mutex::new(0)),
            entity_states: Arc::new(RwLock::new(HashMap::new())),
            projectile_states: Arc::new(RwLock::new(HashMap::new())),
            last_applied_layer: Arc::new(Mutex::new([0; LAYER_COUNT])),
        };
        
        Ok(client)
    }

    pub fn connect(&self) {
        *self.running.lock() = true;
        *self.connect_attempt.lock() = Some(Instant::now());
        self.start_receive_loop();
        self.start_send_loop();
        self.send_connect_request();
        info!("NetClient connecting to {}", self.config.server_addr);
    }

    pub fn disconnect(&self, reason: DisconnectReason) {
        *self.running.lock() = false;
        *self.connect_attempt.lock() = None;
        
        if self.connection.lock().take().is_some() {
            let packet = DisconnectPacket { reason };
            let header = PacketHeader::new(
                PacketType::Disconnect,
                ChannelType::ReliableOrdered,
                0, 0, 0, 0,
            );
            let socket = self.socket.clone();
            let bandwidth = self.bandwidth_tracker.clone();
            // Bug №68: the datagram used to be spawned into the background and
            // the tasks aborted a microsecond later — the Disconnect almost
            // never left, so the server only freed the slot via the 10 s
            // timeout. Give the single datagram time to flush first.
            let handle = tokio::spawn(async move {
                let _ = Self::send_packet_static(&socket, &bandwidth, &header, &packet).await;
            });
            std::thread::sleep(Duration::from_millis(50));
            handle.abort();
        }
        
        if let Some(handle) = self.send_handle.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.recv_handle.lock().take() {
            handle.abort();
        }
        
        info!("NetClient disconnected");
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
                    let _ = event_sender.send(ClientEvent::Error {
                        error: "connect timeout: no ConnectAccept".into(),
                    });
                    break;
                }
                let _ = Self::send_packet_static(&socket, &bandwidth, &header, &packet).await;
            }
        });
    }

    async fn send_packet_static(
        socket: &Arc<UdpSocket>,
        bandwidth: &BandwidthTracker,
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
        out: &OutgoingPacket,
    ) -> anyhow::Result<()> {
        let total_size = out.total_size();
        if total_size > MAX_PACKET_SIZE {
            return Err(anyhow::anyhow!(
                "packet too large: {total_size} > {MAX_PACKET_SIZE}"
            ));
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
        let running = self.running.clone();
        let event_sender = self.event_sender.clone();
        let current_tick = self.current_tick.clone();
        let server_tick = self.server_tick.clone();
        let server_time = self.server_time.clone();
        let rtt = self.rtt.clone();
        let pending_inputs = self.pending_inputs.clone();
        let last_ack_tick = self.last_acknowledged_tick.clone();
        let entity_states = self.entity_states.clone();
        let projectile_states = self.projectile_states.clone();
        let last_applied_layer = self.last_applied_layer.clone();
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
                            &current_tick,
                            &server_tick,
                            &server_time,
                            &rtt,
                            &pending_inputs,
                            &last_ack_tick,
                            &entity_states,
                            &projectile_states,
                            &last_applied_layer,
                            &running,
                            &bandwidth_tracker,
                            &connect_attempt,
                            data,
                        ).await {
                            warn!("Error handling packet: {}", e);
                            let _ = event_sender.send(ClientEvent::Error { error: e.to_string() });
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

    async fn handle_received_packet(
        socket: &Arc<UdpSocket>,
        connection: &Arc<Mutex<Option<Connection>>>,
        connection_id: &Arc<Mutex<Option<u32>>>,
        config: &ClientConfig,
        snapshot_buffer: &Arc<SnapshotBuffer>,
        snapshot_interpolator: &Arc<Mutex<SnapshotInterpolator>>,
        event_sender: &mpsc::UnboundedSender<ClientEvent>,
        current_tick: &Arc<Mutex<Tick>>,
        server_tick: &Arc<Mutex<Tick>>,
        server_time: &Arc<Mutex<f64>>,
        rtt: &Arc<Mutex<Duration>>,
        pending_inputs: &Arc<Mutex<HashMap<u32, InputPacket>>>,
        last_ack_tick: &Arc<Mutex<u32>>,
        entity_states: &Arc<RwLock<HashMap<EntityId, EntitySnapshot>>>,
        projectile_states: &Arc<RwLock<HashMap<EntityId, ProjectileSnapshot>>>,
        last_applied_layer: &Arc<Mutex<[u64; LAYER_COUNT]>>,
        running: &Arc<Mutex<bool>>,
        bandwidth: &BandwidthTracker,
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
        {
            let guard = connection.lock();
            if let Some(conn) = guard.as_ref() {
                conn.record_remote(header.sequence, header.ack, header.ack_bitfield);
            }
        }

        let packet_type = match PacketType::from_u8(header.packet_type) {
            Some(packet_type) => packet_type,
            None => return Err(ConnectionError::InvalidPacketType(header.packet_type)),
        };

        match packet_type {
PacketType::ConnectAccept => {
                let accept: ConnectAcceptPacket = bincode::deserialize(&payload)?;
                // Duplicate Accept (we retried, both got answered): refresh
                // state but emit Connected only once per session.
                let already = connection_id.lock().is_some();

                let mut conn_guard = connection.lock();
                let conn = conn_guard.take().unwrap_or_else(|| {
                    Connection::new(accept.assigned_client_id, config.server_addr, config.connection_config.clone())
                });
                conn.set_state(ConnectionState::Connected);
                conn.set_client_id(Some(accept.assigned_client_id));
                
                *server_tick.lock() = Tick(accept.server_tick);
                *server_time.lock() = accept.server_time;
                *connection_id.lock() = Some(accept.assigned_client_id);
                *conn_guard = Some(conn);
                *connect_attempt.lock() = None;
                // Bug №82: a fresh session owns a fresh tick space — never keep
                // stale pending inputs the new server cannot ack.
                pending_inputs.lock().clear();
                *last_ack_tick.lock() = 0;

                if !already {
                    let _ = event_sender.send(ClientEvent::Connected {
                        connection_id: accept.assigned_client_id,
                        server_tick: accept.server_tick,
                        match_id: accept.match_id,
                    });
                }
            }
            PacketType::ConnectReject => {
                let reject: ConnectRejectPacket = bincode::deserialize(&payload)?;
                *connect_attempt.lock() = None;
                let _ = event_sender.send(ClientEvent::ConnectionFailed { reason: reject.reason });
            }
            PacketType::Disconnect => {
                let disconnect: DisconnectPacket = bincode::deserialize(&payload)?;
                *running.lock() = false;
                let _ = event_sender.send(ClientEvent::Disconnected { reason: disconnect.reason });
            }
            PacketType::Heartbeat => {
                let _hb: HeartbeatPacket = bincode::deserialize(&payload)?;
                // The server pings us to measure ITS RTT on our ack — no send
                // stamp exists here, so we measure nothing and just answer.
                let ack_header = PacketHeader::new(
                    PacketType::HeartbeatAck,
                    ChannelType::ReliableOrdered,
                    0, header.sequence, 0, 0,
                );
                let ack_packet = HeartbeatPacket { client_time: 0.0, server_time: 0.0 };
                let _ = Self::send_packet_static(socket, bandwidth, &ack_header, &ack_packet).await;
            }
            PacketType::HeartbeatAck => {
                // Bug №12: measure RTT against the last heartbeat WE sent
                // (stamped in start_send_loop), never Instant::now().elapsed()
                // which is always ~0. No nested lock: conn and rtt are distinct.
                let sample = connection.lock().as_ref()
                    .map(|conn| conn.last_heartbeat.lock().elapsed());
                if let Some(sample) = sample {
                    let mut current = rtt.lock();
                    *current = Duration::from_millis(
                        ((current.as_millis() as u64 * 3 + sample.as_millis() as u64) / 4) as u64
                    );
                    let _ = event_sender.send(ClientEvent::RttUpdate { rtt: *current });
                }
            }
            PacketType::State | PacketType::StateDelta | PacketType::StateFull => {
                Self::process_state_packet(
                    packet_type,
                    payload,
                    snapshot_buffer,
                    snapshot_interpolator,
                    event_sender,
                    current_tick,
                    server_tick,
                    server_time,
                    entity_states,
                    projectile_states,
                    last_applied_layer,
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
                                current_tick,
                                server_tick,
                                server_time,
                                entity_states,
                                projectile_states,
                                last_applied_layer,
                            )?;
                        }
                        other => {
                            debug!("Fragment for unsupported packet type: {:?}", other);
                        }
                    }
                }
            }
            PacketType::Event => {
                let event_packet: EventPacket = bincode::deserialize(&payload)?;
                for event in event_packet.events {
                    let _ = event_sender.send(ClientEvent::GameEvent { event });
                }
            }
            PacketType::InputAck => {
                let ack: InputAckPacket = bincode::deserialize(&payload)?;
                // Bug №82/#71: the server echoes back input.tick — the very key
                // we stored under — and every ack releases that slot. A negative
                // ack (unknown ship/player) still consumes the entry; leaving it
                // accumulates an unbounded HashMap even at perfect link quality.
                pending_inputs.lock().remove(&ack.tick);
                if ack.accepted {
                    *last_ack_tick.lock() = ack.tick;
                }
                let _ = event_sender.send(ClientEvent::InputAck { tick: ack.tick, accepted: ack.accepted });
            }
            PacketType::CommandAck => {
                let ack: CommandAckPacket = bincode::deserialize(&payload)?;
                let _ = event_sender.send(ClientEvent::CommandAck {
                    command_id: ack.command_id,
                    success: ack.success,
                    error: ack.error,
                });
            }
            _ => {
                debug!("Unhandled packet type: {:?}", packet_type);
            }
        }
        
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn process_state_packet(
        packet_type: PacketType,
        payload: &[u8],
        snapshot_buffer: &Arc<SnapshotBuffer>,
        snapshot_interpolator: &Arc<Mutex<SnapshotInterpolator>>,
        event_sender: &mpsc::UnboundedSender<ClientEvent>,
        current_tick: &Arc<Mutex<Tick>>,
        server_tick: &Arc<Mutex<Tick>>,
        server_time: &Arc<Mutex<f64>>,
        entity_states: &Arc<RwLock<HashMap<EntityId, EntitySnapshot>>>,
        projectile_states: &Arc<RwLock<HashMap<EntityId, ProjectileSnapshot>>>,
        last_applied_layer: &Arc<Mutex<[u64; LAYER_COUNT]>>,
    ) -> Result<(), ConnectionError> {
        if packet_type == PacketType::State || packet_type == PacketType::StateFull {
            let state: StatePacket = bincode::deserialize(payload)?;
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
            *server_tick.lock() = Tick(state.server_tick as u64);
            *server_time.lock() = state.server_time;
            *current_tick.lock() = Tick(state.server_tick as u64);

            let snapshot = Snapshot::from(&state);

            snapshot_buffer.write_snapshot(snapshot.clone());
            snapshot_interpolator.lock().add_snapshot(snapshot.clone());
            *last_applied_layer.lock() = [state.server_tick as u64; LAYER_COUNT];

            // A full state is authoritative: prune ghosts the delta stream
            // may have left behind (destroyed while we were desynced).
            {
                let mut states = entity_states.write();
                states.retain(|id, _| snapshot.entities.iter().any(|e| &e.entity_id == id));
                for entity in &snapshot.entities {
                    states.insert(entity.entity_id, entity.clone());
                }
            }
            for entity in &snapshot.entities {
                let _ = event_sender.send(ClientEvent::EntityUpdate {
                    entity_id: entity.entity_id,
                    snapshot: entity.clone(),
                });
            }

            {
                let mut states = projectile_states.write();
                states.retain(|id, _| snapshot.projectiles.iter().any(|p| &p.entity_id == id));
                for proj in &snapshot.projectiles {
                    states.insert(proj.entity_id, proj.clone());
                }
            }

            for proj in &snapshot.projectiles {
                let _ = event_sender.send(ClientEvent::ProjectileUpdate {
                    projectile_id: proj.entity_id,
                    snapshot: proj.clone(),
                });
            }

            let _ = event_sender.send(ClientEvent::StateUpdate { snapshot });
        } else if packet_type == PacketType::StateDelta {
            let delta: StateDeltaPacket = bincode::deserialize(payload)?;
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
                return Ok(());
            }
            // Bug №25: an incremental delta is only valid on top of the exact
            // base tick the server diffed against. A mismatch means we lost an
            // intermediate delta — applying would corrupt the merge, so drop.
            // Bug №23/№78: a resync (full layer replacement, diffed against an
            // empty base) is applied regardless — it is the healing mechanism
            // for exactly the state we may have lost.
            if !delta.is_resync && delta.base_tick as u64 != last_applied_layer.lock()[layer] {
                debug!(
                    "dropping layer {} delta: base {} != applied {}",
                    layer, delta.base_tick, last_applied_layer.lock()[layer]
                );
                return Ok(());
            }
            *server_tick.lock() = Tick(delta.server_tick as u64);
            *server_time.lock() = delta.server_time;

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
                        let _ = event_sender.send(ClientEvent::EntityRemoved { entity_id: id });
                    }
                }

                {
                    let mut states = entity_states.write();
                    for id in &changed {
                        if let Some(entity) = target.entities.iter().find(|e| e.entity_id == *id) {
                            states.insert(*id, entity.clone());
                            let _ = event_sender.send(ClientEvent::EntityUpdate {
                                entity_id: *id,
                                snapshot: entity.clone(),
                            });
                        }
                    }
                }
                for entity_id in &delta.destroyed {
                    entity_states.write().remove(entity_id);
                    let _ = event_sender.send(ClientEvent::EntityRemoved { entity_id: *entity_id });
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
                            let _ = event_sender.send(ClientEvent::ProjectileRemoved { projectile_id: id });
                        }
                    }
                    for id in &changed_projectiles {
                        if let Some(proj) = target.projectiles.iter().find(|p| p.entity_id == *id) {
                            states.insert(*id, proj.clone());
                            let _ = event_sender.send(ClientEvent::ProjectileUpdate {
                                projectile_id: *id,
                                snapshot: proj.clone(),
                            });
                        }
                    }
                }
                for entity_id in &delta.projectile_destroyed {
                    projectile_states.write().remove(entity_id);
                    let _ = event_sender.send(ClientEvent::ProjectileRemoved { projectile_id: *entity_id });
                }

                let _ = event_sender.send(ClientEvent::StateUpdate { snapshot: target });
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
                            let ack = *conn.remote_sequence.lock();
                            let ack_bitfield = *conn.remote_ack_bitfield.lock();

                            let input = Self::build_input_packet(tick);
                            let seq = {
                                let mut iseq = input_sequence.lock();
                                *iseq = iseq.wrapping_add(1);
                                *iseq
                            };

                            // Bug №82: store under input.tick — the very value
                            // the server echoes back in ack.tick (main.rs).
                            pending_inputs.lock().insert(input.tick, input.clone());

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
                    let _ = Self::send_packet_static(&socket, &bandwidth_tracker, &header, &input).await;
                }
                if let Some(hb) = heartbeat {
                    let _ = Self::send_raw(&socket, &bandwidth_tracker, &hb).await;
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
        self.pending_inputs.lock().insert(input.tick, input.clone());

        // Bug №77: fill real acks so the server can GC its reliable queue.
        let (ack, ack_bitfield) = self.connection.lock().as_ref()
            .map(|c| (*c.remote_sequence.lock(), *c.remote_ack_bitfield.lock()))
            .unwrap_or((0, 0));

        let header = PacketHeader::new(
            PacketType::Input,
            ChannelType::UnreliableSequenced,
            seq,
            ack, ack_bitfield, 0,
        );
        
        let socket = self.socket.clone();
        let bandwidth = self.bandwidth_tracker.clone();
        tokio::spawn(async move {
            let _ = Self::send_packet_static(&socket, &bandwidth, &header, &input).await;
        });
    }

    pub fn send_command(&self, command: ServerCommand) {
        if let Some(conn) = self.connection.lock().as_mut() {
            let command_id = {
                let id = conn.next_sequence();
                id as u64
            };
            
            let packet = CommandPacket { command_id, command };
            // Bug №77: ack the server's sequence high-water mark too.
            let (ack, ack_bitfield) = ( *conn.remote_sequence.lock(), *conn.remote_ack_bitfield.lock() );
            let header = PacketHeader::new(
                PacketType::Command,
                ChannelType::ReliableOrdered,
                conn.next_sequence(),
                ack, ack_bitfield, 0,
            );
            
            let socket = self.socket.clone();
            let bandwidth = self.bandwidth_tracker.clone();
            tokio::spawn(async move {
                let _ = Self::send_packet_static(&socket, &bandwidth, &header, &packet).await;
            });
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

    pub fn get_event_receiver(&self) -> Option<mpsc::UnboundedReceiver<ClientEvent>> {
        self.event_receiver.lock().take()
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