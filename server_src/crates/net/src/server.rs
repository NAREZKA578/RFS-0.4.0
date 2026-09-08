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
use std::time::Duration;
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
    bandwidth_tracker: BandwidthTracker,
    #[allow(dead_code)]
    bandwidth_limiter: BandwidthLimiter,
    tick_rate: u32,
    current_tick: Arc<Mutex<Tick>>,
    server_time: Arc<Mutex<f64>>,
    running: Arc<Mutex<bool>>,
    send_handle: Mutex<Option<JoinHandle<()>>>,
    recv_handle: Mutex<Option<JoinHandle<()>>>,
    event_sender: mpsc::UnboundedSender<ServerEvent>,
    event_receiver: Mutex<Option<mpsc::UnboundedReceiver<ServerEvent>>>,
    match_id: Uuid,
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
            bandwidth_tracker: BandwidthTracker::new(),
            bandwidth_limiter: BandwidthLimiter::new(config.max_bandwidth_bps),
            tick_rate: config.tick_rate,
            current_tick: Arc::new(Mutex::new(Tick(0))),
            server_time: Arc::new(Mutex::new(0.0)),
            running: Arc::new(Mutex::new(false)),
            send_handle: Mutex::new(None),
            recv_handle: Mutex::new(None),
            event_sender,
            event_receiver: Mutex::new(Some(event_receiver)),
            match_id,
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
        self.connection_by_addr.read()
            .get(&addr)
            .and_then(|id| self.connections.read().get(id).cloned())
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
        let bandwidth_tracker = BandwidthTracker::new();
        let running = self.running.clone();
        let event_sender = self.event_sender.clone();
        let current_tick = self.current_tick.clone();
        let server_time = self.server_time.clone();
        let match_id = self.match_id;

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
                            &event_sender,
                            &current_tick,
                            &server_time,
                            match_id,
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

    async fn handle_received_packet(
        socket: &Arc<UdpSocket>,
        connections: &Arc<RwLock<HashMap<u32, Arc<Connection>>>>,
        connection_by_addr: &Arc<RwLock<HashMap<SocketAddr, u32>>>,
        next_id: &Arc<Mutex<u32>>,
        config: &ServerConfig,
        event_sender: &mpsc::UnboundedSender<ServerEvent>,
        current_tick: &Arc<Mutex<Tick>>,
        server_time: &Arc<Mutex<f64>>,
        match_id: Uuid,
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
                if connections.read().len() >= config.max_connections {
                    let reject = ConnectRejectPacket { reason: ConnectRejectReason::ServerFull };
                    let header = PacketHeader::new(
                        PacketType::ConnectReject,
                        ChannelType::ReliableOrdered,
                        0, 0, 0, 0,
                    );
                    let packet_data = serialize_packet(&reject, header)?;
                    socket.send_to(&packet_data, addr).await?;
                    return Ok(());
                }

                let id = {
                    let mut id_gen = next_id.lock();
                    let id = *id_gen;
                    *id_gen = id_gen.wrapping_add(1);
                    if *id_gen == 0 { *id_gen = 1; }
                    id
                };

                let conn = Connection::new(id, addr, config.connection_config.clone());
                conn.set_state(ConnectionState::Connected);

                let accept = {
                    let tick = current_tick.lock().value();
                    let time = *server_time.lock();
                    conn.send_connect_accept(tick, time, match_id)
                };

                let conn_arc = Arc::new(conn);
                connections.write().insert(id, conn_arc.clone());
                connection_by_addr.write().insert(addr, id);

                let _ = event_sender.send(ServerEvent::ClientConnected { connection_id: id, addr });

                Self::send_packet(socket, &conn_arc, &accept).await?;

                id
            }
        };

        let conn = connections.read().get(&connection_id).cloned();

        if let Some(conn) = conn {
            let packets = conn.handle_packet(header, payload)?;
            
            for packet in packets {
                Self::send_packet(socket, &conn, &packet).await?;
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
                _ => {}
            }
        }

        Ok(())
    }

    async fn send_packet(
        socket: &Arc<UdpSocket>,
        conn: &Connection,
        packet: &OutgoingPacket,
    ) -> anyhow::Result<()> {
        let total_size = HEADER_SIZE + packet.payload.len();
        if total_size > MAX_PACKET_SIZE {
            return Err(anyhow::anyhow!("Packet too large: {} > {}", total_size, MAX_PACKET_SIZE));
        }

        let mut buffer = BytesMut::with_capacity(total_size);
        buffer.extend_from_slice(&bincode::serialize(&packet.header)?);
        buffer.extend_from_slice(&packet.payload);
        
        socket.send_to(&buffer, conn.addr).await?;
        Ok(())
    }

    fn start_send_loop(&self) {
        let socket = self.socket.clone();
        let connections = self.connections.clone();
        let client_layers = self.client_layers.clone();
        let snapshot_buffer = self.snapshot_buffer.clone();
        let delta_compressor = self.delta_compressor.clone();
        let interest_manager = self.interest_manager.clone();
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
                
                let snapshot = Self::build_snapshot(tick, time, &interest_manager);
                snapshot_buffer.write_snapshot(snapshot.clone());
                
                Self::send_snapshots(
                    &socket,
                    &connections,
                    &client_layers,
                    &snapshot_buffer,
                    &delta_compressor,
                    &interest_manager,
                    tick,
                ).await;
                
                Self::send_heartbeats(&socket, &connections).await;
            }
        });
        
        *self.send_handle.lock() = Some(handle);
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

        {
            let conns = connections.read();
            let mut layers = client_layers.write();

            for (client_id, conn) in conns.iter() {
                if conn.state() != ConnectionState::Connected {
                    continue;
                }

                let filtered = Self::filter_snapshot(&snapshot, interest_manager, *client_id);
                let tick = filtered.tick.value();

                let entry = layers.entry(*client_id).or_insert_with(|| {
                    // First contact: full state now, layer bases start from it.
                    for packet in conn.send_state(filtered.to_state_packet()) {
                        pending_sends.push((conn.clone(), packet));
                    }
                    Self::split_layers(&filtered, tick)
                });

                for layer in 0..LAYER_COUNT {
                    let layer_u8 = layer as u8;
                    let base = &entry[layer];
                    let since = tick.saturating_sub(base.last_sent_tick);

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
                        tick: Tick(base.last_sent_tick),
                        time: filtered.time,
                        entities: if resync { Vec::new() } else { base.entities.clone() },
                        projectiles: if resync { Vec::new() } else { base.projectiles.clone() },
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

                    for packet in conn.send_state_delta(delta.to_state_delta()) {
                        pending_sends.push((conn.clone(), packet));
                    }
                    entry[layer] = LayerBase {
                        entities: cur_entities,
                        projectiles: cur_projectiles,
                        last_sent_tick: tick,
                    };
                }
            }
        }

        for (conn, packet) in pending_sends {
            let _ = Self::send_packet(socket, &conn, &packet).await;
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
    ) {
        let mut pending_heartbeats: Vec<(Arc<Connection>, OutgoingPacket)> = Vec::new();

        {
            let conns = connections.read();

            for conn in conns.values() {
                if conn.state() == ConnectionState::Connected && conn.should_send_heartbeat() {
                    pending_heartbeats.push((conn.clone(), conn.send_heartbeat()));
                }
            }
        }

        for (conn, packet) in pending_heartbeats {
            let _ = Self::send_packet(socket, &conn, &packet).await;
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
        
        for conn in conns.values() {
            if conn.state() == ConnectionState::Connected {
                let packets = conn.send_event(event_packet.clone());
                for packet in packets {
                    let socket = self.socket.clone();
                    let conn_clone = conn.clone();
                    tokio::spawn(async move {
                        let _ = Self::send_packet(&socket, &conn_clone, &packet).await;
                    });
                }
            }
        }
    }

    pub fn send_command_ack(&self, connection_id: u32, command_id: u64, success: bool, error: Option<String>) {
        if let Some(conn) = self.get_connection(connection_id) {
            let packet = conn.send_command_ack(command_id, success, error);
            let socket = self.socket.clone();
            let conn_clone = conn.clone();
            tokio::spawn(async move {
                let _ = Self::send_packet(&socket, &conn_clone, &packet).await;
            });
        }
    }

    pub fn send_input_ack(&self, connection_id: u32, ack: InputAckPacket) {
        if let Some(conn) = self.get_connection(connection_id) {
            let packet = conn.send_input_ack(ack);
            let socket = self.socket.clone();
            let conn_clone = conn.clone();
            tokio::spawn(async move {
                let _ = Self::send_packet(&socket, &conn_clone, &packet).await;
            });
        }
    }

    pub fn disconnect_client(&self, connection_id: u32, reason: DisconnectReason) {
        let conn = self.get_connection(connection_id);
        if let Some(conn) = conn {
            let socket = self.socket.clone();
            let conn_clone = conn.clone();
            
            tokio::spawn(async move {
                let packet = DisconnectPacket { reason };
                let header = PacketHeader::new(
                    PacketType::Disconnect,
                    ChannelType::ReliableOrdered,
                    conn_clone.next_sequence(),
                    0, 0, 0,
                );
                let _ = Self::send_packet(&socket, &conn_clone, &OutgoingPacket::new(header, packet)).await;
            });
            
            self.connections.write().remove(&connection_id);
            self.connection_by_addr.write().remove(&conn.addr);
            self.delta_compressor.remove_client(connection_id);
            self.client_layers.write().remove(&connection_id);
            
            let _ = self.event_sender.send(ServerEvent::ClientDisconnected {
                connection_id,
                reason,
            });
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
            self.disconnect_client(id, DisconnectReason::Timeout);
            let _ = self.event_sender.send(ServerEvent::ClientTimeout { connection_id: id });
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