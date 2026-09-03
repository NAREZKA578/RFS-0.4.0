/// Complete status of a firefighter, including oxygen, heat exposure, fatigue, SCBA.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthState {
    /// Current hit points (overall physical health).
    pub current: f32,
    /// Maximum hit points.
    pub maximum: f32,
    /// Whether the player is incapacitated (heat stroke, smoke inhalation, etc).
    pub is_incapacitated: bool,
    /// Whether the player is dead.
    pub is_dead: bool,
    /// Seconds until the firefighter can be rescued.
    pub rescue_timer: f32,
    /// Heat exposure level (0.0 = none, 100.0 = critical).
    pub heat_exposure: f32,
    /// Heat damage rate per second when above threshold.
    pub heat_damage_rate: f32,
    /// Smoke inhalation level (0.0 = none, 100.0 = critical).
    pub smoke_inhalation: f32,
    /// Whether wearing SCBA mask properly.
    pub scba_sealed: bool,
    /// Fatigue level (0.0 = fresh, 100.0 = exhausted).
    pub fatigue: f32,
    /// Fatigue drain rate per second during heavy activity.
    pub fatigue_drain_rate: f32,
    /// Whether the firefighter is currently downed.
    pub is_downed: bool,
    /// Progress toward being rescued by another firefighter (0.0 to 1.0).
    pub rescue_progress: f32,
    /// Current SCBA air pressure (PSI).
    pub scba_pressure: f32,
    /// Maximum SCBA air pressure (PSI).
    pub scba_max_pressure: f32,
    /// Whether SCBA low-pressure alarm is active.
    pub scba_alarm: bool,
    /// Current water hose pressure (bar).
    pub water_pressure: f32,
    /// Stamina state (for sprinting / heavy activity).
    pub stamina: StaminaState,
    /// Whether a leg injury (fracture) reduces movement speed.
    pub leg_fractured: bool,
    /// Progress toward being revived when downed (0.0 to 1.0).
    pub revive_progress: f32,
    /// The damage type that most recently caused significant harm, used for incapacitation cause.
    pub last_damage_type: DamageType,
}

/// Stamina state for physical activity tracking.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StaminaState {
    /// Current stamina level (0.0 to max).
    pub current: f32,
    /// Maximum stamina.
    pub max: f32,
    /// Stamina drain rate when sprinting.
    pub sprint_drain_rate: f32,
    /// Stamina regeneration rate when standing still.
    pub regen_standing_rate: f32,
    /// Stamina regeneration rate when walking.
    pub regen_walking_rate: f32,
    /// Delay before stamina regen starts after sprinting (seconds).
    pub regen_delay: f32,
    /// Current regen delay timer.
    pub regen_timer: f32,
    /// Whether stamina is fully depleted (exhausted).
    pub exhausted: bool,
}

impl Default for StaminaState {
    fn default() -> Self {
        Self::new()
    }
}

impl StaminaState {
    /// Creates a full stamina state.
    pub fn new() -> Self {
        Self {
            current: 100.0,
            max: 100.0,
            sprint_drain_rate: 25.0,
            regen_standing_rate: 12.0,
            regen_walking_rate: 6.0,
            regen_delay: 1.5,
            regen_timer: 0.0,
            exhausted: false,
        }
    }

    /// Returns true if the character can sprint.
    pub fn can_sprint(&self) -> bool {
        self.current > 0.0 && !self.exhausted
    }

