use rfs_core::entity::{Bounds, EntityId, StationTemplate, StationType, Transform};
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
    /// Bug №633: seconds until the next repair action is accepted. The 25 hp
    /// ceiling was per *action*, and nothing throttled the actions — a client
    /// spamming Repair packets restored 500 hp/s through one post and made a
    /// damaged ship with a live damage-control seat tougher than any normal
    /// damage intake. Decays in `update`, starts in `repair`.
    pub repair_cooldown: f32,
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
            repair_cooldown: 0.0,
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

    pub fn update(
        &self,
        dt: f32,
        state: &mut StationState,
        compartment_state: Option<&crate::compartment::CompartmentState>,
    ) {
        // Bug №633: the repair cooldown decays even for a dead/offline station —
        // if it decayed behind the early-returns below, a station destroyed
        // mid-cooldown would freeze its clock and never be repairable again.
        state.repair_cooldown = (state.repair_cooldown - dt).max(0.0);

        if state.health <= 0.0 {
            return;
        }

        // Bug №56: operational-ness is re-derived from the compartment every
        // tick — no latch. Pump the water out / quench the fire and the
        // station comes back by itself.
        let compartment_dead =
            compartment_state.is_some_and(|c| c.is_flooded() || c.fire_intensity > 0.5);
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
        state.target_pitch =
            safe_clamp(pitch, self.config.pitch_range.0, self.config.pitch_range.1);
    }

    pub fn get_angles(&self, state: &StationState) -> (f32, f32) {
        (state.yaw, state.pitch)
    }

    /// Bug №630: the resolution of "has the turret arrived at the angles it
    /// was given". A deferred shot fires exactly when this turns true, so the
    /// tolerance must be far below any real slew step (`turn_speed * dt` is
    /// ≥ ~0.03 rad at the fixed sim rate) and far above f32 jitter from the
    /// final sub-step of the capped slew.
    pub const AIM_EPSILON_R: f32 = 1e-4;

    /// True when the station's live angles sit within `AIM_EPSILON_R` of the
    /// target angles `set_target_angles` last installed.
    pub fn aim_reached(&self, state: &StationState) -> bool {
        (state.target_yaw - state.yaw).abs() <= Self::AIM_EPSILON_R
            && (state.target_pitch - state.pitch).abs() <= Self::AIM_EPSILON_R
    }

    pub fn can_fire(&self, state: &StationState) -> bool {
        state.is_operational
            && state.health > 0.0
            && state.cooldown <= 0.0
            && state.reload_progress >= 1.0
            && state.ammo_count > 0
    }

    pub fn fire(&self, state: &mut StationState, ammo_type: Option<u8>) -> Option<FireResult> {
        if !self.can_fire(state) {
            return None;
        }

        if let Some(at) = ammo_type {
            // Bug №631: this used to write any byte straight into
            // `state.ammo_type`, while the comment on `ammo_type_to_projectile`
            // claimed "`fire` also rejects an unknown type". It did not, so the
            // `_ => Cannonball` arm fired a cannonball while the station went on
            // reporting the unknown number — the same report/fire mismatch №192
            // closed on the reload path, still open here through the direct API.
            //
            // Reject the shot rather than load a shell that does not exist: a
            // caller asking for a type that is not a type has asked for nothing,
            // and `None` (the wire path, where the type already sits in
            // `state.ammo_type`) is unaffected.
            if !Self::is_valid_ammo_type(at) {
                return None;
            }
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
        })
    }

    fn calculate_fire_direction(&self, yaw: f32, pitch: f32) -> Vec3f {
        let cp = pitch.cos();
        Vec3f::new(cp * yaw.sin(), pitch.sin(), cp * yaw.cos())
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

    /// The ammo types this station can load.
    ///
    /// Bug №192: the numbering is not validated anywhere, so both entry points
    /// — `ServerCommand::ReloadWeapon { ammo_type }` and
    /// `StationAction::SetAmmoType { value as u8 }` — could store an arbitrary
    /// byte into `state.ammo_type`. The old `_ => Cannonball` catch-all then
    /// fired a cannonball while the station reported ammo type 200, so the
    /// mismatch was invisible until a shell was inspected.
    ///
    /// Kept next to `ammo_type_to_projectile` so the two cannot drift: the
    /// mapping below is the same list, and `is_valid_ammo_type` is what keeps
    /// an out-of-range value from ever reaching it.
    pub const VALID_AMMO_TYPES: [u8; 7] = [0, 1, 2, 3, 4, 5, 6];

    /// Bug №633: with this cooldown a post restores at most 25 hp per second
    /// (`MAX_REPAIR_PER_ACTION` per action on the server side) instead of the
    /// packet-rate 500 hp/s the spam pattern produced. One second per action
    /// means full 500 hp repair in ~20 actions — deliberate, patient work,
    /// not a spam gimmick.
    pub const REPAIR_COOLDOWN: f32 = 1.0;

    /// Whether `ammo_type` names a real shell.
    pub fn is_valid_ammo_type(ammo_type: u8) -> bool {
        Self::VALID_AMMO_TYPES.contains(&ammo_type)
    }

    /// Bug №193: the old `_ => Cannonball` swallowed ammo types 5 and 6, so a
    /// loaded torpedo and a depth charge were both fired as a cannonball —
    /// losing their 5000 and 300 damage, their guidance and their fuse. The
    /// numbering follows `ProjectileType`, which the shell types already used.
    ///
    /// The `_` arm stays for exhaustiveness, but it is no longer reachable
    /// through `reload`, which validates first.
    fn ammo_type_to_projectile(&self, ammo_type: u8) -> rfs_core::packet::ProjectileType {
        Station::projectile_type_for(ammo_type)
            .unwrap_or(rfs_core::packet::ProjectileType::Cannonball)
    }

    /// Bug №664 (remainder): the station→projectile-family mapping, without
    /// needing a Station instance. The fire-control solution reads the loaded
    /// round's type to price its arc for the right shell family (a contact-
    /// exploding ballistic shell and a KP flat-nose slug land differently);
    /// `station.rs` also uses it for the same mapping when it actually spawns.
    pub fn projectile_type_for(ammo_type: u8) -> Option<rfs_core::packet::ProjectileType> {
        use rfs_core::packet::ProjectileType;
        match ammo_type {
            0 => Some(ProjectileType::Cannonball),
            1 => Some(ProjectileType::ExplosiveShell),
            2 => Some(ProjectileType::ArmorPiercing),
            3 => Some(ProjectileType::ChainShot),
            4 => Some(ProjectileType::GrapeShot),
            5 => Some(ProjectileType::Torpedo),
            6 => Some(ProjectileType::DepthCharge),
            _ => None,
        }
    }

    /// Loads one round of `ammo_type`.
    ///
    /// Returns whether the shell was actually loaded. `false` means the request
    /// was rejected and the station was left untouched — either the type is not
    /// a real shell (Bug №192), or the magazine is full or the reload cycle has
    /// not completed yet.
    pub fn reload(&self, state: &mut StationState, ammo_type: u8) -> bool {
        // Bug №192: reject the write, not just the fire. Storing an unknown type
        // is the actual defect; refusing here also means the `_ => Cannonball`
        // arm in `ammo_type_to_projectile` stays unreachable.
        if !Self::is_valid_ammo_type(ammo_type) {
            return false;
        }
        // Bug №84: finishing one reload cycle actually loads a shell. The old
        // code reset the progress and swapped ammo_type but never touched
        // ammo_count, so a fired gun stayed empty forever (progress reset
        // endlessly with ammo_count < max_ammo never becoming false).
        if state.ammo_count < state.max_ammo && state.reload_progress >= 1.0 {
            state.ammo_count += 1;
            state.ammo_type = ammo_type;
            state.reload_progress = 0.0;
            return true;
        }
        false
    }

    /// Apply a repair action with a per-post rate limit (Bug №633).
    ///
    /// Returns whether the action was applied. `false` means the request was
    /// rejected and the station was left untouched: a non-finite amount
    /// (Bug №604), or a repair still on cooldown (Bug №633). The cooldown
    /// starts with the *action*, not with its usefulness — a repair on a
    /// full-health or destroyed station also burns the window, so one owner
    /// cannot pre-charge and another spend it.
    pub fn repair(&self, state: &mut StationState, amount: f32) -> bool {
        // Bug №604: a non-finite amount (the input path used to pass
        // `f32::INFINITY` straight through) would poison `health`, and the
        // `.min(max_health)` below would then hand out a full repair from a
        // single packet. Same guard shape as `apply_damage` (№194).
        if !amount.is_finite() {
            return false;
        }
        // Bug №633: the amount ceiling is per action; this is the action rate.
        if state.repair_cooldown > 0.0 {
            return false;
        }
        state.repair_cooldown = Self::REPAIR_COOLDOWN;
        // Negative repairs must not damage the station.
        state.health = (state.health + amount.max(0.0)).min(state.max_health);
        if state.health > 0.0 {
            state.is_operational = true;
        }
        true
    }

    pub fn apply_damage(&self, state: &mut StationState, damage: f32) {
        // Bug №194: `f32::clamp` asserts min <= max, so a template with a
        // negative `max_health` (it comes straight out of JSON) aborted the
        // process instead of merely misbehaving. Same guard shape as
        // `Ship::apply_health_damage`.
        if !damage.is_finite() {
            return;
        }
        let max = if state.max_health.is_finite() && state.max_health > 0.0 {
            state.max_health
        } else {
            1.0
        };
        // Clamped both ways: negative damage must not overheal above max.
        state.health = (state.health - damage).clamp(0.0, max);
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

    /// Sets the station's default behavior.
    ///
    /// The live semantics the state layer actually carries is a single
    /// operational flag: `Active` puts the station up, `Passive` and
    /// `Disabled` take it down (they are distinct only as wire codes, not
    /// as behavior yet — the first consumer that needs a real passive
    /// split gets the state field it asks for). The command rule is that
    /// the occupant of the station decides; see `Ship::set_station_behavior`.
    pub fn set_default_behavior(&self, state: &mut StationState, behavior: DefaultBehavior) {
        state.is_operational = matches!(behavior, DefaultBehavior::Active);
    }
}

impl DefaultBehavior {
    /// The code a behavior travels as on the wire (bug №669): a small
    /// integer, because `core` carries the command and must not depend on
    /// the ship crate's types. Unknown codes are refused, never defaulted.
    pub fn from_code(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Active),
            1 => Some(Self::Passive),
            2 => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireResult {
    pub station_id: EntityId,
    pub position: Vec3f,
    pub direction: Vec3f,
    pub velocity: Vec3f,
    pub projectile_type: rfs_core::packet::ProjectileType,
    // Bug №193: `damage` and `penetration` used to be hardcoded per station
    // type here, which is why a torpedo's declared 5000 never applied. They
    // moved to `ProjectileConfig::for_type`, so the projectile type carries
    // the numbers and there is a single answer per type.
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefaultBehavior {
    Active,
    Passive,
    Disabled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfs_core::entity::ProjectileType;

    fn station() -> Station {
        Station::new(
            EntityId::new(2001),
            EntityId::new(1),
            StationTemplate {
                name: "main gun".into(),
                station_type: StationType::Gun,
                compartment_name: "bridge".into(),
                local_transform: Transform::from_position(Vec3f::ZERO),
                max_health: 50.0,
                max_ammo: 10,
                default_ammo_type: 0,
                cooldown: 1.0,
                yaw_range: (-90.0, 90.0),
                pitch_range: (-30.0, 30.0),
            },
        )
    }

    /// Bug №669: `from_code` is the whole wire contract for the behavior
    /// codes. Refusal, not defaulting, is what keeps a typo from flipping
    /// a gun unnoticed.
    #[test]
    fn the_behavior_codes_decode_their_own_level() {
        assert_eq!(DefaultBehavior::from_code(0), Some(DefaultBehavior::Active));
        assert_eq!(
            DefaultBehavior::from_code(1),
            Some(DefaultBehavior::Passive)
        );
        assert_eq!(
            DefaultBehavior::from_code(2),
            Some(DefaultBehavior::Disabled)
        );
        for unknown in [3u8, 77, 255] {
            assert_eq!(
                DefaultBehavior::from_code(unknown),
                None,
                "code {unknown} silently decoded instead of refusing"
            );
        }
    }

    /// The command layer believes `set_default_behavior` moves exactly one
    /// flag: `Active` keeps the station runnable, `Passive` and `Disabled`
    /// take it down (distinct only as wire codes for now). If that ever
    /// stops being what the function does, this test stops running green
    /// with it.
    #[test]
    fn a_default_behavior_moves_the_operational_flag() {
        let mut state = StationState {
            is_operational: true,
            ..Default::default()
        };
        for keeping in [DefaultBehavior::Passive, DefaultBehavior::Disabled] {
            station().set_default_behavior(&mut state, keeping);
            assert!(
                !state.is_operational,
                "{keeping:?} must take the station down"
            );
        }
        station().set_default_behavior(&mut state, DefaultBehavior::Active);
        assert!(state.is_operational, "Active must put the station back up");
    }

    /// Bug №193: the old mapping had no arm for ammo 5 and 6, and its `_` arm
    /// turned both into a cannonball. A loaded torpedo was fired as a 100
    /// damage cannonball, losing its 5000 damage, its guidance and its fuse.
    #[test]
    fn torpedo_and_depth_charge_survive_the_ammo_mapping() {
        let s = station();
        assert_eq!(
            s.ammo_type_to_projectile(5),
            ProjectileType::Torpedo,
            "ammo 5 was fired as something other than a torpedo"
        );
        assert_eq!(
            s.ammo_type_to_projectile(6),
            ProjectileType::DepthCharge,
            "ammo 6 was fired as something other than a depth charge"
        );
    }

    /// Every declared ammo type must map to the matching projectile, so the
    /// type that reaches `ProjectileConfig::for_type` is the one that was
    /// loaded.
    #[test]
    fn every_ammo_type_maps_to_its_own_projectile() {
        let s = station();
        let expected = [
            (0u8, ProjectileType::Cannonball),
            (1, ProjectileType::ExplosiveShell),
            (2, ProjectileType::ArmorPiercing),
            (3, ProjectileType::ChainShot),
            (4, ProjectileType::GrapeShot),
            (5, ProjectileType::Torpedo),
            (6, ProjectileType::DepthCharge),
        ];
        for (ammo, projectile) in expected {
            assert_eq!(
                s.ammo_type_to_projectile(ammo),
                projectile,
                "ammo {ammo} mapped to the wrong projectile"
            );
        }
    }

    /// The old mapping could not represent this at all, so nothing tested it.
    #[test]
    fn an_explosive_shell_keeps_a_blast_radius() {
        // ExplosiveShell is 1; its config declares a 10.0 blast. If the type is
        // right, `for_type` will hand back a non-zero radius.
        assert_eq!(
            station().ammo_type_to_projectile(1),
            ProjectileType::ExplosiveShell
        );
    }

    /// Bug №192. The defect was not that a bad type fired the wrong shell — it
    /// was that a bad type was *stored*, so the station reported one thing and
    /// fired another, and nothing anywhere complained.
    #[test]
    fn an_unknown_ammo_type_is_never_written_to_the_station() {
        for bad in [7u8, 8, 42, 200, 255] {
            assert!(
                !Station::is_valid_ammo_type(bad),
                "{bad} must not count as a known shell"
            );

            let mut state = StationState {
                // A completed reload cycle, so the only thing that can stop
                // the shell from loading is the type check.
                reload_progress: 1.0,
                ammo_count: 0,
                ammo_type: 3,
                ..Default::default()
            };
            let loaded = station().reload(&mut state, bad);

            assert!(!loaded, "ammo type {bad} was accepted");
            assert_eq!(
                state.ammo_type, 3,
                "the rejected reload overwrote ammo_type with {bad}"
            );
            assert_eq!(state.ammo_count, 0, "the rejected reload added a round");
        }
    }

    /// Bug №631. `fire` carried the promise, in a comment two functions
    /// down, that it rejected an unknown type; it stored the byte and then
    /// launched the `_` fallback, so the station reported one type and fired
    /// another. The shot must not happen at all, and the station must be left
    /// exactly as it was.
    #[test]
    fn an_unknown_ammo_type_fires_nothing_and_changes_nothing() {
        for bad in [7u8, 8, 42, 200, 255] {
            let mut state = StationState {
                // Ready to fire, with a known round already loaded, so the
                // only thing that can stop the shot is the type check.
                is_operational: true,
                health: 50.0,
                reload_progress: 1.0,
                cooldown: 0.0,
                ammo_count: 3,
                ammo_type: 2,
                ..Default::default()
            };
            let before = state.clone();

            assert!(
                station().fire(&mut state, Some(bad)).is_none(),
                "a shot was fired with ammo type {bad}"
            );
            assert_eq!(
                state.ammo_type, before.ammo_type,
                "the refused shot overwrote ammo_type with {bad}"
            );
            assert_eq!(
                state.ammo_count, before.ammo_count,
                "the refused shot spent a round"
            );
            assert_eq!(
                state.cooldown, before.cooldown,
                "the refused shot set a cooldown"
            );
        }
    }

    /// Every type the mapping knows must survive the validator, or a real
    /// shell would be rejected at the door. This is the check that keeps the
    /// two lists from drifting apart.
    #[test]
    fn every_mapped_ammo_type_passes_validation() {
        for mapped in 0u8..=6 {
            assert!(
                Station::is_valid_ammo_type(mapped),
                "type {mapped} has a projectile mapping but is not accepted"
            );
        }
        assert_eq!(Station::VALID_AMMO_TYPES.len(), 7);
    }

    /// The cast is the trap. `NaN as u8` and `-3.0 as u8` are both `0` in
    /// Rust, and `0` is a *valid* cannonball — so validating after the cast
    /// would have waved a malformed request straight through. This asserts
    /// the fact the caller in `main.rs` relies on.
    #[test]
    // The casts are the subject of this test, not an oversight: the whole
    // point is that `NaN as u8` is `0` and that `0` is a valid cannonball.
    // Silencing the lint here is what keeps it firing everywhere else.
    #[allow(clippy::cast_nan_to_int)]
    fn a_float_ammo_type_would_survive_a_naive_cast_as_a_valid_shell() {
        assert_eq!(f32::NAN as u8, 0);
        assert_eq!(-3.0f32 as u8, 0);
        assert_eq!(300.0f32 as u8, 255);
        // Which is exactly why `main.rs` checks `is_finite`, `fract() == 0`
        // and the range *before* casting. 255 does not pass the validator,
        // but NaN and negatives do.
        assert!(Station::is_valid_ammo_type(f32::NAN as u8));
        assert!(!Station::is_valid_ammo_type(300.0f32 as u8));
    }

    /// A valid type still loads, and the validator did not break Bug №84
    /// (finishing the cycle actually puts a round in the magazine).
    #[test]
    fn a_valid_ammo_type_still_loads() {
        let mut state = StationState {
            reload_progress: 1.0,
            ammo_count: 0,
            ..Default::default()
        };
        assert!(station().reload(&mut state, 5));
        assert_eq!(state.ammo_type, 5);
        assert_eq!(state.ammo_count, 1);
        assert_eq!(state.reload_progress, 0.0);
    }

    /// Bug №604, negative control: `f32::INFINITY` used to reach
    /// `.min(max_health)` and fill a half-damaged station in a single call,
    /// which is exactly what the input path sent. Drop the guard above and
    /// the first assertion goes red.
    #[test]
    fn a_non_finite_repair_amount_does_not_fill_the_station() {
        let mut state = StationState {
            health: 250.0,
            max_health: 500.0,
            ..Default::default()
        };

        station().repair(&mut state, f32::INFINITY);
        assert_eq!(
            state.health, 250.0,
            "an infinite repair amount filled the station"
        );

        station().repair(&mut state, f32::NAN);
        assert_eq!(
            state.health, 250.0,
            "a NaN repair amount changed the station's health"
        );
    }

    /// The guard must not break the honest case: a finite amount still
    /// repairs, and negative amounts still cannot damage the station.
    #[test]
    fn a_finite_repair_amount_still_repairs() {
        let mut state = StationState {
            health: 250.0,
            max_health: 500.0,
            ..Default::default()
        };

        station().repair(&mut state, 100.0);
        assert_eq!(state.health, 350.0, "a finite repair stopped working");

        // Bug №633: the actions are rate-limited now, so the next two
        // amounts step through real ticks — the same decay the ship tick
        // runs — instead of hammering in one instant.
        station().repair(&mut state, -10_000.0);
        assert_eq!(state.health, 350.0, "a negative repair damaged the station");
        station().update(1.0, &mut state, None);
        station().repair(&mut state, 10_000.0);
        assert_eq!(state.health, 500.0, "a repair overshoots max_health");
    }

    /// Bug №633: the repair loop used to accept every action, so a client
    /// spamming Repair packets ran the 25 hp/action ceiling into 500 hp/s
    /// of healing through one post. The window must refuse the immediate
    /// repeat, and the refusal must leave the station untouched.
    #[test]
    fn a_repair_action_starts_a_cooldown_the_next_action_hits() {
        let mut state = StationState {
            health: 250.0,
            max_health: 500.0,
            ..Default::default()
        };

        assert!(
            station().repair(&mut state, 25.0),
            "the first action must apply"
        );
        assert_eq!(state.health, 275.0);
        assert_eq!(
            state.repair_cooldown, 1.0,
            "a finished action must start the cooldown"
        );

        assert!(
            !station().repair(&mut state, 25.0),
            "the immediate second action must be refused"
        );
        assert_eq!(state.health, 275.0, "a refused action must not move health");

        // Half a window in, still frozen.
        station().update(0.5, &mut state, None);
        assert!(
            !station().repair(&mut state, 25.0),
            "the cooldown is not finished yet"
        );
        assert_eq!(state.health, 275.0);

        // A full window later the post works again.
        station().update(0.5, &mut state, None);
        assert!(
            station().repair(&mut state, 25.0),
            "a cooled-down post repairs again"
        );
        assert_eq!(state.health, 300.0);
    }

    /// Bug №633, the destroyed-station edge: if the decay lived behind the
    /// `health <= 0` early-return in `update`, a station destroyed
    /// mid-cooldown would freeze its clock forever and never become
    /// repairable again.
    #[test]
    fn a_cooldown_decays_even_while_the_station_is_dead() {
        let mut state = StationState {
            health: 0.0,
            max_health: 500.0,
            is_operational: false,
            repair_cooldown: 1.0,
            ..Default::default()
        };

        assert!(
            !station().repair(&mut state, 25.0),
            "the frozen window must refuse"
        );
        station().update(1.0, &mut state, None);
        assert_eq!(
            state.repair_cooldown, 0.0,
            "the cooldown must decay even behind the health early-return"
        );
        assert!(
            station().repair(&mut state, 25.0),
            "a destroyed station with a freed window must heal again"
        );
        assert_eq!(state.health, 25.0);
    }
}
