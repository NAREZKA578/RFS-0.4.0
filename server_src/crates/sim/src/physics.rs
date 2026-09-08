use rfs_core::math::{Vec3f, Transform, Quatf, Bounds};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipPhysicsConfig {
    pub mass: f32,
    pub inertia_tensor: [f32; 3],
    pub center_of_mass: Vec3f,
    pub center_of_buoyancy: Vec3f,
    pub waterline_height: f32,
    pub drag_coefficient: f32,
    pub angular_drag_coefficient: f32,
    pub rudder_force_coefficient: f32,
    pub max_rudder_angle: f32,
    pub propeller_force_coefficient: f32,
    pub max_propeller_rpm: f32,
    pub wave_response_amplitude: f32,
    pub wave_response_frequency: f32,
}

impl Default for ShipPhysicsConfig {
    fn default() -> Self {
        Self {
            mass: 500000.0,
            inertia_tensor: [1000000.0, 5000000.0, 5000000.0],
            center_of_mass: Vec3f::new(0.0, -2.0, 0.0),
            center_of_buoyancy: Vec3f::new(0.0, -5.0, 0.0),
            waterline_height: 5.0,
            drag_coefficient: 0.3,
            angular_drag_coefficient: 0.5,
            rudder_force_coefficient: 50000.0,
            max_rudder_angle: 35.0_f32.to_radians(),
            propeller_force_coefficient: 100000.0,
            max_propeller_rpm: 200.0,
            wave_response_amplitude: 0.5,
            wave_response_frequency: 0.5,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShipPhysicsState {
    pub position: Vec3f,
    pub rotation: Quatf,
    pub velocity: Vec3f,
    pub angular_velocity: Vec3f,
    pub thrust: f32,
    pub rudder_angle: f32,
    pub propeller_rpm: f32,
    pub water_level: f32,
    pub submerged_volume: f32,
}

pub struct ShipPhysics {
    config: ShipPhysicsConfig,
    state: ShipPhysicsState,
    forces: Vec3f,
    torques: Vec3f,
}

impl ShipPhysics {
    pub fn new(config: ShipPhysicsConfig) -> Self {
        Self {
            config,
            state: ShipPhysicsState::default(),
            forces: Vec3f::ZERO,
            torques: Vec3f::ZERO,
        }
    }

    pub fn set_state(&mut self, state: ShipPhysicsState) {
        self.state = state;
    }

    pub fn get_state(&self) -> ShipPhysicsState {
        self.state.clone()
    }

    pub fn update(&mut self, dt: f32, throttle: f32, rudder_input: f32) {
        self.forces = Vec3f::ZERO;
        self.torques = Vec3f::ZERO;
        
        self.state.thrust = throttle.clamp(-1.0, 1.0);
        self.state.rudder_angle = (rudder_input * self.config.max_rudder_angle).clamp(
            -self.config.max_rudder_angle,
            self.config.max_rudder_angle,
        );
        
        self.calculate_hydrostatics();
        self.calculate_propulsion(dt);
        self.calculate_rudder_forces(dt);
        self.calculate_drag(dt);
        self.calculate_wave_forces(dt);
        
        self.integrate(dt);
    }

    fn calculate_hydrostatics(&mut self) {
        let submerged_depth = (self.config.waterline_height - self.state.position.y).max(0.0);
        let hull_area = 1000.0;
        self.state.submerged_volume = submerged_depth * hull_area;
        
        let buoyancy_force = self.state.submerged_volume * 1025.0 * 9.81;
        self.forces.y += buoyancy_force;
        
        let com_world = self.state.rotation.mul_vec3(self.config.center_of_mass) + self.state.position;
        let cob_world = self.state.rotation.mul_vec3(self.config.center_of_buoyancy) + self.state.position;
        let righting_arm = cob_world - com_world;
        
        let righting_torque = righting_arm.cross(Vec3f::new(0.0, buoyancy_force, 0.0));
        self.torques += righting_torque;
    }

    fn calculate_propulsion(&mut self, dt: f32) {
        let target_rpm = self.state.thrust * self.config.max_propeller_rpm;
        let rpm_diff = target_rpm - self.state.propeller_rpm;
        self.state.propeller_rpm += rpm_diff * dt * 2.0;
        
        let propeller_force = self.state.propeller_rpm * self.config.propeller_force_coefficient;
        let forward = self.state.rotation.mul_vec3(Vec3f::FORWARD);
        self.forces += forward * propeller_force;
    }

    fn calculate_rudder_forces(&mut self, _dt: f32) {
        if self.state.velocity.length() < 1.0 {
            return;
        }
        
        let speed = self.state.velocity.length();
        let rudder_force = self.state.rudder_angle * speed * speed * self.config.rudder_force_coefficient;
        let right = self.state.rotation.mul_vec3(Vec3f::RIGHT);
        
        self.forces += right * rudder_force;
        
        let rudder_torque = self.config.center_of_mass.z * rudder_force;
        self.torques.y += rudder_torque;
    }

    fn calculate_drag(&mut self, _dt: f32) {
        let local_vel = self.state.rotation.inverse().mul_vec3(self.state.velocity);
        let speed = local_vel.length();
        
        if speed < 0.1 {
            return;
        }
        
        let drag_force = speed * speed * self.config.drag_coefficient;
        let drag_dir = -local_vel.normalize();
        let world_drag = self.state.rotation.mul_vec3(drag_dir * drag_force);
        self.forces += world_drag;
        
        let angular_speed = self.state.angular_velocity.length();
        if angular_speed > 0.01 {
            let angular_drag = angular_speed * angular_speed * self.config.angular_drag_coefficient;
            let angular_drag_dir = -self.state.angular_velocity.normalize();
            self.torques += angular_drag_dir * angular_drag;
        }
    }

    fn calculate_wave_forces(&mut self, _dt: f32) {
        let time = self.state.position.x * 0.01;
        let wave_height = (time * self.config.wave_response_frequency).sin() * self.config.wave_response_amplitude;
        
        let heave_force = (wave_height - self.state.position.y) * 10000.0 - self.state.velocity.y * 5000.0;
        self.forces.y += heave_force;
        
        let pitch_torque = -self.state.rotation.x * 50000.0 - self.state.angular_velocity.x * 10000.0;
        self.torques.x += pitch_torque;
        
        let roll_torque = -self.state.rotation.z * 50000.0 - self.state.angular_velocity.z * 10000.0;
        self.torques.z += roll_torque;
    }

    fn integrate(&mut self, dt: f32) {
        let acceleration = self.forces / self.config.mass;
        self.state.velocity += acceleration * dt;
        self.state.position += self.state.velocity * dt;
        
        let angular_acceleration = Vec3f::new(
            self.torques.x / self.config.inertia_tensor[0],
            self.torques.y / self.config.inertia_tensor[1],
            self.torques.z / self.config.inertia_tensor[2],
        );
        self.state.angular_velocity += angular_acceleration * dt;
        
        let half_omega = Quatf::new(
            self.state.angular_velocity.x * 0.5,
            self.state.angular_velocity.y * 0.5,
            self.state.angular_velocity.z * 0.5,
            0.0,
        );
        
        let q = self.state.rotation;
        let dq = Quatf::new(
            half_omega.x * q.w + half_omega.y * q.z - half_omega.z * q.y,
            half_omega.y * q.w + half_omega.z * q.x - half_omega.x * q.z,
            half_omega.z * q.w + half_omega.x * q.y - half_omega.y * q.x,
            -half_omega.x * q.x - half_omega.y * q.y - half_omega.z * q.z,
        );
        
        self.state.rotation = Quatf::new(
            q.x + dq.x * dt,
            q.y + dq.y * dt,
            q.z + dq.z * dt,
            q.w + dq.w * dt,
        ).normalize();
    }

    pub fn apply_impulse(&mut self, impulse: Vec3f, point: Vec3f) {
        self.state.velocity += impulse / self.config.mass;
        let r = point - (self.state.rotation.mul_vec3(self.config.center_of_mass) + self.state.position);
        let torque = r.cross(impulse);
        self.state.angular_velocity += Vec3f::new(
            torque.x / self.config.inertia_tensor[0],
            torque.y / self.config.inertia_tensor[1],
            torque.z / self.config.inertia_tensor[2],
        );
    }

    pub fn transform(&self) -> Transform {
        Transform::new(self.state.position, self.state.rotation, Vec3f::ONE)
    }

    pub fn velocity(&self) -> Vec3f {
        self.state.velocity
    }

    pub fn angular_velocity(&self) -> Vec3f {
        self.state.angular_velocity
    }

    pub fn bounds(&self) -> Bounds {
        let half_extents = Vec3f::new(50.0, 20.0, 150.0);
        Bounds::new(
            self.state.position - half_extents,
            self.state.position + half_extents,
        )
    }

    pub fn get_submerged_volume(&self) -> f32 {
        self.state.submerged_volume
    }

    pub fn get_draft(&self) -> f32 {
        (self.config.waterline_height - self.state.position.y).max(0.0)
    }

    pub fn get_heel_angle(&self) -> f32 {
        self.state.rotation.z
    }

    pub fn get_trim_angle(&self) -> f32 {
        self.state.rotation.x
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wave {
    pub amplitude: f32,
    pub wavelength: f32,
    pub direction: Vec3f,
    pub phase: f32,
    pub speed: f32,
}

pub struct WaveSystem {
    waves: Vec<Wave>,
    time: f32,
}

impl WaveSystem {
    pub fn new() -> Self {
        Self {
            waves: Vec::new(),
            time: 0.0,
        }
    }

    pub fn add_wave(&mut self, wave: Wave) {
        self.waves.push(wave);
    }

    pub fn update(&mut self, dt: f32) {
        self.time += dt;
    }

    pub fn get_height(&self, position: Vec3f) -> f32 {
        let mut height = 0.0;
        for wave in &self.waves {
            let dot = position.x * wave.direction.x + position.z * wave.direction.z;
            let k = 2.0 * std::f32::consts::PI / wave.wavelength;
            let omega = k * wave.speed;
            height += wave.amplitude * (k * dot - omega * self.time + wave.phase).sin();
        }
        height
    }

    pub fn get_normal(&self, position: Vec3f, epsilon: f32) -> Vec3f {
        let h = self.get_height(position);
        let hx = self.get_height(position + Vec3f::new(epsilon, 0.0, 0.0));
        let hz = self.get_height(position + Vec3f::new(0.0, 0.0, epsilon));
        
        Vec3f::new(h - hx, epsilon, h - hz).normalize()
    }

    pub fn get_velocity(&self, position: Vec3f) -> Vec3f {
        let mut vel = Vec3f::ZERO;
        for wave in &self.waves {
            let dot = position.x * wave.direction.x + position.z * wave.direction.z;
            let k = 2.0 * std::f32::consts::PI / wave.wavelength;
            let omega = k * wave.speed;
            let phase = k * dot - omega * self.time + wave.phase;
            let amplitude = wave.amplitude * omega;
            vel.x += wave.direction.x * amplitude * phase.cos();
            vel.z += wave.direction.z * amplitude * phase.cos();
        }
        vel
    }
}