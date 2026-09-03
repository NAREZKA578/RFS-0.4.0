use glam::Vec3;

/// A fire hydrant on the map.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FireHydrant {
    pub id: u32,
    pub position: Vec3,
    /// Flow rate in gallons per minute.
    pub flow_rate: f32,
    /// Whether this hydrant is currently in use.
    pub in_use: bool,
    /// Player currently connected to this hydrant, if any.
    pub connected_by: Option<u32>,
}

/// Extinguishing agent applied to a fire — different agents work on different fire classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExtinguishingAgent {
    /// Water — cools and smothers (Class A fires).
    Water,
    /// Foam — smothers and prevents re-ignition (Class B liquids).
    Foam,
    /// Dry chemical powder — interrupts chain reaction (Class ABC).
    Powder,
    /// CO2 — displaces oxygen (Class B, electrical).
    CO2,
    /// Sand/dirt — smothers (small Class B, metals).
    Sand,
}

impl ExtinguishingAgent {
    /// Effectiveness multiplier against each fuel type.
    pub fn effectiveness(self, fuel: FuelType) -> f32 {
        use ExtinguishingAgent::*;
        use FuelType::*;
        match (self, fuel) {
            // Water works well on ordinary combustibles, dangerous on some others
            (Water, Wood) => 1.0,
            (Water, Rubber) => 0.7,
            (Water, Plastic) => 0.8,
            (Water, Gas) => 0.1,      // Water spreads gas fires
            (Water, Liquid) => 0.1,   // Water spreads liquid fires
            (Water, Chemical) => 0.0, // Water can cause explosions with chemicals
            (Water, Electric) => 0.0, // Water conducts electricity

            // Foam excellent for liquids, good for ordinary combustibles
            (Foam, Wood) => 0.6,
            (Foam, Rubber) => 0.8,
            (Foam, Plastic) => 0.7,
            (Foam, Gas) => 0.3,
            (Foam, Liquid) => 1.0,
            (Foam, Chemical) => 0.5,
            (Foam, Electric) => 0.2,

            // Powder works on everything but doesn't cool well (re-ignition risk)
            (Powder, Wood) => 0.7,
            (Powder, Rubber) => 0.8,
            (Powder, Plastic) => 0.8,
            (Powder, Gas) => 0.9,
            (Powder, Liquid) => 0.9,
            (Powder, Chemical) => 0.7,
            (Powder, Electric) => 1.0,

            // CO2 — good for electrical and liquids, poor on deep-seated fires
            (CO2, Wood) => 0.3,
            (CO2, Rubber) => 0.4,
            (CO2, Plastic) => 0.4,
            (CO2, Gas) => 0.7,
            (CO2, Liquid) => 0.8,
            (CO2, Chemical) => 0.6,
            (CO2, Electric) => 1.0,

            // Sand — smothers, good for small metal/liquid fires
            (Sand, Wood) => 0.4,
            (Sand, Rubber) => 0.5,
            (Sand, Plastic) => 0.5,
            (Sand, Gas) => 0.6,
            (Sand, Liquid) => 0.7,
            (Sand, Chemical) => 0.4,
            (Sand, Electric) => 0.6,
        }
    }

    /// Whether this agent prevents re-ignition (seals surface / cools).
    pub fn prevents_reignition(self) -> bool {
        matches!(self, ExtinguishingAgent::Water | ExtinguishingAgent::Foam)
    }

    /// Whether this agent cools the fuel (prevents smoldering).
    pub fn cools(self) -> bool {
        matches!(self, ExtinguishingAgent::Water)
    }
}

/// State of a fire source in the world.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FireSource {
    pub id: u32,
    pub position: Vec3,
    /// Fire intensity 0.0 (embers) to 100.0 (fully developed).
    pub intensity: f32,
    /// Radius of the fire in meters.
    pub radius: f32,
    /// Internal temperature of the fuel mass in degrees Celsius.
    pub temperature: f32,
    /// Type of material burning.
    pub fuel_type: FuelType,
    /// Whether this fire has been knocked down (suppressed but may smolder).
    pub knocked_down: bool,
    /// Whether this fire has been fully extinguished.
    pub extinguished: bool,
    /// Whether the fire is smolding — hot embers but no active flames.
    pub smoldering: bool,
    /// Last extinguishing agent applied (affects re-ignition behavior).
    pub last_agent: Option<ExtinguishingAgent>,
    /// Time remaining for smoldering phase (seconds). 0 = fully cold.
    pub smolder_time: f32,
    /// Ignition temperature for this fuel type (°C) — fire reignites above this.
    pub ignition_point: f32,
}

