use crate::bandwidth::BandwidthTracker;
use crate::channel::{Channel, ChannelConfig};
use crate::fragment::{FragmentAssembler, FRAGMENT_HEADER_SIZE, make_fragments};
use rfs_core::packet::*;
use rfs_core::time::{TICK_RATE};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tracing::warn;
use uuid::Uuid;

pub const MAX_PACKET_SIZE: usize = 1400;
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
pub const MAX_RELIABLE_WINDOW: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub max_packet_size: usize,
    pub connection_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub reliable_window: u32,
    pub send_buffer_size: usize,
    pub receive_buffer_size: usize,
    pub channels: Vec<ChannelConfig>,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_packet_size: MAX_PACKET_SIZE,
            connection_timeout: CONNECTION_TIMEOUT,
            heartbeat_interval: HEARTBEAT_INTERVAL,
            reliable_window: MAX_RELIABLE_WINDOW,
            send_buffer_size: 1024 * 1024,
            receive_buffer_size: 1024 * 1024,
            channels: vec![
                ChannelConfig::new(ChannelType::UnreliableSequenced, 100, 100),
                ChannelConfig::new(ChannelType::ReliableOrdered, 1000, 1000),
                ChannelConfig::new(ChannelType::ReliableUnordered, 100, 100),
            ],
        }
    }
}

pub struct Connection {
    pub id: u32,
    pub addr: SocketAddr,
    pub state: Mutex<ConnectionState>,
    pub config: ConnectionConfig,
    pub channels: Vec<Channel>,
    pub local_sequence: Mutex<u32>,
    pub remote_sequence: Mutex<u32>,
    pub remote_ack: Mutex<u32>,
    pub remote_ack_bitfield: Mutex<u32>,
    pub pending_acks: Mutex<VecDeque<u32>>,
    pub last_received: Mutex<Instant>,
    pub last_sent: Mutex<Instant>,
    pub last_heartbeat: Mutex<Instant>,
    pub rtt: Mutex<Duration>,
    pub bandwidth: BandwidthTracker,
    pub client_id: Mutex<Option<u32>>,
    pub player_entity: Mutex<Option<EntityId>>,
    pub ship_entity: Mutex<Option<EntityId>>,
    pub interest_entities: Mutex<Vec<EntityId>>,
    pub next_fragment_id: Mutex<u32>,
    pub fragment_assembler: Mutex<FragmentAssembler>,
}

impl Connection {
    pub fn new(id: u32, addr: SocketAddr, config: ConnectionConfig) -> Self {
        let channels = config.channels.iter().map(|c| Channel::new(c.clone())).collect();

        Self {
            id,
            addr,
            state: Mutex::new(ConnectionState::Connecting),
            config,
            channels,
            local_sequence: Mutex::new(0),
            remote_sequence: Mutex::new(0),
            remote_ack: Mutex::new(0),
            remote_ack_bitfield: Mutex::new(0),
            pending_acks: Mutex::new(VecDeque::new()),
            last_received: Mutex::new(Instant::now()),
            last_sent: Mutex::new(Instant::now()),
            last_heartbeat: Mutex::new(Instant::now()),
            rtt: Mutex::new(Duration::from_millis(100)),
            bandwidth: BandwidthTracker::new(),
            client_id: Mutex::new(None),
            player_entity: Mutex::new(None),
            ship_entity: Mutex::new(None),
            interest_entities: Mutex::new(Vec::new()),
            next_fragment_id: Mutex::new(0),
            fragment_assembler: Mutex::new(FragmentAssembler::new()),
        }
    }

    pub fn state(&self) -> ConnectionState {
        *self.state.lock()
    }

    pub fn set_state(&self, state: ConnectionState) {
        *self.state.lock() = state;
    }

    pub fn is_connected(&self) -> bool {
        *self.state.lock() == ConnectionState::Connected
    }

    pub fn client_id(&self) -> Option<u32> {
        *self.client_id.lock()
    }

    pub fn set_client_id(&self, client_id: Option<u32>) {
        *self.client_id.lock() = client_id;
    }

    pub fn player_entity(&self) -> Option<EntityId> {
        *self.player_entity.lock()
    }

    pub fn set_player_entity(&self, player_entity: Option<EntityId>) {
        *self.player_entity.lock() = player_entity;
    }

    pub fn ship_entity(&self) -> Option<EntityId> {
        *self.ship_entity.lock()
    }

    pub fn set_ship_entity(&self, ship_entity: Option<EntityId>) {
        *self.ship_entity.lock() = ship_entity;
    }

    pub fn interest_entities(&self) -> Vec<EntityId> {
        self.interest_entities.lock().clone()
    }

    pub fn set_interest_entities(&self, entities: Vec<EntityId>) {
        *self.interest_entities.lock() = entities;
    }

    pub fn next_sequence(&self) -> u32 {
        let mut seq = self.local_sequence.lock();
        *seq = seq.wrapping_add(1);
        *seq
    }

