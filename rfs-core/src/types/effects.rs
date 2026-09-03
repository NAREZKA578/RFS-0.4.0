use glam::Vec3;

/// Type of sound effect to play.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SoundType {
    /// Crackling fire.
    Fire,
    /// Water spray from hose.
    Water,
    /// Structural collapse or creaking.
    Collapse,
    /// Breaking glass.
    Glass,
    /// Wood cracking/breaking.
    WoodCrack,
    /// Siren from fire truck.
    Siren,
    /// Radio communication chatter.
    Radio,
    /// Breathing through SCBA regulator.
    ScbaBreathing,
    /// SCBA low-air alarm beep.
    ScbaAlarm,
    /// Civilian screaming or calling for help.
    Civilian,
    /// Gas leak hiss.
    GasLeak,
    /// Footstep on different surfaces.
    Footstep,
    /// Tool usage (axe chop, halligan pry).
    ToolUse,
    /// Ambient environmental sounds.
    Ambient,
}

/// A sound effect event to be played at a world position.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SoundEvent {
    pub sound_type: SoundType,
    pub position: Vec3,
    pub volume: f32,
}

impl SoundEvent {
    pub fn new(sound_type: SoundType, position: Vec3, volume: f32) -> Self {
        Self {
            sound_type,
            position,
            volume,
        }
    }
}

/// Type of particle effect to spawn.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ParticleType {
    /// Fire flames and embers.
    Fire,
    /// Smoke (various densities/colors).
    Smoke,
    /// Water spray/droplets.
    Water,
    /// Steam from water hitting hot surfaces.
    Steam,
    /// Sparks from electrical or metal work.
    Sparks,
    /// Debris from structural damage.
    Debris,
    /// Dust from collapsed materials.
    Dust,
    /// Glass shards from broken windows.
    GlassShards,
    /// Heat shimmer/haze.
    HeatHaze,
}

/// A particle effect event to be spawned at a world position.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParticleEvent {
    pub particle_type: ParticleType,
    pub position: Vec3,
    pub velocity: Vec3,
    pub lifetime: f32,
    pub scale: f32,
    pub count: u32,
}

impl ParticleEvent {
    pub fn new(particle_type: ParticleType, position: Vec3) -> Self {
        Self {
            particle_type,
            position,
            velocity: Vec3::ZERO,
            lifetime: 1.0,
            scale: 1.0,
            count: 1,
        }
    }

    pub fn with_velocity(mut self, velocity: Vec3) -> Self {
        self.velocity = velocity;
        self
    }

    pub fn with_lifetime(mut self, lifetime: f32) -> Self {
        self.lifetime = lifetime;
        self
    }

    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    pub fn with_count(mut self, count: u32) -> Self {
        self.count = count;
        self
    }
}
