use crate::entity::HitMark;
use crate::math::{Transform, Vec3f};
pub use crate::spatial::EntityId;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use uuid::Uuid;

// Bug №700: the authentication block (bug №700, owner plan point 1) changes
// the framing of every datagram — an HMAC block now precedes the header — so
// the version moves and an old client is refused at the version door it
// already reaches (bug №176) instead of being misparsed as datagrams.
// Bug №216: the version moves again — the session fields (`session` on
// Accept/Heartbeat/Input/Command) and the Challenge/Confirm variants change
// the meaning of every replicated packet, so a cross-version pair must not
// try to talk: the old client would send session-less packets that the new
// server honestly drops, and the game would look like random lag.
// Bug №732: the version moves a third time — payloads now encode with
// postcard (varint framing) instead of bincode's fixed-width fixint, so a
// cross-version pair would be two encoders saying the same words in different
// shapes. The header itself stays a hand-rolled 24 bytes (see HEADER_SIZE);
// the bump is for the payload half.
pub const PROTOCOL_VERSION: u16 = 4;
/// Bug №239: the single statement of the per-datagram wire budget. It does
/// not deliberately enforce itself inside core — core only serializes — but
/// the net crate re-exports exactly this value as `connection::MAX_PACKET_SIZE`
/// and takes its default config MTU from it, so a change made here is the
/// change the wire really enforces. Restating the number anywhere else is a
/// defect, not a convenience.
pub const MAX_PACKET_SIZE: usize = 1400;
pub const HEADER_SIZE: usize = 24;
/// Bug №700: what every datagram grows by before the header — a 16-byte
/// truncated HMAC plus an 8-byte replay sequence. Both sides pay it in both
/// modes (the unsigned dialect writes a zero block) so the framing itself
/// has exactly one statement, here, next to the budget it eats into.
pub const AUTH_WIRE_LEN: usize = 24;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
// Bug №60: this enum's wire code is POSITIONAL in the declaration order —
// bincode 1.3 (which carried the wire across №9..№700) ignored the `= N`
// discriminants, and postcard since bug №732 does the same (an enum encodes
// as the varint ordinal of the declaration order). Keep declaration order
// stable (pinned by Tool/harness/tests/protocol.rs); a reorder requires a
// PROTOCOL_VERSION bump.
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
    /// Bug №267 (A): the client asks the server to resync one replication
    /// layer immediately. Appended after RpcResponse so the positional
    /// coding of every earlier variant stays exactly where it was (bug №60)
    /// — no PROTOCOL_VERSION bump is needed for an append.
    ResyncRequest = 0x52,
    /// Bug №698: the world's static description, sent once when a client
    /// connects. The world is not a layer and does not change, so there is
    /// nothing to stream — a client that never hears of the structures cannot
    /// draw them, and a shell that vanishes at an invisible rock looks like a
    /// bug in the shooter.
    MapDescription = 0x53,
    /// Bug №216: the server-to-client challenge of the connect handshake.
    /// An address claim proves nothing over UDP; the challenge proves the
    /// sender actually listens on the socket it wrote, because the nonce —
    /// an off-path attacker cannot guess — travels back in Confirm. Appended
    /// after MapDescription so the positional coding of every earlier
    /// variant stays exactly where it was (bug №60) — no enum reorder.
    ConnectChallenge = 0x54,
    /// Bug №216: the client's answer. Carries back the nonce from the
    /// challenge; only a matching nonce promotes Connect to a real
    /// connection.
    ConnectConfirm = 0x55,
    /// Bug №30 (B): the client's per-layer applied report. Appended after
    /// ConnectConfirm, so the positional coding of every earlier variant
    /// stays exactly where it was — no PROTOCOL_VERSION bump is needed for
    /// an append (the same contract ResyncRequest documents above). An
    /// older binary receiving it drops it at the type gate (degrade to the
    /// pre-№30 behavior); an older SENDER simply never emits it.
    LayerSync = 0x56,
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
            0x52 => Self::ResyncRequest,
            0x53 => Self::MapDescription,
            0x54 => Self::ConnectChallenge,
            0x55 => Self::ConnectConfirm,
            0x56 => Self::LayerSync,
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

    pub fn new(
        packet_type: PacketType,
        channel: ChannelType,
        sequence: u32,
        ack: u32,
        ack_bitfield: u32,
        payload_size: u16,
    ) -> Self {
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

    /// Bug №732: the header's wire form, written by hand and a constant
    /// [`HEADER_SIZE`] bytes.
    ///
    /// It used to be `bincode::serialize(&self)` — fixint, big-endian, the
    /// exact bytes this writer emits. Moving payloads to postcard cannot
    /// move the header: postcard frames integers as varints, the header
    /// would stop being a fixed-width slice, and every MTU, fragmentation
    /// and size-cap argument built on `HEADER_SIZE` across the net crate
    /// would have to be rewritten for nothing. Nine plain integers carry no
    /// schema that a self-describing format could compact.
    ///
    /// The layout matches what bincode 1.3 emitted (fields in declaration
    /// order, big-endian), so the constant, the reader below and any wire
    /// dump agree field for field:
    /// `magic u32, version u16, packet_type u8, channel u8, sequence u32,
    /// ack u32, ack_bitfield u32, payload_size u16, flags u16`.
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.resize(start + HEADER_SIZE, 0);
        out[start..start + 4].copy_from_slice(&self.magic.to_be_bytes());
        out[start + 4..start + 6].copy_from_slice(&self.version.to_be_bytes());
        out[start + 6] = self.packet_type;
        out[start + 7] = self.channel;
        out[start + 8..start + 12].copy_from_slice(&self.sequence.to_be_bytes());
        out[start + 12..start + 16].copy_from_slice(&self.ack.to_be_bytes());
        out[start + 16..start + 20].copy_from_slice(&self.ack_bitfield.to_be_bytes());
        out[start + 20..start + 22].copy_from_slice(&self.payload_size.to_be_bytes());
        out[start + 22..start + 24].copy_from_slice(&self.flags.to_be_bytes());
    }

    /// The fixed wire bytes on their own, for the call sites that build a
    /// raw datagram header-first and append the payload after it.
    pub fn to_wire(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_SIZE);
        self.encode_into(&mut out);
        out
    }

    /// Parses exactly [`HEADER_SIZE`] prefix bytes as the fixed header. The
    /// bytes have to agree with [`PacketHeader::encode_into`] field for
    /// field; a datagram shorter than the header is refused the same way
    /// `deserialize_packet` refused it before the hand codec existed.
    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        if data.len() < HEADER_SIZE {
            return Err(bincode::Error::new(bincode::ErrorKind::Custom(
                "Packet too small".into(),
            )));
        }
        Ok(Self {
            magic: u32::from_be_bytes(data[0..4].try_into().unwrap()),
            version: u16::from_be_bytes(data[4..6].try_into().unwrap()),
            packet_type: data[6],
            channel: data[7],
            sequence: u32::from_be_bytes(data[8..12].try_into().unwrap()),
            ack: u32::from_be_bytes(data[12..16].try_into().unwrap()),
            ack_bitfield: u32::from_be_bytes(data[16..20].try_into().unwrap()),
            payload_size: u16::from_be_bytes(data[20..22].try_into().unwrap()),
            flags: u16::from_be_bytes(data[22..24].try_into().unwrap()),
        })
    }
}

