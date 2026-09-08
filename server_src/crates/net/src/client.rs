use crate::connection::{Connection, ConnectionConfig, ConnectionState, ConnectionError};
use crate::snapshot::{Snapshot, SnapshotBuffer, SnapshotInterpolator, EntitySnapshot, ProjectileSnapshot, LAYER_COUNT};
use crate::bandwidth::{BandwidthTracker, BandwidthLimiter, BandwidthStats};
use rfs_core::packet::*;
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
use tracing::{debug, info, warn, error};
use uuid::Uuid;

pub struct NetClient {
    socket: Arc<UdpSocket>,
    config: ClientConfig,
    #[allow(dead_code)]
    server_addr: SocketAddr,
    connection: Arc<Mutex<Option<Connection>>>,
    connection_id: Arc<Mutex<Option<u32>>>,
    snapshot_buffer: Arc<SnapshotBuffer>,
    snapshot_interpolator: Arc<Mutex<SnapshotInterpolator>>,
    bandwidth_tracker: BandwidthTracker,
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
            snapshot_buffer: Arc::new(SnapshotBuffer::new(config.snapshot_history)),
            snapshot_interpolator: Arc::new(Mutex::new(SnapshotInterpolator::new(config.snapshot_history))),
            bandwidth_tracker: BandwidthTracker::new(),
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
        self.start_receive_loop();
        self.start_send_loop();
        self.send_connect_request();
        info!("NetClient connecting to {}", self.config.server_addr);
    }

    pub fn disconnect(&self, reason: DisconnectReason) {
        *self.running.lock() = false;
        
        if self.connection.lock().take().is_some() {
            let packet = DisconnectPacket { reason };
            let header = PacketHeader::new(
                PacketType::Disconnect,
                ChannelType::ReliableOrdered,
                0, 0, 0, 0,
            );
            let socket = self.socket.clone();
            tokio::spawn(async move {
                let _ = Self::send_packet_static(&socket, &header, &packet).await;
            });
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
        tokio::spawn(async move {
            let _ = Self::send_packet_static(&socket, &header, &packet).await;
        });
    }

    async fn send_packet_static(
        socket: &Arc<UdpSocket>,
        header: &PacketHeader,
        packet: &impl Serializable,
    ) -> anyhow::Result<()> {
        let payload = bincode::serialize(packet).unwrap_or_default();
        let mut header = *header;
        header.payload_size = payload.len() as u16;
        let total_size = HEADER_SIZE + payload.len();
        
        let mut buffer = BytesMut::with_capacity(total_size);
        buffer.extend_from_slice(&bincode::serialize(&header)?);
        buffer.extend_from_slice(&payload);
        
        socket.send(&buffer).await?;
        Ok(())
    }

    fn start_receive_loop(&self) {
        let socket = self.socket.clone();
        let connection = self.connection.clone();
        let connection_id = self.connection_id.clone();
        let config = self.config.clone();
        let snapshot_buffer = self.snapshot_buffer.clone();
        let snapshot_interpolator = self.snapshot_interpolator.clone();
        let bandwidth_tracker = BandwidthTracker::new();
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
        data: &[u8],
    ) -> Result<(), ConnectionError> {
        if data.len() < HEADER_SIZE {
            return Err(ConnectionError::PacketTooLarge(data.len(), HEADER_SIZE));
        }

        let (header, payload) = deserialize_packet(data)?;

        let packet_type = match PacketType::from_u8(header.packet_type) {
            Some(packet_type) => packet_type,
            None => return Err(ConnectionError::InvalidPacketType(header.packet_type)),
        };

        match packet_type {
            PacketType::ConnectAccept => {
                let accept: ConnectAcceptPacket = bincode::deserialize(&payload)?;
                
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
                
                let _ = event_sender.send(ClientEvent::Connected {
                    connection_id: accept.assigned_client_id,
                    server_tick: accept.server_tick,
                    match_id: accept.match_id,
                });
            }
            PacketType::ConnectReject => {
                let reject: ConnectRejectPacket = bincode::deserialize(&payload)?;
                let _ = event_sender.send(ClientEvent::ConnectionFailed { reason: reject.reason });
            }
            PacketType::Disconnect => {
                let disconnect: DisconnectPacket = bincode::deserialize(&payload)?;
                *running.lock() = false;
                let _ = event_sender.send(ClientEvent::Disconnected { reason: disconnect.reason });
            }
            PacketType::Heartbeat => {
                let _hb: HeartbeatPacket = bincode::deserialize(&payload)?;
                let rtt_duration = Instant::now().elapsed();
                // NOTE: single lock acquisition — parking_lot Mutex is not
                // reentrant, a nested rtt.lock() here deadlocks the thread.
                let rtt_value = {
                    let mut guard = rtt.lock();
                    *guard = Duration::from_millis(
                        ((guard.as_millis() as u64 * 3 + rtt_duration.as_millis() as u64) / 4)
                            as u64,
                    );
                    *guard
                };

                let ack_header = PacketHeader::new(
                    PacketType::HeartbeatAck,
                    ChannelType::ReliableOrdered,
                    0, header.sequence, 0, 0,
                );
                let ack_packet = HeartbeatPacket { client_time: 0.0, server_time: 0.0 };
                let _ = Self::send_packet_static(socket, &ack_header, &ack_packet).await;

                let _ = event_sender.send(ClientEvent::RttUpdate { rtt: rtt_value });
            }
            PacketType::HeartbeatAck => {
                let rtt_duration = Instant::now().elapsed();
                // NOTE: single lock acquisition — see Heartbeat arm above.
                let mut guard = rtt.lock();
                *guard = Duration::from_millis(
                    ((guard.as_millis() as u64 * 3 + rtt_duration.as_millis() as u64) / 4) as u64
                );
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
                if ack.accepted {
                    *last_ack_tick.lock() = ack.tick;
                    pending_inputs.lock().remove(&ack.tick);
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
            *server_tick.lock() = Tick(state.server_tick as u64);
            *server_time.lock() = state.server_time;
            *current_tick.lock() = Tick(state.server_tick as u64);

            let snapshot = Snapshot::from(&state);

            snapshot_buffer.write_snapshot(snapshot.clone());
            snapshot_interpolator.lock().add_snapshot(snapshot.clone());
            *last_applied_layer.lock() = [state.server_tick as u64; LAYER_COUNT];

            for entity in &snapshot.entities {
                entity_states.write().insert(entity.entity_id, entity.clone());
                let _ = event_sender.send(ClientEvent::EntityUpdate {
                    entity_id: entity.entity_id,
                    snapshot: entity.clone(),
                });
            }

            for proj in &snapshot.projectiles {
                projectile_states.write().insert(proj.entity_id, proj.clone());
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
            // Layers are independent streams: drop stale/out-of-order deltas,
            // apply fresh ones on top of the latest snapshot.
            if delta.server_tick as u64 <= last_applied_layer.lock()[layer] {
                return Ok(());
            }
            *server_tick.lock() = Tick(delta.server_tick as u64);
            *server_time.lock() = delta.server_time;

            if let Some(latest) = snapshot_buffer.get_latest() {
                let target = latest.apply_delta(&delta);
                snapshot_buffer.write_snapshot(target.clone());
                snapshot_interpolator.lock().add_snapshot(target.clone());
                last_applied_layer.lock()[layer] = delta.server_tick as u64;

                for entity in &target.entities {
                    entity_states.write().insert(entity.entity_id, entity.clone());
                    let _ = event_sender.send(ClientEvent::EntityUpdate {
                        entity_id: entity.entity_id,
                        snapshot: entity.clone(),
                    });
                }

                for entity_id in &delta.destroyed {
                    entity_states.write().remove(entity_id);
                    let _ = event_sender.send(ClientEvent::EntityRemoved { entity_id: *entity_id });
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
        let tick_rate = self.config.tick_rate;

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1000 / tick_rate as u64));
            
            while *running.lock() {
                interval.tick().await;
                
                let tick = {
                    let mut ct = current_tick.lock();
                    *ct = ct.next();
                    *ct
                };
                
                let pending = {
                    let conn_guard = connection.lock();
                    if conn_guard.as_ref().map(|c| c.state()) == Some(ConnectionState::Connected) {
                        let input = Self::build_input_packet(tick);
                        let seq = {
                            let mut iseq = input_sequence.lock();
                            *iseq = iseq.wrapping_add(1);
                            *iseq
                        };
                        
                        pending_inputs.lock().insert(seq, input.clone());
                        
                        let header = PacketHeader::new(
                            PacketType::Input,
                            ChannelType::UnreliableSequenced,
                            seq,
                            0, 0, 0,
                        );
                        Some((header, input))
                    } else {
                        None
                    }
                };
                
                if let Some((header, input)) = pending {
                    let _ = Self::send_packet_static(&socket, &header, &input).await;
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
        
        self.pending_inputs.lock().insert(seq, input.clone());
        
        let header = PacketHeader::new(
            PacketType::Input,
            ChannelType::UnreliableSequenced,
            seq,
            0, 0, 0,
        );
        
        let socket = self.socket.clone();
        tokio::spawn(async move {
            let _ = Self::send_packet_static(&socket, &header, &input).await;
        });
    }

    pub fn send_command(&self, command: ServerCommand) {
        if let Some(conn) = self.connection.lock().as_mut() {
            let command_id = {
                let id = conn.next_sequence();
                id as u64
            };
            
            let packet = CommandPacket { command_id, command };
            let header = PacketHeader::new(
                PacketType::Command,
                ChannelType::ReliableOrdered,
                conn.next_sequence(),
                0, 0, 0,
            );
            
            let socket = self.socket.clone();
            tokio::spawn(async move {
                let _ = Self::send_packet_static(&socket, &header, &packet).await;
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