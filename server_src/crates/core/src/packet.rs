pub use crate::spatial::EntityId;
use crate::math::{Vec3f, Transform};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_PACKET_SIZE: usize = 1400;
pub const HEADER_SIZE: usize = 24;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PacketType {
    Connect = 0x01,
    ConnectAccept = 0x02,
    ConnectReject = 0x03,
    Disconnect = 0x04,
    Heartbeat = 0x05,
    HeartbeatAck = 0x06,
    Input = 0x10,
    InputAck = 0x11,
    State = 0x20,
    StateDelta = 0x21,
    StateFull = 0x22,
    Fragment = 0x23,
    Event = 0x30,
    EventAck = 0x31,
    Command = 0x40,
    CommandAck = 0x41,
    Rpc = 0x50,
    RpcResponse = 0x51,
}

impl PacketType {
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0x01 => Self::Connect,
            0x02 => Self::ConnectAccept,
            0x03 => Self::ConnectReject,
            0x04 => Self::Disconnect,
            0x05 => Self::Heartbeat,
            0x06 => Self::HeartbeatAck,
            0x10 => Self::Input,
            0x11 => Self::InputAck,
            0x20 => Self::State,
            0x21 => Self::StateDelta,
            0x22 => Self::StateFull,
            0x23 => Self::Fragment,
            0x30 => Self::Event,
            0x31 => Self::EventAck,
            0x40 => Self::Command,
            0x41 => Self::CommandAck,
            0x50 => Self::Rpc,
            0x51 => Self::RpcResponse,
            _ => return None,
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelType {
    Unreliable = 0,
    UnreliableSequenced = 1,
    ReliableUnordered = 2,
    ReliableOrdered = 3,
}

impl ChannelType {
    pub const POSITION: Self = Self::UnreliableSequenced;
    pub const COMMAND: Self = Self::ReliableOrdered;
    pub const EVENT: Self = Self::ReliableUnordered;
    pub const RPC: Self = Self::ReliableOrdered;
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PacketHeader {
    pub magic: u32,
    pub version: u16,
    pub packet_type: u8,
    pub channel: u8,
    pub sequence: u32,
    pub ack: u32,
    pub ack_bitfield: u32,
    pub payload_size: u16,
    pub flags: u16,
}

impl PacketHeader {
    pub const MAGIC: u32 = 0x52465300;

