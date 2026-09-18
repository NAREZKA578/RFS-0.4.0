//! Water Interaction
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Handles interaction between water and other objects (ships, projectiles, etc.)

use glam::{Vec2, Vec3};
use std::collections::HashMap;

/// Water interaction
pub struct WaterInteraction {
    /// Objects interacting with water
    objects: HashMap<usize, WaterInteractionObject>,
    /// Ripples (from object interactions)
    ripples: Vec<Ripple>,
    /// Splashes (from object interactions)
    splashes: Vec<Splash>,
}

impl WaterInteraction {
    pub fn new() -> Self {
        Self {
            objects: HashMap::new(),
            ripples: Vec::new(),
            splashes: Vec::new(),
        }
    }

    /// Add an object interacting with water
    pub fn add_object(&mut self, id: usize, position: Vec3, size: Vec3) {
        self.objects.insert(
            id,
            WaterInteractionObject {
                position,
                size,
                velocity: Vec3::ZERO,
                submerged: false,
                submerge_depth: 0.0,
            },
        );
    }

    /// Remove an object
    pub fn remove_object(&mut self, id: usize) {
        self.objects.remove(&id);
    }

    /// Update object position
    pub fn update_object(&mut self, id: usize, position: Vec3, velocity: Vec3) {
        if let Some(obj) = self.objects.get_mut(&id) {
            obj.position = position;
            obj.velocity = velocity;
        }
    }

    /// Update all interactions
    pub fn update(&mut self, water_height: f32, delta_time: std::time::Duration) {
        let delta_seconds = delta_time.as_secs_f32();

        // Update ripples
        for ripple in &mut self.ripples {
            ripple.update(delta_seconds);
        }

        // Update splashes
        for splash in &mut self.splashes {
            splash.update(delta_seconds);
        }

        // Remove dead ripples and splashes
        self.ripples.retain(|r| r.is_alive());
        self.splashes.retain(|s| s.is_alive());

        // Update object submergence
        for obj in self.objects.values_mut() {
            obj.submerged = obj.position.y < water_height;
            obj.submerge_depth = (water_height - obj.position.y).max(0.0);
        }
    }

    /// Add a ripple at a position
    pub fn add_ripple(&mut self, position: Vec3, radius: f32, strength: f32) {
        self.ripples.push(Ripple::new(position, radius, strength));
    }

    /// Add a splash at a position
    pub fn add_splash(&mut self, position: Vec3, size: f32, strength: f32) {
        self.splashes.push(Splash::new(position, size, strength));
    }

    /// Get displacement at a position (from ripples and objects)
    pub fn get_displacement(&self, position: Vec3) -> Vec3 {
        let mut displacement = Vec3::ZERO;

        // Add displacement from ripples
        for ripple in &self.ripples {
            displacement += ripple.get_displacement(position);
        }

        // Add displacement from objects
        for obj in self.objects.values() {
            if obj.submerged {
                displacement += self.get_object_displacement(position, obj);
            }
        }

        displacement
    }

    fn get_object_displacement(&self, position: Vec3, obj: &WaterInteractionObject) -> Vec3 {
        let obj_pos = Vec2::new(obj.position.x, obj.position.z);
        let dist = position.distance(obj_pos.extend(0.0));
        if dist > obj.size.x + obj.size.z {
            return Vec3::ZERO;
        }

        let ratio = 1.0 - dist / (obj.size.x + obj.size.z);
        Vec3::new(0.0, obj.submerge_depth * ratio.powi(2), 0.0)
    }

    /// Get foam amount at a position
    pub fn get_foam(&self, position: Vec3, water_height: f32, foam_threshold: f32) -> f32 {
        let mut foam = 0.0;

        // Add foam from splashes
        for splash in &self.splashes {
            foam += splash.get_foam(position);
        }

        // Add foam from objects
        for obj in self.objects.values() {
            if obj.submerged && obj.velocity.y > foam_threshold {
                let obj_pos = Vec2::new(obj.position.x, obj.position.z);
                let dist = position.distance(obj_pos.extend(water_height));
                foam += (1.0 - dist / (obj.size.x + obj.size.z)).max(0.0);
            }
        }

        foam.min(1.0)
    }
}

impl Default for WaterInteraction {
    fn default() -> Self {
        Self::new()
    }
}

/// Water interaction object
#[derive(Debug, Clone)]
pub struct WaterInteractionObject {
    pub position: Vec3,
    pub size: Vec3,
    pub velocity: Vec3,
    pub submerged: bool,
    pub submerge_depth: f32,
}

/// Ripple
#[derive(Debug, Clone)]
pub struct Ripple {
    position: Vec3,
    radius: f32,
    max_radius: f32,
    strength: f32,
    time: f32,
    max_time: f32,
}

impl Ripple {
    pub fn new(position: Vec3, max_radius: f32, strength: f32) -> Self {
        Self {
            position,
            radius: 0.0,
            max_radius,
            strength,
            time: 0.0,
            max_time: 2.0,
        }
    }

    pub fn update(&mut self, delta_time: f32) {
        self.time += delta_time;
        self.radius = self.max_radius * (self.time / self.max_time).min(1.0);
    }

    pub fn is_alive(&self) -> bool {
        self.time < self.max_time
    }

    pub fn get_displacement(&self, position: Vec3) -> Vec3 {
        let dist = position.distance(self.position);
        if dist > self.radius {
            return Vec3::ZERO;
        }

        let ratio = 1.0 - dist / self.radius;
        Vec3::new(0.0, self.strength * ratio.powi(2), 0.0)
    }
}

/// Splash
#[derive(Debug, Clone)]
pub struct Splash {
    position: Vec3,
    size: f32,
    max_size: f32,
    strength: f32,
    time: f32,
    max_time: f32,
}

impl Splash {
    pub fn new(position: Vec3, max_size: f32, strength: f32) -> Self {
        Self {
            position,
            size: 0.0,
            max_size,
            strength,
            time: 0.0,
            max_time: 1.0,
        }
    }

    pub fn update(&mut self, delta_time: f32) {
        self.time += delta_time;
        self.size = self.max_size * (self.time / self.max_time).min(1.0);
    }

    pub fn is_alive(&self) -> bool {
        self.time < self.max_time
    }

    pub fn get_foam(&self, position: Vec3) -> f32 {
        let dist = position.distance(self.position);
        if dist > self.size {
            return 0.0;
        }

        let ratio = 1.0 - dist / self.size;
        self.strength * ratio
    }
}
