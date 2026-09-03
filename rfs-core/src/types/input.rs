use glam::{Vec2, Vec3};

/// A single frame of player input sent from client to server.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InputFrame {
    /// Monotonically increasing sequence number.
    pub sequence: u32,
    /// Time elapsed since the previous input frame in seconds.
    pub delta_time: f32,
    /// Mouse/view angles (pitch, yaw).
    pub view_angles: Vec2,
    /// Player's reported position.
    pub position: Vec3,
    /// Movement input state.
    pub movement: InputMovement,
    /// Aiming input state.
    pub aim: InputAim,
    /// Action input state (fire, reload, interact).
    pub actions: InputActions,
}

impl InputFrame {
    /// Creates a new input frame with the given sequence number and timestamp.
    pub fn new(sequence: u32, _total_time: f64) -> Self {
        Self {
            sequence,
            delta_time: 0.0,
            view_angles: Vec2::ZERO,
            position: Vec3::ZERO,
            movement: InputMovement::default(),
            aim: InputAim::default(),
            actions: InputActions::default(),
        }
    }

    /// Computes the normalized movement direction vector from movement inputs.
    pub fn movement_vector(&self) -> Vec3 {
        let mut dir = Vec3::ZERO;
        if self.movement.forward {
            dir.z -= 1.0;
        }
        if self.movement.backward {
            dir.z += 1.0;
        }
        if self.movement.left {
            dir.x -= 1.0;
        }
        if self.movement.right {
            dir.x += 1.0;
        }
        if dir.length_squared() > 0.0 {
            dir = dir.normalize();
        }
        dir
    }
}

/// Movement-related input flags for a single frame.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct InputMovement {
    /// Move forward.
    pub forward: bool,
    /// Move backward.
    pub backward: bool,
    /// Strafe left.
    pub left: bool,
    /// Strafe right.
    pub right: bool,
    /// Sprint modifier active.
    pub sprint: bool,
    /// Walk modifier active.
    pub walk: bool,
    /// Jump.
    pub jump: bool,
    /// Crouch toggle.
    pub crouch: bool,
}

/// Aiming input state for a single frame.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct InputAim {
    /// Whether the player is aiming down sights.
    pub aiming: bool,
}

/// Action input state for a single frame.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct InputActions {
    /// Primary fire button pressed (hose spray).
    pub primary_fire: bool,
    /// Interact button pressed.
    pub interact: bool,
}
