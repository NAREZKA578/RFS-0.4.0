use rfs_core::math::Vec3f;
use rfs_core::packet::ProjectileType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const GRAVITY: f32 = 9.81;
pub const WATER_DENSITY: f32 = 1025.0;
pub const AIR_DENSITY: f32 = 1.225;
pub const DRAG_COEFFICIENT_SPHERE: f32 = 0.47;
pub const DRAG_COEFFICIENT_STREAM: f32 = 0.04;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BallisticsConfig {
    pub gravity: f32,
    pub air_density: f32,
    pub water_density: f32,
    pub enable_water_physics: bool,
    pub enable_ricochet: bool,
    pub ricochet_angle_threshold: f32,
    pub max_ricochets: u32,
    pub penetration_rng_factor: f32,
}

impl Default for BallisticsConfig {
    fn default() -> Self {
        Self {
            gravity: GRAVITY,
            air_density: AIR_DENSITY,
            water_density: WATER_DENSITY,
            enable_water_physics: true,
            enable_ricochet: true,
            ricochet_angle_threshold: 0.707,
            max_ricochets: 2,
            penetration_rng_factor: 0.1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectileConfig {
    pub projectile_type: ProjectileType,
    pub mass: f32,
    pub diameter: f32,
    pub drag_coefficient: f32,
    pub initial_velocity: f32,
    pub max_range: f32,
    pub damage: f32,
    pub penetration: f32,
    pub explosion_radius: f32,
    pub fuse_delay: f32,
    pub is_guided: bool,
    pub turn_rate: f32,
}

impl ProjectileConfig {
    pub fn cannonball() -> Self {
        Self {
            projectile_type: ProjectileType::Cannonball,
            mass: 18.0,
            diameter: 0.15,
            drag_coefficient: DRAG_COEFFICIENT_SPHERE,
            initial_velocity: 400.0,
            max_range: 5000.0,
            damage: 100.0,
            penetration: 50.0,
            explosion_radius: 0.0,
            fuse_delay: 0.0,
            is_guided: false,
            turn_rate: 0.0,
        }
    }

    pub fn explosive_shell() -> Self {
        Self {
            projectile_type: ProjectileType::ExplosiveShell,
            mass: 20.0,
            diameter: 0.15,
            drag_coefficient: DRAG_COEFFICIENT_SPHERE,
            initial_velocity: 350.0,
            max_range: 6000.0,
            damage: 150.0,
            penetration: 30.0,
            explosion_radius: 10.0,
            fuse_delay: 0.5,
            is_guided: false,
            turn_rate: 0.0,
        }
    }

    pub fn armor_piercing() -> Self {
        Self {
            projectile_type: ProjectileType::ArmorPiercing,
            mass: 22.0,
            diameter: 0.12,
            drag_coefficient: DRAG_COEFFICIENT_STREAM,
            initial_velocity: 800.0,
            max_range: 8000.0,
            damage: 200.0,
            penetration: 200.0,
            explosion_radius: 0.0,
            fuse_delay: 0.0,
            is_guided: false,
            turn_rate: 0.0,
        }
    }

    pub fn chain_shot() -> Self {
        Self {
            projectile_type: ProjectileType::ChainShot,
            mass: 15.0,
            diameter: 0.2,
            drag_coefficient: 1.2,
            initial_velocity: 200.0,
            max_range: 1000.0,
            damage: 50.0,
            penetration: 10.0,
            explosion_radius: 0.0,
            fuse_delay: 0.0,
            is_guided: false,
            turn_rate: 0.0,
        }
    }

    pub fn torpedo() -> Self {
        Self {
            projectile_type: ProjectileType::Torpedo,
            mass: 800.0,
            diameter: 0.53,
            drag_coefficient: DRAG_COEFFICIENT_STREAM,
            initial_velocity: 50.0,
            max_range: 10000.0,
            damage: 5000.0,
            penetration: 100.0,
            explosion_radius: 20.0,
            fuse_delay: 2.0,
            is_guided: true,
            turn_rate: 10.0,
        }
    }
}

pub struct BallisticsCalculator {
    config: BallisticsConfig,
    projectile_configs: HashMap<ProjectileType, ProjectileConfig>,
}

impl BallisticsCalculator {
    pub fn new(config: BallisticsConfig) -> Self {
        let mut configs = HashMap::new();
        configs.insert(ProjectileType::Cannonball, ProjectileConfig::cannonball());
        configs.insert(ProjectileType::ExplosiveShell, ProjectileConfig::explosive_shell());
        configs.insert(ProjectileType::ArmorPiercing, ProjectileConfig::armor_piercing());
        configs.insert(ProjectileType::ChainShot, ProjectileConfig::chain_shot());
        configs.insert(ProjectileType::Torpedo, ProjectileConfig::torpedo());
        
        Self {
            config,
            projectile_configs: configs,
        }
    }

    pub fn get_config(&self, projectile_type: ProjectileType) -> Option<&ProjectileConfig> {
        self.projectile_configs.get(&projectile_type)
    }

    pub fn calculate_trajectory(
        &self,
        position: Vec3f,
        direction: Vec3f,
        projectile_type: ProjectileType,
        max_time: f32,
        time_step: f32,
    ) -> Vec<TrajectoryPoint> {
        let Some(config) = self.projectile_configs.get(&projectile_type) else {
            return Vec::new();
        };
        let mut points = Vec::new();
        
        let mut pos = position;
        let mut vel = direction.normalize() * config.initial_velocity;
        let mut time = 0.0;
        
        while time < max_time && pos.y > -10.0 {
            points.push(TrajectoryPoint {
                time,
                position: pos,
                velocity: vel,
                speed: vel.length(),
            });
            
            let (new_pos, new_vel) = self.integrate_step(pos, vel, config, time_step);
            pos = new_pos;
            vel = new_vel;
            time += time_step;
        }
        
        points
    }

    /// Air integration for one step: gravity + quadratic drag (plan §4).
    /// Exported so the tick loop flies shells through this instead of a
    /// hardcoded Euler step.
    pub fn integrate_step(
        &self,
        position: Vec3f,
        velocity: Vec3f,
        config: &ProjectileConfig,
        dt: f32,
    ) -> (Vec3f, Vec3f) {
        let speed = velocity.length();
        if speed < 0.1 {
            return (position, velocity);
        }

        let drag_force = self.calculate_drag(speed, config);
        let drag_accel = drag_force / config.mass;
        let drag_dir = -velocity.normalize();
        
        let gravity = Vec3f::new(0.0, -self.config.gravity, 0.0);
        let total_accel = gravity + drag_dir * drag_accel;
        
        let new_vel = velocity + total_accel * dt;
        let new_pos = position + velocity * dt + total_accel * dt * dt * 0.5;
        
        (new_pos, new_vel)
    }

    fn calculate_drag(&self, speed: f32, config: &ProjectileConfig) -> f32 {
        let area = (config.diameter * 0.5).powi(2) * std::f32::consts::PI;
        let density = if config.projectile_type == ProjectileType::Torpedo {
            self.config.water_density
        } else {
            self.config.air_density
        };
        
        0.5 * density * speed * speed * config.drag_coefficient * area
    }

    pub fn calculate_penetration(
        &self,
        projectile: &ProjectileConfig,
        impact_velocity: Vec3f,
        impact_normal: Vec3f,
        armor_thickness: f32,
        armor_angle: f32,
    ) -> PenetrationResult {
        let impact_speed = impact_velocity.length();
        // normalize() is zero-safe (returns ZERO), but fp rounding can push
        // the dot a hair past 1.0 — clamp before acos or impact_angle is NaN.
        let cos_angle = impact_velocity.normalize().dot(impact_normal).abs().clamp(0.0, 1.0);
        let impact_angle = cos_angle.acos();
        let effective_thickness = armor_thickness / armor_angle.cos().max(0.01);
        
        let mut penetration = projectile.penetration * (impact_speed / projectile.initial_velocity).sqrt();
        
        penetration *= 1.0 + (self.config.penetration_rng_factor * 2.0 - 1.0) * 0.5;
        
        let ricochet = self.config.enable_ricochet 
            && impact_angle > self.config.ricochet_angle_threshold 
            && projectile.projectile_type != ProjectileType::ExplosiveShell;
        
        let penetrated = penetration > effective_thickness && !ricochet;
        let remaining_penetration = (penetration - effective_thickness).max(0.0);
        
        let post_pen_velocity;
        if penetrated {
            let speed_loss = effective_thickness / penetration.max(0.01);
            post_pen_velocity = impact_velocity * (1.0 - speed_loss * 0.5);
        } else if ricochet {
            let reflect = impact_velocity - impact_normal * 2.0 * impact_velocity.dot(impact_normal);
            post_pen_velocity = reflect * 0.3;
        } else {
            post_pen_velocity = Vec3f::ZERO;
        }
        
        PenetrationResult {
            penetrated,
            ricochet,
            penetration_depth: penetration.min(effective_thickness),
            remaining_penetration,
            post_penetration_velocity: post_pen_velocity,
            effective_armor_thickness: effective_thickness,
            impact_angle,
        }
    }

    pub fn calculate_damage(
        &self,
        projectile: &ProjectileConfig,
        penetration_result: &PenetrationResult,
        distance: f32,
    ) -> f32 {
        let mut damage = projectile.damage;
        
        let range_factor = 1.0 - (distance / projectile.max_range).min(1.0) * 0.3;
        damage *= range_factor;
        
        if penetration_result.ricochet {
            damage *= 0.25;
        } else if penetration_result.penetrated {
            damage *= 1.0 + penetration_result.remaining_penetration * 0.01;
        } else {
            damage *= 0.5;
        }
        
        damage.max(1.0)
    }

    pub fn simulate_ricochet(
        &self,
        velocity: Vec3f,
        normal: Vec3f,
        elasticity: f32,
    ) -> Vec3f {
        let dot = velocity.dot(normal);
        if dot >= 0.0 {
            return velocity;
        }
        
        let reflected = velocity - normal * 2.0 * dot;
        reflected * elasticity
    }

    pub fn check_water_entry(
        &self,
        position: Vec3f,
        velocity: Vec3f,
        water_level: f32,
    ) -> Option<WaterEntryResult> {
        if !self.config.enable_water_physics {
            return None;
        }
        
        if position.y > water_level && velocity.y < 0.0 {
            let time_to_water = (position.y - water_level) / -velocity.y;
            if time_to_water > 0.0 && time_to_water < 1.0 {
                let entry_pos = position + velocity * time_to_water;
                let entry_vel = velocity;
                
                return Some(WaterEntryResult {
                    entry_position: entry_pos,
                    entry_velocity: entry_vel,
                    time_to_entry: time_to_water,
                });
            }
        }
        
        None
    }

    pub fn simulate_underwater(
        &self,
        position: Vec3f,
        velocity: Vec3f,
        config: &ProjectileConfig,
        dt: f32,
    ) -> (Vec3f, Vec3f) {
        let speed = velocity.length();
        if speed < 0.1 {
            return (position, Vec3f::ZERO);
        }
        
        let drag_force = self.calculate_drag_underwater(speed, config);
        let drag_accel = drag_force / config.mass;
        let drag_dir = -velocity.normalize();
        
        let buoyancy = Vec3f::new(0.0, self.config.water_density * 9.81 * 0.001, 0.0);
        let total_accel = drag_dir * drag_accel + buoyancy / config.mass;
        
        let new_vel = velocity + total_accel * dt;
        let new_pos = position + velocity * dt + total_accel * dt * dt * 0.5;
        
        (new_pos, new_vel)
    }

    fn calculate_drag_underwater(&self, speed: f32, config: &ProjectileConfig) -> f32 {
        let area = (config.diameter * 0.5).powi(2) * std::f32::consts::PI;
        0.5 * self.config.water_density * speed * speed * config.drag_coefficient * area * 10.0
    }

    pub fn solve_ballistic_arc(
        &self,
        from: Vec3f,
        to: Vec3f,
        projectile_type: ProjectileType,
    ) -> Option<BallisticSolution> {
        let Some(config) = self.projectile_configs.get(&projectile_type) else {
            return None;
        };
        let v0 = config.initial_velocity;
        let g = self.config.gravity;

        // Degenerate inputs: no muzzle velocity, no gravity, or a purely
        // vertical shot — the closed form divides by these.
        if v0 <= 0.0 || g <= 0.0 {
            return None;
        }
        
        let dx = to.x - from.x;
        let dz = to.z - from.z;
        let horizontal_dist = (dx * dx + dz * dz).sqrt();
        if horizontal_dist < 1e-3 {
            return None;
        }
        let dy = to.y - from.y;
        
        let v0_sq = v0 * v0;
        let v0_quad = v0_sq * v0_sq;
        let discriminant = v0_quad - g * (g * horizontal_dist * horizontal_dist + 2.0 * dy * v0_sq);
        
        if discriminant < 0.0 {
            return None;
        }
        
        let sqrt_disc = discriminant.sqrt();
        let angle1 = (v0_sq + sqrt_disc) / (g * horizontal_dist);
        let angle2 = (v0_sq - sqrt_disc) / (g * horizontal_dist);
        
        let angle = if angle1 > 0.0 && angle1 < angle2 { angle1 } else { angle2 };
        
        if angle <= 0.0 {
            return None;
        }
        
        let pitch = angle.atan();
        let yaw = dz.atan2(dx);
        
        let time_of_flight = horizontal_dist / (v0 * pitch.cos());
        let max_height = from.y + (v0 * pitch.sin()).powi(2) / (2.0 * g);
        
        Some(BallisticSolution {
            pitch,
            yaw,
            time_of_flight,
            max_height,
            initial_velocity: v0,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryPoint {
    pub time: f32,
    pub position: Vec3f,
    pub velocity: Vec3f,
    pub speed: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PenetrationResult {
    pub penetrated: bool,
    pub ricochet: bool,
    pub penetration_depth: f32,
    pub remaining_penetration: f32,
    pub post_penetration_velocity: Vec3f,
    pub effective_armor_thickness: f32,
    pub impact_angle: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaterEntryResult {
    pub entry_position: Vec3f,
    pub entry_velocity: Vec3f,
    pub time_to_entry: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BallisticSolution {
    pub pitch: f32,
    pub yaw: f32,
    pub time_of_flight: f32,
    pub max_height: f32,
    pub initial_velocity: f32,
}

impl BallisticSolution {
    pub fn direction(&self) -> Vec3f {
        let cp = self.pitch.cos();
        Vec3f::new(
            cp * self.yaw.cos(),
            self.pitch.sin(),
            cp * self.yaw.sin(),
        )
    }
}