pub trait Serializable: Serialize + for<'de> Deserialize<'de> {
    fn serialize(&self) -> Result<Vec<u8>, bincode::Error> {
        // Bug №732: payload wire is postcard — the error still boxes into the
        // `bincode::Error` carrier every signature on the path declares.
        // (NCPC-732-TEMP proved the wire reacts: bincode reintroduced here
        // and in `encode` left the loopback pair timing out at Connect.)
        postcard::to_allocvec(self).map_err(encoding_error)
    }

    fn deserialize(data: &[u8]) -> Result<Self, bincode::Error>
    where
        Self: Sized,
    {
        // Trailing bytes are refused here: postcard reads exactly the frame
        // its varints asked for, and what is left over is a corrupt frame,
        // not a second message.
        postcard::from_bytes(data).map_err(encoding_error)
    }
}

impl<T: Serialize + for<'de> Deserialize<'de>> Serializable for T {}

/// Bug №732: postcard's error, boxed into the error carrier the wire stack
/// already speaks. `Serialization(#[from] bincode::Error)` and the dozen
/// `Result<_, bincode::Error>` signatures across connection/client/server
/// stay as they are; nothing downstream matches on the finer bincode kinds —
/// the failure sites log or wrap, never branch — so `Custom` is lossless.
pub fn encoding_error(error: postcard::Error) -> bincode::Error {
    bincode::Error::new(bincode::ErrorKind::Custom(error.to_string()))
}

/// Bug №732: the one payload encoder for the wire. Every production call
/// site funnels here (or through the identical `Serializable` trait default),
/// so a future format change is this one spot plus a protocol bump.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, bincode::Error> {
    // Bug №732: the one payload encoder for the wire. Every production call
    // site funnels here (or through the identical `Serializable` trait
    // default), so a future format change is this one spot plus a protocol
    // bump. (NCPC-732-TEMP proved the tests react: with bincode here the
    // loopback pair's handshake goes red.)
    postcard::to_allocvec(value).map_err(encoding_error)
}

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
    /// Bug №216: the per-connection session number the server minted for
    /// this connection. The client echoes it in every inbound packet
    /// (Heartbeat, Input, Command); the server drops packets whose echo does
    /// not match the session it issued over the accept — which only the real
    /// listener of the claimed address ever saw.
    pub session: u128,
}

