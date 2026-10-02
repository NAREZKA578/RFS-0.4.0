use rfs_core::entity::{StationTemplate, StationType, EntityId, Transform, Bounds};
use rfs_core::math::Vec3f;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationConfig {
    pub station_type: StationType,
    pub compartment_name: String,
    pub local_transform: Transform,
    pub max_ammo: u32,
    pub default_ammo_type: u8,
    pub reload_time: f32,
    pub max_health: f32,
    pub yaw_range: (f32, f32),
    pub pitch_range: (f32, f32),
    pub turn_speed: f32,
    pub fire_rate: f32,
}

impl Default for StationConfig {
    fn default() -> Self {
        Self {
            station_type: StationType::Gun,
            compartment_name: "main_deck".to_string(),
            local_transform: Transform::IDENTITY,
            max_ammo: 100,
            default_ammo_type: 0,
            reload_time: 10.0,
            max_health: 500.0,
            yaw_range: (-90.0_f32.to_radians(), 90.0_f32.to_radians()),
            pitch_range: (-30.0_f32.to_radians(), 45.0_f32.to_radians()),
            turn_speed: 1.0,
            fire_rate: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationState {
    pub station_type: StationType,
    pub occupant: Option<EntityId>,
    pub yaw: f32,
    pub pitch: f32,
    pub target_yaw: f32,
    pub target_pitch: f32,
    pub reload_progress: f32,
    pub ammo_type: u8,
    pub ammo_count: u32,
    pub max_ammo: u32,
    pub is_operational: bool,
    pub health: f32,
    pub max_health: f32,
    pub cooldown: f32,
    pub last_fire_time: f32,
}

impl Default for StationState {
    fn default() -> Self {
        Self {
            station_type: StationType::Gun,
            occupant: None,
            yaw: 0.0,
            pitch: 0.0,
            target_yaw: 0.0,
            target_pitch: 0.0,
            reload_progress: 1.0,
            ammo_type: 0,
            ammo_count: 100,
            max_ammo: 100,
            is_operational: true,
            health: 500.0,
            max_health: 500.0,
            cooldown: 0.0,
            last_fire_time: 0.0,
        }
    }
}

/// A station is a config holder. All mutable state lives in
/// `ShipState::station_states` (single source of truth, plan §3.x / bug №53):
/// the tick copy is what the physics tick mutates, what `fire`/`reload`/
/// occupancy mutate, and what gets replicated.
pub struct Station {
    config: StationConfig,
    compartment_id: EntityId,
    entity_id: EntityId,
    ship_id: EntityId,
}

impl Station {
    pub fn new(entity_id: EntityId, ship_id: EntityId, template: StationTemplate) -> Self {
        let config = StationConfig {
            station_type: template.station_type,
            compartment_name: template.compartment_name,
            local_transform: template.local_transform,
            max_ammo: template.max_ammo,
            default_ammo_type: template.default_ammo_type,
            // Bug №56: honor the declared ranges verbatim — the old code
            // mirrored them off the upper bound, throwing away an asymmetric
            // sector's lower edge (yaw_range.0 was ignored).
            reload_time: template.cooldown.max(0.1),
            max_health: template.max_health,
            yaw_range: template.yaw_range,
            pitch_range: template.pitch_range,
            turn_speed: 2.0,
            fire_rate: 1.0 / template.cooldown.max(0.1),
        };

        Self {
            config,
            compartment_id: EntityId::new(0),
            entity_id,
            ship_id,
        }
    }

    pub fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    pub fn ship_id(&self) -> EntityId {
        self.ship_id
    }

    pub fn compartment_id(&self) -> EntityId {
        self.compartment_id
    }

    pub fn set_compartment_id(&mut self, id: EntityId) {
        self.compartment_id = id;
    }

    pub fn config(&self) -> &StationConfig {
        &self.config
    }

    pub fn update(&self, dt: f32, state: &mut StationState, compartment_state: Option<&crate::compartment::CompartmentState>) {
        if state.health <= 0.0 {
            return;
        }

        // Bug №56: operational-ness is re-derived from the compartment every
        // tick — no latch. Pump the water out / quench the fire and the
        // station comes back by itself.
        let compartment_dead = compartment_state
            .is_some_and(|c| c.is_flooded() || c.fire_intensity > 0.5);
        state.is_operational = !compartment_dead;
        if !state.is_operational {
            return;
        }

        state.cooldown = (state.cooldown - dt).max(0.0);

        let yaw_diff = state.target_yaw - state.yaw;
        let pitch_diff = state.target_pitch - state.pitch;

        // Bug №56: step capped at |diff| — the old `signum() * turn_speed`
        // overshot through the target and jittered forever.
        let max_step = self.config.turn_speed * dt;
        state.yaw += yaw_diff.clamp(-max_step, max_step);
        state.pitch += pitch_diff.clamp(-max_step, max_step);
        
        // Safe clamp: inverted (min>max) or non-finite ranges from JSON must
        // not panic (f32::clamp panics on min>max). Fall back to current value.
        fn safe_clamp(v: f32, lo: f32, hi: f32) -> f32 {
            if !v.is_finite() {
                return 0.0;
            }
            if !lo.is_finite() || !hi.is_finite() || lo > hi {
                return v;
            }
            v.clamp(lo, hi)
        }
        state.yaw = safe_clamp(state.yaw, self.config.yaw_range.0, self.config.yaw_range.1);
        state.pitch = safe_clamp(
            state.pitch,
            self.config.pitch_range.0,
            self.config.pitch_range.1,
        );

        if state.reload_progress < 1.0 {
            state.reload_progress += dt / self.config.reload_time;
            if state.reload_progress >= 1.0 {
                state.reload_progress = 1.0;
                // Bug №158: auto-reload — a completed cycle loads one round
                // without a manual Reload command, so a fired gun re-arms
                // itself. The cycle repeats until the magazine is full.
                //
                // Bug №158 (regression): the reset to 0.0 was unconditional, so
                // even the cycle that *filled* the magazine immediately started
                // another one. The station therefore only became "ready" a full
                // reload_time after its magazine was already full — `can_fire`
                // requires `reload_progress >= 1.0`, so a gun fired once stayed
                // dead for twice the reload window. Reset only while the
                // magazine still has room; otherwise stay loaded and ready.
                if state.ammo_count < state.max_ammo {
                    state.ammo_count += 1;
                    if state.ammo_count < state.max_ammo {
                        state.reload_progress = 0.0;
                    }
                }
            }
        }
    }

    pub fn set_target_angles(&self, state: &mut StationState, yaw: f32, pitch: f32) {
        fn safe_clamp(v: f32, lo: f32, hi: f32) -> f32 {
            if !v.is_finite() {
                return 0.0;
            }
            if !lo.is_finite() || !hi.is_finite() || lo > hi {
                return v;
            }
            v.clamp(lo, hi)
        }
        state.target_yaw = safe_clamp(yaw, self.config.yaw_range.0, self.config.yaw_range.1);
        state.target_pitch = safe_clamp(
            pitch,
            self.config.pitch_range.0,
            self.config.pitch_range.1,
        );
    }

    pub fn get_angles(&self, state: &StationState) -> (f32, f32) {
        (state.yaw, state.pitch)
    }

    pub fn can_fire(&self, state: &StationState) -> bool {
        state.is_operational &&
        state.health > 0.0 &&
        state.cooldown <= 0.0 &&
        state.reload_progress >= 1.0 &&
        state.ammo_count > 0
    }

    pub fn fire(&self, state: &mut StationState, ammo_type: Option<u8>) -> Option<FireResult> {
        if !self.can_fire(state) {
            return None;
        }

        if let Some(at) = ammo_type {
            state.ammo_type = at;
        }
        
        state.ammo_count = state.ammo_count.saturating_sub(1);
        state.reload_progress = 0.0;
        state.cooldown = 1.0 / self.config.fire_rate;
        state.last_fire_time = 0.0;
        
        let direction = self.calculate_fire_direction(state.yaw, state.pitch);
        
        Some(FireResult {
            station_id: self.entity_id,
            position: self.local_transform().position,
            direction,
            velocity: direction * self.get_muzzle_velocity(),
            projectile_type: self.ammo_type_to_projectile(state.ammo_type),
            damage: self.get_damage(),
            penetration: self.get_penetration(),
        })
    }

    fn calculate_fire_direction(&self, yaw: f32, pitch: f32) -> Vec3f {
        let cp = pitch.cos();
        Vec3f::new(
            cp * yaw.sin(),
            pitch.sin(),
            cp * yaw.cos(),
        )
    }

    fn get_muzzle_velocity(&self) -> f32 {
        match self.config.station_type {
            StationType::Gun => 400.0,
            StationType::Engine => 0.0,
            StationType::Pump => 0.0,
            StationType::Helm => 0.0,
            _ => 200.0,
        }
    }

    fn get_damage(&self) -> f32 {
        match self.config.station_type {
            StationType::Gun => 100.0,
            _ => 0.0,
        }
    }

    fn get_penetration(&self) -> f32 {
        match self.config.station_type {
            StationType::Gun => 50.0,
            _ => 0.0,
        }
    }

    fn ammo_type_to_projectile(&self, ammo_type: u8) -> rfs_core::packet::ProjectileType {
        match ammo_type {
            0 => rfs_core::packet::ProjectileType::Cannonball,
            1 => rfs_core::packet::ProjectileType::ExplosiveShell,
            2 => rfs_core::packet::ProjectileType::ArmorPiercing,
            3 => rfs_core::packet::ProjectileType::ChainShot,
            4 => rfs_core::packet::ProjectileType::GrapeShot,
            _ => rfs_core::packet::ProjectileType::Cannonball,
        }
    }

    pub fn reload(&self, state: &mut StationState, ammo_type: u8) {
        // Bug №84: finishing one reload cycle actually loads a shell. The old
        // code reset the progress and swapped ammo_type but never touched
        // ammo_count, so a fired gun stayed empty forever (progress reset
        // endlessly with ammo_count < max_ammo never becoming false).
        if state.ammo_count < state.max_ammo && state.reload_progress >= 1.0 {
            state.ammo_count += 1;
            state.ammo_type = ammo_type;
            state.reload_progress = 0.0;
        }
    }

    pub fn repair(&self, state: &mut StationState, amount: f32) {
        // Negative repairs must not damage the station.
        state.health = (state.health + amount.max(0.0)).min(state.max_health);
        if state.health > 0.0 {
            state.is_operational = true;
        }
    }

    pub fn apply_damage(&self, state: &mut StationState, damage: f32) {
        // Clamped both ways: negative damage must not overheal above max.
        state.health = (state.health - damage).clamp(0.0, state.max_health);
        if state.health <= 0.0 {
            state.is_operational = false;
            state.occupant = None;
        }
    }

    pub fn can_occupy(&self, state: &StationState) -> bool {
        state.is_operational && state.health > 0.0 && state.occupant.is_none()
    }

    pub fn get_occupant(&self, state: &StationState) -> Option<EntityId> {
        state.occupant
    }

    pub fn local_transform(&self) -> Transform {
        self.config.local_transform
    }

    pub fn bounds(&self) -> Bounds {
        let half_size = Vec3f::new(2.0, 2.0, 2.0);
        let pos = self.local_transform().position;
        Bounds::new(pos - half_size, pos + half_size)
    }

    pub fn station_type(&self) -> StationType {
        self.config.station_type
    }

    pub fn set_default_behavior(&self, state: &mut StationState, behavior: DefaultBehavior) {
        state.is_operational = matches!(behavior, DefaultBehavior::Active);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireResult {
    pub station_id: EntityId,
    pub position: Vec3f,
    pub direction: Vec3f,
    pub velocity: Vec3f,
    pub projectile_type: rfs_core::packet::ProjectileType,
    pub damage: f32,
    pub penetration: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefaultBehavior {
    Active,
    Passive,
    Disabled,
}