    /// Returns the speed multiplier based on stamina level.
    pub fn speed_penalty(&self) -> f32 {
        if self.exhausted {
            0.7
        } else {
            0.9 + 0.1 * (self.current / self.max)
        }
    }
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthState {
    /// Creates a new health state with full vitals.
    pub fn new() -> Self {
        Self {
            current: 100.0,
            maximum: 100.0,
            is_incapacitated: false,
            is_dead: false,
            rescue_timer: 120.0,
            heat_exposure: 0.0,
            heat_damage_rate: 5.0,
            smoke_inhalation: 0.0,
            scba_sealed: true,
            fatigue: 0.0,
            fatigue_drain_rate: 8.0,
            is_downed: false,
            rescue_progress: 0.0,
            scba_pressure: 4500.0,
            scba_max_pressure: 4500.0,
            scba_alarm: false,
            water_pressure: 0.0,
            stamina: StaminaState::new(),
            leg_fractured: false,
            revive_progress: 0.0,
            last_damage_type: DamageType::Unknown,
        }
    }

    /// Updates the health state by dt seconds.
    /// Thresholds match client constants for consistency.
    pub fn update(&mut self, dt: f32) {
        // Light heat exposure (threshold = 30.0, matching HEAT_LIGHT_THRESHOLD)
        if self.heat_exposure > 30.0 {
            let dmg = 2.0 * ((self.heat_exposure - 30.0) / 70.0) * dt;
            self.current = (self.current - dmg).max(0.0);
        }
        // Heavy heat exposure (threshold = 70.0, matching HEAT_HEAVY_THRESHOLD)
        if self.heat_exposure > 70.0 {
            let dmg = self.heat_damage_rate * (self.heat_exposure / 100.0) * dt;
            self.current = (self.current - dmg).max(0.0);
        }

        // Smoke inhalation (reduced if SCBA is sealed)
        if !self.scba_sealed && self.smoke_inhalation < 100.0 {
            self.smoke_inhalation = (self.smoke_inhalation + 5.0 * dt).min(100.0);
        } else if self.scba_sealed && self.smoke_inhalation > 0.0 {
            self.smoke_inhalation = (self.smoke_inhalation - 2.0 * dt).max(0.0);
        }

        // Heavy smoke causes HP damage (threshold = 80.0, matching SMOKE_HEAVY_THRESHOLD)
        if self.smoke_inhalation > 80.0 {
            let dmg = 5.0 * (self.smoke_inhalation / 100.0) * dt;
            self.current = (self.current - dmg).max(0.0);
        } else if self.smoke_inhalation > 50.0 {
            // Light smoke damage (threshold = 50.0, matching SMOKE_LIGHT_THRESHOLD)
            let dmg = 2.0 * ((self.smoke_inhalation - 50.0) / 50.0) * dt;
            self.current = (self.current - dmg).max(0.0);
        }

        // Heat exposure slowly decreases if not in hot zone
        if self.heat_exposure > 0.0 {
            self.heat_exposure = (self.heat_exposure - 1.0 * dt).max(0.0);
        }

        // Check incapacitation
        if self.current <= 0.0 && !self.is_incapacitated {
            self.is_incapacitated = true;
            self.is_downed = true;
            self.rescue_timer = 120.0;
        }

        // Downed rescue countdown
        if self.is_downed {
            self.rescue_timer -= dt;
            if self.rescue_timer <= 0.0 {
                self.is_dead = true;
            }
        }
    }

    /// Returns true if the firefighter can still operate.
    pub fn is_operational(&self) -> bool {
        !self.is_incapacitated && !self.is_dead
    }

    /// Returns true if the firefighter is in danger.
    pub fn is_in_danger(&self) -> bool {
        self.heat_exposure > 70.0 || self.smoke_inhalation > 80.0 || self.current < 30.0
    }

    /// Applies heat damage from environment.
    pub fn apply_heat(&mut self, intensity: f32, dt: f32) {
        let gear_protection = 0.3; // turnout gear reduces heat
        let effective = intensity * (1.0 - gear_protection);
        self.heat_exposure = (self.heat_exposure + effective * dt).min(100.0);
        if effective * dt > 0.5 {
            self.last_damage_type = DamageType::Heat;
        }
    }

    /// Applies smoke inhalation.
    pub fn apply_smoke(&mut self, density: f32, dt: f32) {
        if !self.scba_sealed {
            self.smoke_inhalation = (self.smoke_inhalation + density * dt).min(100.0);
            if density * dt > 0.5 {
                self.last_damage_type = DamageType::Smoke;
            }
        }
    }

    /// Increases fatigue from heavy activity.
    pub fn apply_fatigue(&mut self, dt: f32) {
        self.fatigue = (self.fatigue + self.fatigue_drain_rate * dt).min(100.0);
    }

    /// Restores fatigue when resting.
    pub fn rest(&mut self, dt: f32) {
        self.fatigue = (self.fatigue - 15.0 * dt).max(0.0);
    }

    /// Heals HP (e.g. medical treatment on scene).
    /// Also clears the downed/incapacitated state if HP is restored above zero.
    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.maximum);
        if self.current > 0.0 && self.is_downed {
            self.is_downed = false;
            self.is_incapacitated = false;
            self.is_dead = false;
            self.rescue_timer = 120.0;
        }
    }

    /// Returns movement speed multiplier based on fatigue and injury.
    pub fn speed_factor(&self) -> f32 {
        let fatigue_penalty = 1.0 - (self.fatigue / 100.0) * 0.4;
        let injury_penalty = if self.current < 30.0 { 0.6 } else { 1.0 };
        fatigue_penalty * injury_penalty
    }
}

/// The source or type of hazard that caused damage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DamageType {
    /// Direct fire/flame contact.
    Fire,
    /// Extreme heat without direct flame.
    Heat,
    /// Smoke inhalation.
    Smoke,
    /// Structural collapse or falling debris.
    Collapse,
    /// Falling from height.
    Fall,
    /// Explosion (gas, chemicals).
    Explosion,
    /// Electrical hazard.
    Electrical,
    /// Unknown or miscellaneous.
    Unknown,
}
