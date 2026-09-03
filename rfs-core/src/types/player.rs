pub use crate::types::common::Stance;
use crate::types::health::HealthState;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a player in the game session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u32);

impl PlayerId {
    pub fn invalid() -> Self {
        PlayerId(u32::MAX)
    }
}

impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Authoritative state of a player, replicated from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    pub player_id: PlayerId,
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    pub health_state: HealthState,
    pub stance: Stance,
    pub is_grounded: bool,
    /// Currently held tool index.
    pub held_tool: u8,
    /// Role of this firefighter.
    pub role: crate::types::common::FireRole,
    /// Whether this player is carrying a civilian.
    pub carrying_civilian: bool,
    /// Whether the player is currently alive (dead players show as downed).
    pub is_alive: bool,
    /// The last client input sequence applied to this player's state.
    /// Lets the client reconcile against the exact prediction it recorded for
    /// that sequence instead of a state that is RTT-age newer ([DA2]).
    pub input_seq: u32,
}