/// Bug №216: the server-to-client challenge. `nonce` is unpredictable and
/// single-use; the client proves it lives on the claimed socket by echoing
/// it back in ConnectConfirm. An off-path forger that claimed a victim's
/// source address never receives this packet and never answers it.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct ConnectChallengePacket {
    pub nonce: u128,
}

/// Bug №216: the client's answer to ConnectChallenge, the proof of life on
/// the claimed socket.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct ConnectConfirmPacket {
    pub nonce: u128,
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
    /// Bug №216: the session echo. A heartbeat without the session the
    /// accept carried cannot refresh a connection's liveness — a spoofed
    /// beat would otherwise keep any registered address alive forever.
    pub session: u128,
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
    /// Bug №216: the session echo. Input is accepted only with the session
    /// the server delivered in ConnectAccept — an off-path forger that fakes
    /// the victim's source address cannot echo a value it never saw.
    pub session: u128,
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
    /// Bug №272: the last hit, replicated so the impact survives a lost event.
    pub last_hit: Option<HitMark>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompartmentState {
    pub compartment_id: EntityId,
    pub water_level: f32,
    pub max_water_level: f32,
    pub is_sealed: bool,
    pub is_breached: bool,
    pub fire_intensity: f32,
    /// Bug №61: pump activity used to be dropped on the wire, so the client
    /// could never show whether a pump is running.
    pub pump_active: bool,
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
    /// Bug №61: the full station suite — the client had no access to
    /// ammunition, health or cooldown before.
    pub ammo_count: u32,
    pub max_ammo: u32,
    pub health: f32,
    pub max_health: f32,
    pub cooldown: f32,
    pub max_cooldown: f32,
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
    /// Bug №274: which ship this player rides. The player's position is derived
    /// from the ship's transform, so without this a client cannot tell which
    /// ship it is on — it has to reverse-engineer the ship's station list.
    pub current_ship: Option<EntityId>,
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
    /// Bug №23/№78: true when the server diffed against an empty base, so this
    /// delta is a complete replacement for the layer. The client applies it
    /// even when its base tick does not match (healing lost deltas) and prunes
    /// everything it previously held for this layer as a full replace.
    pub is_resync: bool,
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
    /// Bug №24: Options preserve "unchanged" — the old non-Option fields
    /// zeroed position/velocity/lifetime on every partial update.
    pub position: Option<Vec3f>,
    pub velocity: Option<Vec3f>,
    pub lifetime: Option<f32>,
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
    /// Bug №634: a chat message accepted from an authenticated player. The
    /// command carries a spoofable `player` field; by the time this event is
    /// broadcast, the server has already replaced it with the connection's
    /// authenticated player. Interest filtering applies at the server:
    /// `All` reaches everyone (no subject), every narrower channel is about
    /// the sender.
    ChatReceived {
        player: EntityId,
        message: String,
        channel: ChatChannel,
    },
    /// A ramming hit: two hulls met and the contact did damage.
    ///
    /// Appended last so the positional coding of every earlier variant (bug
    /// №60) stays where it was. The design note asks for the ram to be a
    /// DIFFERENT source of damage, not a new damage system — the same
    /// compartment graph does the work, but the client has to be able to tell
    /// a ram from a shell, so this is its own event.
    ShipRam {
        /// The ship that hit.
        rammer: EntityId,
        /// The ship that was hit.
        target: EntityId,
        position: Vec3f,
        normal: Vec3f,
        damage: f32,
    },
    /// Bug №701: the hull's loss became certain and the ship began to go
    /// under. Fires ONCE on the crossing edge (the №153 rule), not on every
    /// tick of the drift; `ShipSunk` stays the last event of the sequence and
    /// still names the wreck's position. The crew's per-player deaths ride
    /// `PlayerDied`, which the server fans out separately — the sim cannot
    /// name players, only its ships.
    ///
    /// Appended last so the positional coding of every earlier variant (bug
    /// №60) stays where it was.
    ShipSinkingStarted {
        ship: EntityId,
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
    /// Bug №216: the session echo, same contract as Input's — the command
    /// executes only with the session the accept carried.
    pub session: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerCommand {
    SetShipThrottle {
        ship: EntityId,
        throttle: f32,
    },
    SetShipRudder {
        ship: EntityId,
        rudder: f32,
    },
    FireWeapon {
        station: EntityId,
        target_pos: Option<Vec3f>,
    },
    ReloadWeapon {
        station: EntityId,
        ammo_type: u8,
    },
    SealBulkhead {
        bulkhead: EntityId,
    },
    OpenBulkhead {
        bulkhead: EntityId,
    },
    /// Seals or opens a whole compartment's hull-side seal (bug №668): the
    /// flag a breached compartment checks before taking water at all. The
    /// seat rule mirrors the bulkhead commands: damage control only.
    SetCompartmentSeal {
        compartment: EntityId,
        sealed: bool,
    },
    ActivatePump {
        pump: EntityId,
    },
    DeactivatePump {
        pump: EntityId,
    },
    RepairStation {
        station: EntityId,
    },
    ExtinguishFire {
        compartment: EntityId,
    },
    /// Turns a station's default behavior on and off (bug №669). The code is
    /// a small integer: 0 = Active, 1 = Passive, 2 = Disabled (the mapping
    /// lives on the ship side, `DefaultBehavior::from_code`). An unknown code
    /// is refused by the server, never defaulted.
    SetStationBehavior {
        station: EntityId,
        behavior: u8,
    },
    EnterStation {
        player: EntityId,
        station: EntityId,
    },
    ExitStation {
        player: EntityId,
    },
    Respawn {
        player: EntityId,
    },
    ChatMessage {
        player: EntityId,
        message: String,
        channel: ChatChannel,
    },
    VoteKick {
        initiator: EntityId,
        target: EntityId,
    },
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

/// Bug №267 (A): the layer this client wants resynced now. Deliberately tiny
/// — it must never fragment, and 24 bytes of payload leaves room for it under
/// any MTU.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResyncRequestPacket {
    pub layer: u8,
}

/// Bug №30 (B): the client's answer to the server's blind base advance. The
/// server used to move each layer's diff base to the last queued send; a
/// queued send is not a delivered one, so a loss left the server declaring a
/// base tick the client never applied — every following delta of that layer
/// was dropped until a resync healed it. This report carries the ticks the
/// client has actually APPLIED per layer (the same numbers its own base-mismatch
/// guard checks deltas against), so the server can diff against truth.
///
/// Deliberately tiny and absolute: four plain ticks beat any incremental
/// scheme the way the resync request does — losing a report costs one report
/// interval of freshness, never a corrupting gap; the server merges reports
/// with `max` (bug №30: duplicate or reordered datagrams cannot rewind it).
///
/// The lens `[u32; 4]` is the replication layer count. Core cannot see
/// `crate::snapshot::LAYER_COUNT`, so the net crate pins its own
/// `LAYER_COUNT == 4` at compile time next to every encode/decode site; if
/// the replication layer count ever changes, that pin fails the build
/// instead of the wire drifting from it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerSyncPacket {
    /// One applied tick per replication layer, in `LAYER_COUNT` order. A
    /// value of 0 means that layer has never applied anything (the client's
    /// default before its first full state).
    pub applied: [u32; 4],
    /// Bug №216: the session echo, same contract as Input's — a report is
    /// accepted only with the session the accept carried, or an off-path
    /// forger could drive the server's confirmed bases wherever it likes.
    pub session: u128,
}

/// Bug №698: the world's static description, sent once per client.
///
/// The payload is a volume plus a list of solid volumes. It can be larger
/// than a datagram (a map with thousands of structures), so it goes through
/// the same fragmentation path as a full state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapDescriptionPacket {
    pub name: String,
    pub bounds_min: Vec3f,
    pub bounds_max: Vec3f,
    pub obstacles: Vec<crate::world::MapObstacleDescription>,
}

