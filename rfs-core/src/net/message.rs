pub use crate::types::common::Stance;
use std::sync::Arc;

use crate::types::common::VoiceChannel;
use crate::types::common::{ChatChannel, FireRole};
use crate::types::effects::{ParticleEvent, SoundEvent};
use crate::types::health::DamageType;
use crate::types::input::InputFrame;
pub use crate::types::player::{PlayerId, PlayerState};
use crate::types::vehicle::VehicleState;
use crate::types::world::{CivilianState, FireHydrant, FireSource};
use glam::{Quat, Vec3};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerListEntry {
    pub id: PlayerId,
    pub name: String,
    pub ping: u32,
    pub role: FireRole,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NetMessage {
    // Connection (0-5)
    Connect {
        player_name: String,
        protocol_version: u32,
        auth_token: Option<String>,
        client_uuid: Option<String>,
        /// Index into the map's spawn_points list chosen on the deployment screen.
        /// `None` or out-of-range falls back to server-side round-robin.
        spawn_index: Option<u8>,
    },
    Accept {
        player_id: PlayerId,
        server_name: String,
        player_count: u32,
        max_players: u32,
        map_name: String,
        game_mode: String,
        session_uuid: String,
    },
    QueueUpdate {
        position: u32,
        total: u32,
    },
    Reject {
        reason: String,
    },
    Disconnect {
        reason: String,
    },
    Ping {
        client_time: f64,
    },
    Pong {
        client_time: f64,
        server_time: f64,
    },

    // Player (10-19)
    PlayerSpawned {
        player_id: PlayerId,
        position: Vec3,
        rotation: Quat,
    },
    /// Client tells the server which spawn point it chose on the deployment screen.
    /// The server repositions the player accordingly.
    SpawnChoice {
        player_id: PlayerId,
        spawn_index: u8,
    },
    PlayerDespawned {
        player_id: PlayerId,
    },
    PlayerInput {
        frame: InputFrame,
    },
    PlayerStateUpdate {
        player_id: PlayerId,
        state: PlayerState,
    },
    PlayerIncapacitated {
        player_id: PlayerId,
        cause: DamageType,
    },
    PlayerRescued {
        player_id: PlayerId,
        rescuer: PlayerId,
    },
    PlayerStanceChanged {
        player_id: PlayerId,
        stance: Stance,
    },
    PlayerToolChanged {
        player_id: PlayerId,
        tool_index: u8,
    },
    PlayerRoleChanged {
        player_id: PlayerId,
        role: FireRole,
    },

    // Equipment (20-25)
    ToolUsed {
        player_id: PlayerId,
        tool_index: u8,
        target_position: Vec3,
    },
    HoseActivated {
        player_id: PlayerId,
        nozzle_position: Vec3,
        direction: Vec3,
        pressure: f32,
    },
    HoseDeactivated {
        player_id: PlayerId,
    },
    HydrantConnected {
        player_id: PlayerId,
        hydrant_id: u32,
    },
    HydrantDisconnected {
        player_id: PlayerId,
    },

    // Fire (30-35)
    FireIgnited {
        fire_id: u32,
        position: Vec3,
        intensity: f32,
        temperature: f32,
    },
    FireExtinguished {
        fire_id: u32,
    },
    FireUpdated {
        fire_id: u32,
        intensity: f32,
        temperature: f32,
    },
    SmokeSpread {
        position: Vec3,
        density: f32,
        radius: f32,
    },
    StructureDamaged {
        position: Vec3,
        damage_level: f32,
    },

    // Civilian (40-48)
    CivilianFound {
        civilian: CivilianState,
    },
    CivilianRescued {
        civilian_id: u32,
        rescuer_id: PlayerId,
    },
    CivilianStatus {
        civilian_id: u32,
        status: crate::types::world::CivilianStatus,
    },
    CivilianPickup {
        civilian_id: u32,
        carrier_id: PlayerId,
    },
    CivilianDrop {
        civilian_id: u32,
        carrier_id: PlayerId,
    },

    // Chat (50)
    ChatMessage {
        sender_id: PlayerId,
        sender_name: String,
        message: String,
        channel: ChatChannel,
    },

    // Voice (51-53)
    VoicePacket {
        sender_id: PlayerId,
        channel: VoiceChannel,
        /// Encoded audio samples — mono, 16-bit signed PCM.
        samples: Vec<i16>,
        /// Sample rate of the PCM data (e.g. 16000).
        sample_rate: u32,
    },

    // Dispatch (55-58)
    DispatchCall {
        call_id: u32,
        call_type: String,
        address: String,
        severity: u8,
        description: String,
        position: Vec3,
        fire_count: u32,
        civilian_count: u32,
        floor_count: u32,
    },
    DispatchUpdate {
        call_id: u32,
        update: String,
    },
    DispatchCancelled {
        call_id: u32,
        reason: String,
    },

    // World state (60-63)
    // Large vector fields use Arc<Vec<T>> so the server can share a single
    // allocation across all clients (via Arc::clone — a cheap refcount bump)
    // instead of cloning the entire Vec for each client. Serde serializes
    // Arc<Vec<T>> identically to Vec<T> (dereferences through the Arc).
    WorldSnapshot {
        tick: u64,
        server_time: f64,
        players: Arc<Vec<PlayerState>>,
        fire_trucks: Arc<Vec<VehicleState>>,
        fire_sources: Arc<Vec<FireSource>>,
        hydrants: Arc<Vec<FireHydrant>>,
        civilians: Arc<Vec<CivilianState>>,
    },
    WorldDelta {
        tick: u64,
        server_time: f64,
        spawned_players: Vec<PlayerId>,
        despawned_players: Vec<PlayerId>,
        updated_players: Vec<PlayerState>,
        updated_trucks: Arc<Vec<VehicleState>>,
        updated_fires: Arc<Vec<FireSource>>,
        extinguished_fires: Vec<u32>,
        updated_hydrants: Arc<Vec<FireHydrant>>,
        updated_civilians: Arc<Vec<CivilianState>>,
        rescued_civilians: Vec<u32>,
    },

    // Effects (70-71)
    SoundEvent {
        event: SoundEvent,
    },
    ParticleEvent {
        event: ParticleEvent,
    },

    // Server admin (90-94)
    ServerMessage {
        text: String,
    },
    PlayerListRequest,
    PlayerListResponse {
        players: Vec<PlayerListEntry>,
    },
    KickPlayer {
        player_id: PlayerId,
        reason: String,
    },
    BanPlayer {
        player_id: PlayerId,
        reason: String,
    },
}

/// Maximum decoded message size in bytes.
/// Raised from 8192 to 65536 to match the increased packet payload limit so
/// large WorldSnapshots are not rejected at the message layer.
const MAX_MESSAGE_SIZE: usize = 65536;

impl NetMessage {
    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    pub fn decode(data: &[u8]) -> Result<Self, Box<bincode::ErrorKind>> {
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(Box::new(bincode::ErrorKind::Custom(format!(
                "Message too large: {} bytes (max {})",
                data.len(),
                MAX_MESSAGE_SIZE
            ))));
        }
        bincode::deserialize(data)
    }

    pub fn is_reliable(&self) -> bool {
        !matches!(
            self,
            NetMessage::PlayerInput { .. } | NetMessage::VoicePacket { .. }
        )
    }
}