    pub fn new(packet_type: PacketType, channel: ChannelType, sequence: u32, ack: u32, ack_bitfield: u32, payload_size: u16) -> Self {
        Self {
            magic: Self::MAGIC,
            version: PROTOCOL_VERSION,
            packet_type: packet_type as u8,
            channel: channel as u8,
            sequence,
            ack,
            ack_bitfield,
            payload_size,
            flags: 0,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.magic == Self::MAGIC && self.version == PROTOCOL_VERSION
    }
}

pub trait Serializable: Serialize + for<'de> Deserialize<'de> {
    fn serialize(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    fn deserialize(data: &[u8]) -> Result<Self, bincode::Error>
    where
        Self: Sized,
    {
        bincode::deserialize(data)
    }
}

impl<T: Serialize + for<'de> Deserialize<'de>> Serializable for T {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectPacket {
    pub client_id: Uuid,
    pub protocol_version: u16,
    pub player_name: String,
    pub build_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectAcceptPacket {
    pub server_tick: u64,
    pub server_time: f64,
    pub assigned_client_id: u32,
    pub match_id: Uuid,
    pub tick_rate: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectRejectPacket {
    pub reason: ConnectRejectReason,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectRejectReason {
    VersionMismatch = 0,
    ServerFull = 1,
    Banned = 2,
    InvalidData = 3,
    MatchNotFound = 4,
    MatchFull = 5,
    InternalError = 6,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisconnectPacket {
    pub reason: DisconnectReason,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisconnectReason {
    ClientQuit = 0,
    Kicked = 1,
    Timeout = 2,
    ServerShutdown = 3,
    Error = 4,
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct HeartbeatPacket {
    pub client_time: f64,
    pub server_time: f64,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct InputPacket {
    pub tick: u32,
    pub delta_time: f32,
    pub move_forward: f32,
    pub move_right: f32,
    pub move_up: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub actions: InputActions,
    pub station_interaction: Option<StationInteraction>,
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct InputActions: u32 {
        const NONE = 0;
        const JUMP = 1 << 0;
        const CROUCH = 1 << 1;
        const SPRINT = 1 << 2;
        const USE = 1 << 3;
        const RELOAD = 1 << 4;
        const FIRE_PRIMARY = 1 << 5;
        const FIRE_SECONDARY = 1 << 6;
        const STATION_ENTER = 1 << 7;
        const STATION_EXIT = 1 << 8;
        const INTERACT = 1 << 9;
        const TOGGLE_MAP = 1 << 10;
        const TOGGLE_SCOREBOARD = 1 << 11;
        const CHAT = 1 << 12;
        const VOICE = 1 << 13;
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct StationInteraction {
    pub station_id: EntityId,
    pub action: StationAction,
    pub value: f32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StationAction {
    SetYaw = 0,
    SetPitch = 1,
    SetThrottle = 2,
    SetRudder = 3,
    Fire = 4,
    Reload = 5,
    SetAmmoType = 6,
    PumpWater = 7,
    SealBulkhead = 8,
    OpenBulkhead = 9,
    Repair = 10,
    ExtinguishFire = 11,
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct InputAckPacket {
    pub tick: u32,
    pub accepted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatePacket {
    pub server_tick: u32,
    pub server_time: f64,
    pub entities: Vec<EntityState>,
    pub projectiles: Vec<ProjectileState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityState {
    pub entity_id: EntityId,
    pub entity_type: EntityType,
    pub transform: Transform,
    pub velocity: Vec3f,
    pub angular_velocity: Vec3f,
    pub health: f32,
    pub max_health: f32,
    pub flags: EntityFlags,
    pub ship_data: Option<ShipStateData>,
    pub station_data: Option<StationStateData>,
    pub player_data: Option<PlayerStateData>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityType {
    Ship = 0,
    Station = 1,
    Player = 2,
    Projectile = 3,
    Compartment = 4,
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct EntityFlags: u32 {
        const NONE = 0;
        const ON_FIRE = 1 << 0;
        const FLOODING = 1 << 1;
        const DISABLED = 1 << 2;
        const SINKING = 1 << 3;
        const CAPTURED = 1 << 4;
        const PLAYER_CONTROLLED = 1 << 5;
        const AI_CONTROLLED = 1 << 6;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShipStateData {
    pub ship_class_id: u32,
    pub compartments: Vec<CompartmentState>,
    pub stations: Vec<EntityId>,
    pub fuel: f32,
    pub max_fuel: f32,
    pub speed: f32,
    pub max_speed: f32,
    pub heading: f32,
    pub rudder_angle: f32,
    pub throttle: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompartmentState {
    pub compartment_id: EntityId,
    pub water_level: f32,
    pub max_water_level: f32,
    pub is_sealed: bool,
    pub is_breached: bool,
    pub fire_intensity: f32,
    pub connected_compartments: SmallVec<[EntityId; 4]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StationStateData {
    pub station_type: StationType,
    pub occupant: Option<EntityId>,
    pub yaw: f32,
    pub pitch: f32,
    pub reload_progress: f32,
    pub ammo_type: u8,
    pub is_operational: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StationType {
    Helm = 0,
    Gun = 1,
    Engine = 2,
    Pump = 3,
    DamageControl = 4,
    Radar = 5,
    Comms = 6,
    Captain = 7,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerStateData {
    pub player_id: u32,
    pub name: String,
    pub team: u8,
    pub current_station: Option<EntityId>,
    pub posture: PlayerPosture,
    pub health: f32,
    pub stamina: f32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerPosture {
    Standing = 0,
    Crouching = 1,
    Prone = 2,
    Seated = 3,
    Unconscious = 4,
    Dead = 5,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectileState {
    pub entity_id: EntityId,
    pub projectile_type: ProjectileType,
    pub position: Vec3f,
    pub velocity: Vec3f,
    pub spawn_tick: u32,
    pub lifetime: f32,
    pub owner: EntityId,
    pub damage: f32,
    pub penetration: f32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjectileType {
    Cannonball = 0,
    ExplosiveShell = 1,
    ArmorPiercing = 2,
    ChainShot = 3,
    GrapeShot = 4,
    Torpedo = 5,
    DepthCharge = 6,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateDeltaPacket {
    /// Replication layer (§3.2/§3.4 of the plan): 0 ships, 1 compartments/stations,
    /// 2 players, 3 projectiles. Each layer is an independent delta stream.
    pub layer: u8,
    pub base_tick: u32,
    pub server_tick: u32,
    pub server_time: f64,
    pub created: Vec<EntityState>,
    pub updated: Vec<EntityStateUpdate>,
    pub destroyed: Vec<EntityId>,
    pub projectile_created: Vec<ProjectileState>,
    pub projectile_updated: Vec<ProjectileStateUpdate>,
    pub projectile_destroyed: Vec<EntityId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectileStateUpdate {
    pub entity_id: EntityId,
    pub position: Vec3f,
    pub velocity: Vec3f,
    pub lifetime: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityStateUpdate {
    pub entity_id: EntityId,
    pub transform: Option<Transform>,
    pub velocity: Option<Vec3f>,
    pub angular_velocity: Option<Vec3f>,
    pub health: Option<f32>,
    pub flags: Option<EntityFlags>,
    pub ship_data: Option<ShipStateData>,
    pub station_data: Option<StationStateData>,
    pub player_data: Option<PlayerStateData>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventPacket {
    pub events: Vec<GameEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GameEvent {
    ShipHit {
        target: EntityId,
        projectile: EntityId,
        position: Vec3f,
        normal: Vec3f,
        damage: f32,
        penetration: f32,
        hit_compartment: Option<EntityId>,
    },
    ShipSunk {
        ship: EntityId,
        position: Vec3f,
    },
    CompartmentFlooded {
        compartment: EntityId,
        water_level: f32,
    },
    CompartmentSealed {
        compartment: EntityId,
    },
    CompartmentBreached {
        compartment: EntityId,
    },
    StationOccupied {
        station: EntityId,
        player: EntityId,
    },
    StationVacated {
        station: EntityId,
        player: EntityId,
    },
    PlayerSpawned {
        player: EntityId,
        ship: EntityId,
        position: Vec3f,
    },
    PlayerDied {
        player: EntityId,
        killer: Option<EntityId>,
    },
    ProjectileFired {
        projectile: EntityId,
        position: Vec3f,
        velocity: Vec3f,
        weapon: EntityId,
    },
    ProjectileImpact {
        projectile: EntityId,
        position: Vec3f,
        normal: Vec3f,
    },
    BulkheadStateChanged {
        bulkhead: EntityId,
        is_sealed: bool,
    },
    PumpStateChanged {
        pump: EntityId,
        is_active: bool,
    },
    FireStarted {
        compartment: EntityId,
        intensity: f32,
    },
    FireExtinguished {
        compartment: EntityId,
    },
    TeamScoreChanged {
        team: u8,
        score: u32,
    },
    MatchStateChanged {
        state: MatchState,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchState {
    Waiting = 0,
    Starting = 1,
    InProgress = 2,
    Ended = 3,
    Aborted = 4,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventAckPacket {
    pub event_ids: Vec<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandPacket {
    pub command_id: u64,
    pub command: ServerCommand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerCommand {
    SetShipThrottle { ship: EntityId, throttle: f32 },
    SetShipRudder { ship: EntityId, rudder: f32 },
    FireWeapon { station: EntityId, target_pos: Option<Vec3f> },
    ReloadWeapon { station: EntityId, ammo_type: u8 },
    SealBulkhead { bulkhead: EntityId },
    OpenBulkhead { bulkhead: EntityId },
    ActivatePump { pump: EntityId },
    DeactivatePump { pump: EntityId },
    RepairStation { station: EntityId },
    ExtinguishFire { compartment: EntityId },
    EnterStation { player: EntityId, station: EntityId },
    ExitStation { player: EntityId },
    Respawn { player: EntityId },
    ChatMessage { player: EntityId, message: String, channel: ChatChannel },
    VoteKick { initiator: EntityId, target: EntityId },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatChannel {
    All = 0,
    Team = 1,
    Ship = 2,
    Whisper = 3,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandAckPacket {
    pub command_id: u64,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcPacket {
    pub rpc_id: u64,
    pub method: String,
    pub params: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcResponsePacket {
    pub rpc_id: u64,
    pub success: bool,
    pub result: Option<Vec<u8>>,
    pub error: Option<String>,
}

pub fn serialize_packet<T: Serializable>(packet: &T, header: PacketHeader) -> Result<Vec<u8>, bincode::Error> {
    let payload = bincode::serialize(packet)?;
    let mut header = header;
    header.payload_size = payload.len() as u16;
    let mut buffer = Vec::with_capacity(HEADER_SIZE + payload.len());
    buffer.extend_from_slice(&bincode::serialize(&header)?);
    buffer.extend_from_slice(&payload);
    Ok(buffer)
}

pub fn deserialize_packet(data: &[u8]) -> Result<(PacketHeader, &[u8]), bincode::Error> {
    if data.len() < HEADER_SIZE {
        return Err(bincode::Error::new(bincode::ErrorKind::Custom("Packet too small".into())));
    }

    let header: PacketHeader = bincode::deserialize(&data[..HEADER_SIZE])?;

    if !header.is_valid() {
        return Err(bincode::Error::new(bincode::ErrorKind::Custom("Invalid packet header".into())));
    }

    let payload = &data[HEADER_SIZE..];
    if payload.len() != header.payload_size as usize {
        return Err(bincode::Error::new(bincode::ErrorKind::Custom("Payload size mismatch".into())));
    }

    Ok((header, payload))
}