use serde::{Deserialize, Serialize};

/// Unique identifier for an equipment piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EquipmentId(pub u32);

/// Type of tool a firefighter can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolType {
    /// Fire hose — primary fire suppression tool.
    Hose,
    /// Flathead axe — for forcible entry and ventilation.
    Axe,
    /// Pike pole — for pull-down ceiling/wall checks.
    PikePole,
    /// Halligan bar — for forcible entry through doors/locks.
    HalliganBar,
    /// Portable fire extinguisher — for small/incipient fires.
    Extinguisher,
    /// Thermal imaging camera — see through smoke, find hotspots.
    ThermalCamera,
    /// Portable ladder — for access to upper floors/roofs.
    Ladder,
    /// Jaws of life — for rescue/extrication.
    JawsOfLife,
    /// Fire hook — for overhaul operations.
    Hook,
}

impl ToolType {
    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            ToolType::Hose => "Шланг",
            ToolType::Axe => "Топор",
            ToolType::PikePole => "Кирка",
            ToolType::HalliganBar => "Халиган",
            ToolType::Extinguisher => "Огнетушитель",
            ToolType::ThermalCamera => "Термокамера",
            ToolType::Ladder => "Лестница",
            ToolType::JawsOfLife => "Ножницы",
            ToolType::Hook => "Крюк",
        }
    }
}

/// Type of nozzle on a hose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NozzleType {
    /// Smooth bore — solid stream, high reach, deep penetration.
    SmoothBore,
    /// Fog nozzle — wide spray pattern, good for heat protection.
    Fog,
    /// Straight stream — focused stream with adjustable pattern.
    StraightStream,
}

impl NozzleType {
    /// Effective range in meters.
    pub fn range(self) -> f32 {
        match self {
            NozzleType::SmoothBore => 25.0,
            NozzleType::Fog => 10.0,
            NozzleType::StraightStream => 20.0,
        }
    }

    /// Water flow rate in gallons per minute at standard pressure.
    pub fn flow_rate(self) -> f32 {
        match self {
            NozzleType::SmoothBore => 150.0,
            NozzleType::Fog => 100.0,
            NozzleType::StraightStream => 125.0,
        }
    }
}

/// Runtime state of a tool carried by a firefighter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolState {
    /// The tool type.
    pub tool: ToolType,
    /// Current condition/durability (0.0 = broken, 1.0 = perfect).
    pub condition: f32,
}

impl ToolState {
    /// Creates a new tool with perfect condition.
    pub fn new(tool: ToolType) -> Self {
        Self {
            tool,
            condition: 1.0,
        }
    }
}

/// State of a hose line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoseState {
    /// Current water pressure in PSI.
    pub pressure: f32,
    /// Maximum pressure the pump can deliver.
    pub max_pressure: f32,
    /// Current nozzle type.
    pub nozzle: NozzleType,
    /// Whether the hose is currently flowing water.
    pub flowing: bool,
    /// Total length of hose deployed in meters.
    pub length: f32,
    /// Water consumed so far in gallons.
    pub water_used: f32,
}

impl Default for HoseState {
    fn default() -> Self {
        Self::new()
    }
}

impl HoseState {
    /// Creates a new hose state with default values.
    pub fn new() -> Self {
        Self {
            pressure: 100.0,
            max_pressure: 150.0,
            nozzle: NozzleType::StraightStream,
            flowing: false,
            length: 15.0,
            water_used: 0.0,
        }
    }

    /// Updates the hose state by dt seconds.
    pub fn update(&mut self, dt: f32) {
        if self.flowing {
            self.water_used += self.nozzle.flow_rate() / 60.0 * dt;
        }
    }
}

/// Protective gear worn by a firefighter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectiveGear {
    /// SCBA (Self-Contained Breathing Apparatus) state.
    pub scba: ScbaState,
    /// Turnout gear condition (coat, pants, boots, gloves, helmet).
    pub turnout_condition: f32,
}

