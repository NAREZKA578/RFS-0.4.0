//! Core library for the RFS (Russian Firefighter Simulator) game.
//!
//! This crate provides the shared data types, networking protocol, and
//! configuration structures used by both the server and client.

pub mod collider;
pub mod commands;
pub mod config;
pub mod net;
pub mod server_discovery;
pub mod spatial;
pub mod types;

pub use glam::{Quat, Vec3};
pub use net::message::NetMessage;

pub mod game_constants {
    /// Default rescue timeout in seconds before a downed firefighter dies.
    pub const RESCUE_TIMEOUT: f32 = 120.0;
    /// Squad respawn delay in seconds.
    pub const SQUAD_RESPAWN_DELAY: f32 = 10.0;
    /// Maximum chat message length in characters.
    pub const MAX_CHAT_LEN: usize = 200;
    /// Maximum player name length in characters.
    pub const MAX_PLAYER_NAME_LEN: usize = 32;
    /// Heartbeat interval in milliseconds.
    pub const HEARTBEAT_INTERVAL_MS: u64 = 1000;
    /// Connection timeout in milliseconds.
    pub const CONNECTION_TIMEOUT_MS: u64 = 5000;
    /// Maximum missed heartbeats before disconnect.
    pub const MAX_HEARTBEAT_MISSES: u32 = 5;
    /// Voice local-channel hearing range in meters.
    pub const VOICE_LOCAL_RANGE: f32 = 40.0;
    /// Seconds without any packet from server before disconnecting.
    pub const DISCONNECT_TIMEOUT_S: u64 = 15;
}

/// Fire simulation constants — shared between client and server to keep
/// the simulation in sync.
pub mod fire_constants {
    /// Fire intensity growth rate multiplier (per fuel burn rate).
    pub const GROWTH_MULT: f32 = 0.5;
    /// Fire intensity threshold below which fire grows.
    pub const GROWTH_THRESHOLD: f32 = 80.0;
    /// Natural fire decay per second when above growth threshold.
    pub const NATURAL_DECAY: f32 = 2.0;
    /// Maximum fire intensity.
    pub const MAX_INTENSITY: f32 = 100.0;
    /// Temperature per intensity point.
    pub const TEMPERATURE_MULT: f32 = 8.0;
    /// Ambient temperature (Celsius).
    pub const AMBIENT_TEMPERATURE: f32 = 20.0;
    /// Heat contribution per intensity point to ambient temperature.
    pub const AMBIENT_HEAT_MULT: f32 = 0.1;
    /// Cooling rate for smoldering embers (°C per second).
    pub const SMOLDER_COOLING_RATE: f32 = 5.0;
    /// Fire radius growth rate (meters per second) when intensity > threshold.
    pub const RADIUS_GROWTH_RATE: f32 = 0.1;
    /// Maximum fire radius in meters.
    pub const MAX_RADIUS: f32 = 15.0;
    /// Intensity threshold above which fire radius grows.
    pub const RADIUS_GROWTH_THRESHOLD: f32 = 50.0;

    // Civilian damage constants
    /// Panic damage radius multiplier (fire radius * mult).
    pub const CIVILIAN_PANIC_RADIUS_MULT: f32 = 2.0;
    /// Unconscious damage radius multiplier (fire radius * mult).
    pub const CIVILIAN_UNCONSCIOUS_RADIUS_MULT: f32 = 3.0;
    /// Panic civilian damage per second near fire.
    pub const CIVILIAN_PANIC_DAMAGE_RATE: f32 = 10.0;
    /// Unconscious civilian damage per second near fire.
    pub const CIVILIAN_UNCONSCIOUS_DAMAGE_RATE: f32 = 15.0;
}