    pub fn handle_packet(&self, header: PacketHeader, payload: &[u8]) -> Result<Vec<OutgoingPacket>, ConnectionError> {
        *self.last_received.lock() = Instant::now();
        self.bandwidth.record_received(payload.len());

        {
            let mut remote_sequence = self.remote_sequence.lock();
            if header.sequence > *remote_sequence {
                *remote_sequence = header.sequence;
            }
        }

        *self.remote_ack.lock() = header.ack;
        *self.remote_ack_bitfield.lock() = header.ack_bitfield;
        self.process_acks();

        match PacketType::from_u8(header.packet_type) {
            Some(PacketType::Heartbeat) => {
                self.send_heartbeat_ack(header.sequence)
            }
            Some(PacketType::HeartbeatAck) => {
                let rtt = self.last_received.lock().elapsed();
                let mut current = self.rtt.lock();
                *current = Duration::from_millis(
                    ((current.as_millis() as u64 * 3 + rtt.as_millis() as u64) / 4) as u64
                );
                Ok(vec![])
            }
            Some(PacketType::Input) => {
                Ok(vec![])
            }
            Some(PacketType::Fragment) => {
                // Reassembled by the caller via `reassemble_fragment`.
                Ok(vec![])
            }
            Some(PacketType::Command) => {
                Ok(vec![])
            }
            Some(PacketType::EventAck) => {
                Ok(vec![])
            }
            Some(PacketType::Rpc) => {
                Ok(vec![])
            }
            Some(_) => {
                warn!("Unhandled packet type in handle_packet");
                Ok(vec![])
            }
            None => {
                warn!("Invalid packet type: {:?}", header.packet_type);
                Ok(vec![])
            }
        }
    }