pub fn serialize_packet<T: Serializable>(
    packet: &T,
    header: PacketHeader,
) -> Result<Vec<u8>, bincode::Error> {
    // Bug №732: payload postcard, header hand-rolled (see `encode_into`) —
    // the framing stays header-then-payload, and the payload length the
    // header declared stays the truth signed off above.
    let payload = postcard::to_allocvec(packet).map_err(encoding_error)?;
    let mut header = header;
    // Bug №9: `as u16` truncation used to let a lying header reach the wire.
    header.payload_size = u16::try_from(payload.len())
        .map_err(|_| bincode::Error::new(bincode::ErrorKind::SizeLimit))?;
    let mut buffer = Vec::with_capacity(HEADER_SIZE + payload.len());
    header.encode_into(&mut buffer);
    buffer.extend_from_slice(&payload);
    Ok(buffer)
}

pub fn deserialize_packet(data: &[u8]) -> Result<(PacketHeader, &[u8]), bincode::Error> {
    let header = PacketHeader::decode(data)?;

    if !header.is_valid() {
        return Err(bincode::Error::new(bincode::ErrorKind::Custom(
            "Invalid packet header".into(),
        )));
    }

    let payload = &data[HEADER_SIZE..];
    if payload.len() != header.payload_size as usize {
        return Err(bincode::Error::new(bincode::ErrorKind::Custom(
            "Payload size mismatch".into(),
        )));
    }

    Ok((header, payload))
}