impl FuelType {
    /// Burn rate multiplier.
    pub fn burn_rate(self) -> f32 {
        match self {
            FuelType::Wood => 1.0,
            FuelType::Gas => 3.0,
            FuelType::Electric => 1.5,
            FuelType::Liquid => 2.5,
            FuelType::Chemical => 2.0,
            FuelType::Rubber => 1.8,
            FuelType::Plastic => 2.2,
        }
    }

    /// Minimum temperature (°C) at which this fuel auto-ignites.
    pub fn auto_ignition_temp(self) -> f32 {
        match self {
            FuelType::Wood => 300.0,
            FuelType::Gas => 500.0,
            FuelType::Electric => 600.0, // insulation melts/ignites
            FuelType::Liquid => 350.0,   // flash point varies
            FuelType::Chemical => 400.0,
            FuelType::Rubber => 320.0,
            FuelType::Plastic => 350.0,
        }
    }

    /// Temperature (°C) below which this fuel stops smoldering.
    pub fn smolder_stop_temp(self) -> f32 {
        match self {
            FuelType::Wood => 150.0,
            FuelType::Gas => 200.0,
            FuelType::Electric => 250.0,
            FuelType::Liquid => 180.0,
            FuelType::Chemical => 220.0,
            FuelType::Rubber => 170.0,
            FuelType::Plastic => 190.0,
        }
    }

    /// How long (seconds) this fuel can smolder before going cold.
    pub fn smolder_duration(self) -> f32 {
        match self {
            FuelType::Wood => 600.0,     // Wood embers last a long time
            FuelType::Gas => 30.0,       // Gas dissipates quickly
            FuelType::Electric => 120.0, // Plastics/insulation
            FuelType::Liquid => 60.0,    // Liquid evaporates
            FuelType::Chemical => 300.0, // Chemicals can persist
            FuelType::Rubber => 400.0,   // Rubber smolders
            FuelType::Plastic => 350.0,  // Plastics melt and smolder
        }
    }
}

impl FireSource {
    /// Creates a new fire source with realistic initial values.
    pub fn new(id: u32, position: Vec3, fuel_type: FuelType, initial_intensity: f32) -> Self {
        Self {
            id,
            position,
            intensity: initial_intensity,
            radius: 1.0,
            temperature: initial_intensity * 8.0,
            fuel_type,
            knocked_down: false,
            extinguished: false,
            smoldering: false,
            last_agent: None,
            smolder_time: 0.0,
            ignition_point: fuel_type.auto_ignition_temp(),
        }
    }

    /// Creates a fire source at default position.
    pub fn at_origin(id: u32, fuel_type: FuelType, initial_intensity: f32) -> Self {
        Self::new(id, Vec3::ZERO, fuel_type, initial_intensity)
    }
}

/// Type of material fueling a fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum FuelType {
    #[default]
    Wood,
    Gas,
    Electric,
    Liquid,
    Chemical,
    Rubber,
    Plastic,
}

impl std::fmt::Display for FuelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::str::FromStr for FuelType {
    type Err = String;

    /// Parses a fuel type from its debug-style name (case-insensitive),
    /// e.g. "wood", "Wood", "electric", "rubber".
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "wood" => Ok(FuelType::Wood),
            "gas" | "gasoline" => Ok(FuelType::Gas),
            "electric" | "electrical" => Ok(FuelType::Electric),
            "liquid" => Ok(FuelType::Liquid),
            "chemical" | "hazmat" => Ok(FuelType::Chemical),
            "rubber" => Ok(FuelType::Rubber),
            "plastic" => Ok(FuelType::Plastic),
            other => Err(format!("unknown fuel type: '{}'", other)),
        }
    }
}

/// A civilian (NPC) that needs rescue.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CivilianState {
    pub id: u32,
    pub position: Vec3,
    pub status: CivilianStatus,
    /// Health of the civilian (0-100).
    pub health: f32,
    /// Whether the civilian is conscious.
    pub conscious: bool,
    /// Player carrying this civilian, if any.
    pub carried_by: Option<u32>,
}

/// Current status of a civilian.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CivilianStatus {
    /// Panicking, running around.
    Panicked,
    /// Hiding in place.
    Hiding,
    /// Trapped and cannot move.
    Trapped,
    /// Unconscious.
    Unconscious,
    /// Being carried by a firefighter.
    Carried,
    /// Rescued and safe.
    Rescued,
}
