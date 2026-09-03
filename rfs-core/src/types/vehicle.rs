use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// Unique identifier for a fire truck instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VehicleId(pub u32);

/// Type of fire truck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TruckType {
    /// Pumper/engine — carries water tank and pump.
    Engine,
    /// Ladder truck — aerial ladder for upper floors.
    Ladder,
    /// Rescue truck — specialized rescue equipment.
    Rescue,
    /// Water tanker — large water supply.
    Tanker,
}

/// Authoritative state of a fire truck, replicated from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleState {
    pub vehicle_id: VehicleId,
    pub truck_type: TruckType,
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    pub engine_on: bool,
    /// Water tank level in gallons (0.0 to max_capacity).
    pub water_level: f32,
    /// Maximum tank capacity in gallons.
    pub max_capacity: f32,
    /// Current pump pressure in PSI.
    pub pump_pressure: f32,
    /// Whether the pump is active.
    pub pump_active: bool,
    /// Whether connected to a hydrant.
    pub hydrant_connected: bool,
    /// Current ladder extension in meters (0.0 = retracted).
    pub ladder_extension: f32,
    /// Ladder rotation in degrees.
    pub ladder_angle: f32,
}
