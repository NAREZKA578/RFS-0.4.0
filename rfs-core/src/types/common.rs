/// A player's stance, affecting height and movement speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Stance {
    /// Standing upright.
    Standing = 0,
    /// Crouching (reduced height, slower movement).
    Crouching = 1,
}

impl Stance {
    /// Movement speed multiplier for this stance.
    pub fn speed_factor(self) -> f32 {
        match self {
            Stance::Standing => 1.0,
            Stance::Crouching => 0.6,
        }
    }

    /// Collision height in meters for this stance.
    pub fn height(self) -> f32 {
        match self {
            Stance::Standing => 1.8,
            Stance::Crouching => 1.0,
        }
    }

    /// Human-readable name of the stance.
    pub fn name(self) -> &'static str {
        match self {
            Stance::Standing => "Standing",
            Stance::Crouching => "Crouching",
        }
    }
}

/// Role of a firefighter in the crew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FireRole {
    /// Incident commander — coordinates operations.
    Commander,
    /// Pump operator — controls water pressure and supply.
    PumpOperator,
    /// Nozzle operator — handles the hose and fights fire.
    Nozzle,
    /// Search and rescue — finds and evacuates civilians.
    SearchRescue,
    /// Ventilation — clears smoke, opens entry points.
    Ventilation,
}

impl FireRole {
    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            FireRole::Commander => "Commander",
            FireRole::PumpOperator => "Pump Operator",
            FireRole::Nozzle => "Nozzle Operator",
            FireRole::SearchRescue => "Search & Rescue",
            FireRole::Ventilation => "Ventilation",
        }
    }
}

/// Chat channel for player communication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ChatChannel {
    /// Proximity chat (nearby players only).
    Local,
    /// Crew-only chat (your fire crew).
    Crew,
    /// Squad-only chat.
    Squad,
    /// Team-wide chat (all units on scene).
    Team,
    /// Dispatch radio channel.
    Dispatch,
    /// Admin/server message channel.
    Admin,
}

/// Voice chat channel — Squad-style push-to-talk channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VoiceChannel {
    /// Local proximity voice (nearby players only, ~40m).
    Local,
    /// Squad voice (your fire crew / squad).
    Squad,
    /// Radio voice (team-wide / all units).
    Radio,
}

impl VoiceChannel {
    /// Human-readable name of the channel.
    pub fn name(self) -> &'static str {
        match self {
            VoiceChannel::Local => "Local",
            VoiceChannel::Squad => "Squad",
            VoiceChannel::Radio => "Radio",
        }
    }
}