impl Default for ProtectiveGear {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtectiveGear {
    /// Creates new protective gear with full SCBA and perfect turnout.
    pub fn new() -> Self {
        Self {
            scba: ScbaState::new(),
            turnout_condition: 1.0,
        }
    }
}

/// State of the SCBA (Self-Contained Breathing Apparatus).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScbaState {
    /// Remaining air in the cylinder in psi.
    pub air_pressure: f32,
    /// Maximum cylinder pressure (typically 3000-4500 psi).
    pub max_pressure: f32,
    /// Air consumption rate based on exertion.
    pub consumption_rate: f32,
    /// Whether the firefighter is currently breathing from SCBA.
    pub active: bool,
    /// Warning threshold pressure.
    pub warning_psi: f32,
    /// Alarm has been triggered.
    pub alarm_triggered: bool,
}

impl Default for ScbaState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScbaState {
    /// Creates a new SCBA with a full cylinder.
    pub fn new() -> Self {
        Self {
            air_pressure: 4500.0,
            max_pressure: 4500.0,
            consumption_rate: 40.0, // psi per minute at moderate exertion
            active: true,
            warning_psi: 1000.0,
            alarm_triggered: false,
        }
    }

    /// Updates the SCBA by dt seconds based on exertion level.
    pub fn update(&mut self, dt: f32, exertion: f32) {
        let dt = dt.max(0.0);
        if self.active && self.air_pressure > 0.0 {
            let rate = self.consumption_rate * (1.0 + exertion * 0.5) / 60.0;
            self.air_pressure = (self.air_pressure - rate * dt).max(0.0);
        }
        if self.air_pressure <= self.warning_psi && !self.alarm_triggered {
            self.alarm_triggered = true;
        }
    }

    /// Returns remaining air as a percentage.
    pub fn air_percentage(&self) -> f32 {
        if self.max_pressure > 0.0 {
            self.air_pressure / self.max_pressure * 100.0
        } else {
            0.0
        }
    }

    /// Returns estimated remaining time in seconds.
    pub fn remaining_time_secs(&self, exertion: f32) -> f32 {
        if self.consumption_rate <= 0.0 {
            return f32::INFINITY;
        }
        let rate = self.consumption_rate * (1.0 + exertion * 0.5) / 60.0;
        self.air_pressure / rate
    }
}

/// Complete equipment loadout for a firefighter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentLoadout {
    /// Currently held tool.
    pub held_tool: ToolType,
    /// Tool belt (up to 4 tools).
    pub tool_belt: Vec<ToolState>,
    /// Hose state (if carrying a hose).
    pub hose: Option<HoseState>,
    /// Protective gear worn.
    pub gear: ProtectiveGear,
}

impl EquipmentLoadout {
    /// Creates a default loadout for the given role.
    pub fn default_for_role(role: crate::types::common::FireRole) -> Self {
        let mut belt = Vec::new();
        match role {
            crate::types::common::FireRole::Nozzle => {
                belt.push(ToolState::new(ToolType::Axe));
                belt.push(ToolState::new(ToolType::HalliganBar));
            }
            crate::types::common::FireRole::SearchRescue => {
                belt.push(ToolState::new(ToolType::HalliganBar));
                belt.push(ToolState::new(ToolType::JawsOfLife));
                belt.push(ToolState::new(ToolType::ThermalCamera));
            }
            crate::types::common::FireRole::Ventilation => {
                belt.push(ToolState::new(ToolType::Axe));
                belt.push(ToolState::new(ToolType::PikePole));
                belt.push(ToolState::new(ToolType::Hook));
            }
            crate::types::common::FireRole::PumpOperator => {
                belt.push(ToolState::new(ToolType::ThermalCamera));
            }
            crate::types::common::FireRole::Commander => {
                belt.push(ToolState::new(ToolType::ThermalCamera));
                belt.push(ToolState::new(ToolType::Axe));
            }
        }

        let hose = if matches!(role, crate::types::common::FireRole::Nozzle) {
            Some(HoseState::new())
        } else {
            None
        };

        Self {
            held_tool: if hose.is_some() {
                ToolType::Hose
            } else {
                ToolType::Axe
            },
            tool_belt: belt,
            hose,
            gear: ProtectiveGear::new(),
        }
    }
}