    fn process_acks(&self) {
        let mut pending = self.pending_acks.lock();
        let remote_ack = *self.remote_ack.lock();
        let remote_ack_bitfield = *self.remote_ack_bitfield.lock();

        while let Some(&front) = pending.front() {
            if front <= remote_ack ||
               (remote_ack_bitfield & (1 << (front.wrapping_sub(remote_ack) & 31))) != 0 {
                pending.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn send_heartbeat(&self) -> OutgoingPacket {
        let seq = self.next_sequence();
        let packet = HeartbeatPacket {
            client_time: 0.0,
            server_time: 0.0,
        };
        let header = PacketHeader::new(
            PacketType::Heartbeat,
            ChannelType::ReliableOrdered,
            seq,
            *self.remote_sequence.lock(),
            *self.remote_ack_bitfield.lock(),
            0,
        );
        self.pending_acks.lock().push_back(seq);
        OutgoingPacket::new(header, packet)
    }

    pub fn send_heartbeat_ack(&self, ack_seq: u32) -> Result<Vec<OutgoingPacket>, ConnectionError> {
        let seq = self.next_sequence();
        let packet = HeartbeatPacket {
            client_time: 0.0,
            server_time: 0.0,
        };
        let header = PacketHeader::new(
            PacketType::HeartbeatAck,
            ChannelType::ReliableOrdered,
            seq,
            ack_seq,
            *self.remote_ack_bitfield.lock(),
            0,
        );
        Ok(vec![OutgoingPacket::new(header, packet)])
    }

    pub fn send_connect_accept(&self, server_tick: u64, server_time: f64, match_id: Uuid) -> OutgoingPacket {
        let seq = self.next_sequence();
        let packet = ConnectAcceptPacket {
            server_tick,
            server_time,
            assigned_client_id: self.id,
            match_id,
            tick_rate: TICK_RATE,
        };
        let header = PacketHeader::new(
            PacketType::ConnectAccept,
            ChannelType::ReliableOrdered,
            seq,
            *self.remote_sequence.lock(),
            *self.remote_ack_bitfield.lock(),
            0,
        );
        self.pending_acks.lock().push_back(seq);
        OutgoingPacket::new(header, packet)
    }

    pub fn send_state(&self, state: StatePacket) -> Vec<OutgoingPacket> {
        let mut packets = Vec::new();
        let serialized = bincode::serialize(&state).unwrap_or_default();

        if serialized.len() <= self.config.max_packet_size - HEADER_SIZE {
            let seq = self.next_sequence();
            let header = PacketHeader::new(
                PacketType::State,
                ChannelType::UnreliableSequenced,
                seq,
                *self.remote_sequence.lock(),
                *self.remote_ack_bitfield.lock(),
                serialized.len() as u16,
            );
            packets.push(OutgoingPacket::new(header, state));
        } else {
            packets.extend(self.fragment_large_packet::<StatePacket>(PacketType::State, serialized));
        }
        packets
    }

    pub fn send_state_delta(&self, delta: StateDeltaPacket) -> Vec<OutgoingPacket> {
        let mut packets = Vec::new();
        let serialized = bincode::serialize(&delta).unwrap_or_default();

        if serialized.len() <= self.config.max_packet_size - HEADER_SIZE {
            let seq = self.next_sequence();
            let header = PacketHeader::new(
                PacketType::StateDelta,
                ChannelType::UnreliableSequenced,
                seq,
                *self.remote_sequence.lock(),
                *self.remote_ack_bitfield.lock(),
                serialized.len() as u16,
            );
            packets.push(OutgoingPacket::new(header, delta));
        } else {
            packets.extend(self.fragment_large_packet::<StateDeltaPacket>(PacketType::StateDelta, serialized));
        }
        packets
    }

    pub fn send_event(&self, event: EventPacket) -> Vec<OutgoingPacket> {
        let seq = self.next_sequence();
        let header = PacketHeader::new(
            PacketType::Event,
            ChannelType::ReliableUnordered,
            seq,
            *self.remote_sequence.lock(),
            *self.remote_ack_bitfield.lock(),
            0,
        );
        self.pending_acks.lock().push_back(seq);
        vec![OutgoingPacket::new(header, event)]
    }

    pub fn send_command_ack(&self, command_id: u64, success: bool, error: Option<String>) -> OutgoingPacket {
        let seq = self.next_sequence();
        let packet = CommandAckPacket { command_id, success, error };
        let header = PacketHeader::new(
            PacketType::CommandAck,
            ChannelType::ReliableOrdered,
            seq,
            *self.remote_sequence.lock(),
            *self.remote_ack_bitfield.lock(),
            0,
        );
        self.pending_acks.lock().push_back(seq);
        OutgoingPacket::new(header, packet)
    }

    pub fn send_input_ack(&self, ack: InputAckPacket) -> OutgoingPacket {
        let seq = self.next_sequence();
        let header = PacketHeader::new(
            PacketType::InputAck,
            ChannelType::ReliableOrdered,
            seq,
            *self.remote_sequence.lock(),
            *self.remote_ack_bitfield.lock(),
            0,
        );
        OutgoingPacket::new(header, ack)
    }

    fn fragment_large_packet<T: Serializable>(&self, packet_type: PacketType, data: Vec<u8>) -> Vec<OutgoingPacket> {
        let mut packets = Vec::new();
        let max_payload = self.config.max_packet_size - HEADER_SIZE - FRAGMENT_HEADER_SIZE;
        let msg_id = {
            let mut id = self.next_fragment_id.lock();
            *id = id.wrapping_add(1);
            if *id == 0 {
                *id = 1;
            }
            *id
        };

        for payload in make_fragments(packet_type, &data, msg_id, max_payload + FRAGMENT_HEADER_SIZE) {
            let seq = self.next_sequence();
            let header = PacketHeader::new(
                PacketType::Fragment,
                ChannelType::UnreliableSequenced,
                seq,
                *self.remote_sequence.lock(),
                *self.remote_ack_bitfield.lock(),
                payload.len() as u16,
            );
            packets.push(OutgoingPacket::raw(header, payload));
        }
        packets
    }

    /// Feed one `Fragment` payload into this connection's reassembler.
    /// Returns the original packet type plus the full message bytes once complete.
    pub fn reassemble_fragment(&self, payload: &[u8]) -> Option<(PacketType, Vec<u8>)> {
        self.fragment_assembler.lock().push(payload)
    }

    pub fn is_timed_out(&self) -> bool {
        self.last_received.lock().elapsed() > self.config.connection_timeout
    }

    pub fn should_send_heartbeat(&self) -> bool {
        self.last_heartbeat.lock().elapsed() >= self.config.heartbeat_interval
    }

    pub fn update_heartbeat(&self) {
        *self.last_heartbeat.lock() = Instant::now();
    }

    pub fn get_channel(&self, channel_type: ChannelType) -> Option<&Channel> {
        self.channels.iter().find(|c| c.config().channel_type == channel_type)
    }
}

#[derive(Debug, Clone)]
pub struct OutgoingPacket {
    pub header: PacketHeader,
    pub payload: Vec<u8>,
}

impl OutgoingPacket {
    pub fn new(mut header: PacketHeader, packet: impl Serializable) -> Self {
        let payload = bincode::serialize(&packet).unwrap_or_default();
        header.payload_size = payload.len() as u16;
        Self {
            header,
            payload,
        }
    }

    pub fn raw(mut header: PacketHeader, payload: Vec<u8>) -> Self {
        header.payload_size = payload.len() as u16;
        Self {
            header,
            payload,
        }
    }

    pub fn total_size(&self) -> usize {
        HEADER_SIZE + self.payload.len()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("Invalid packet type: {0}")]
    InvalidPacketType(u8),
    #[error("Packet too large: {0} > {1}")]
    PacketTooLarge(usize, usize),
    #[error("Connection timed out")]
    Timeout,
    #[error("Connection closed")]
    Closed,
}