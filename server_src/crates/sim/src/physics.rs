use rfs_core::math::{Bounds, Quatf, Transform, Vec3f};
use serde::{Deserialize, Serialize};

/// Bug №725: the waterline area the hydrostatics integrate buoyancy over.
///
/// This used to be a literal `1000.0` inside `calculate_hydrostatics`, while
/// the mass, the waterline height and the rest all lived in
/// `ShipPhysicsConfig` — so a second ship class (the destroyer exists and is
/// registered) configured everything but got the frigate's waterline. The
/// number is config now; `1000.0` stays as the default so every class file
/// already on disk keeps loading unchanged.
///
/// `DEFAULT_HULL_AREA` (not a `#[serde(default)]` on the field alone) because
/// the derive's default for `f32` is `0.0`, which is the one value a hull area
/// cannot be — that would read as "no buoyancy at all".
const DEFAULT_HULL_AREA: f32 = 1000.0;

/// The `#[serde(default = ...)]` engine the field below names.
fn default_hull_area() -> f32 {
    DEFAULT_HULL_AREA
}

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
    /// Bug №725: the ship's waterline area, in m². A class declares its own;
    /// an older class file without the field loads the historical `1000.0`.
    #[serde(default = "default_hull_area")]
    pub hull_area: f32,
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
            hull_area: DEFAULT_HULL_AREA,
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
        self.state.rudder_angle = (rudder_input * self.config.max_rudder_angle)
            .clamp(-self.config.max_rudder_angle, self.config.max_rudder_angle);

        self.calculate_hydrostatics();
        self.calculate_propulsion(dt);
        self.calculate_rudder_forces(dt);
        self.calculate_drag(dt);
        self.calculate_wave_forces(dt);

        self.integrate(dt);
    }

    fn calculate_hydrostatics(&mut self) {
        let submerged_depth = (self.config.waterline_height - self.state.position.y).max(0.0);
        // Bug №725: the area is the class's declared one, no longer a shared
        // literal. `max(0.0)` also reads a negative or NaN declaration as "no
        // waterline area" — a broken config must collapse to an unmoving hull,
        // not to a sign-flipped or exploding buoyancy term (the NaN rules of
        // №197/№198 apply here the same way).
        let hull_area = self.config.hull_area.max(0.0);
        self.state.submerged_volume = submerged_depth * hull_area;

        let buoyancy_force = self.state.submerged_volume * 1025.0 * 9.81;
        self.forces.y += buoyancy_force;

        let com_world =
            self.state.rotation.mul_vec3(self.config.center_of_mass) + self.state.position;
        let cob_world =
            self.state.rotation.mul_vec3(self.config.center_of_buoyancy) + self.state.position;
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

        // The rudder acts at the stern, far behind the COG. The old code used
        // center_of_mass.z as the lever — the COG is ~z=0, so the yaw torque
        // was ~0 and the ship could not be steered by the rudder forces.
        // Use a fixed stern lever arm (matches bounds half-length z=150).
        const RUDDER_LEVER_ARM: f32 = -150.0;

        let speed = self.state.velocity.length();
        let rudder_force =
            self.state.rudder_angle * speed * speed * self.config.rudder_force_coefficient;
        let right = self.state.rotation.mul_vec3(Vec3f::RIGHT);

        self.forces += right * rudder_force;

        let rudder_torque = RUDDER_LEVER_ARM * rudder_force;
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
        let wave_height = (time * self.config.wave_response_frequency).sin()
            * self.config.wave_response_amplitude;

        let heave_force =
            (wave_height - self.state.position.y) * 10000.0 - self.state.velocity.y * 5000.0;
        self.forces.y += heave_force;

        // Bug №195: see `roll_and_pitch` and `restoring_torque`. Angular
        // damping stays on its own axes — `angular_velocity` components are real.
        let restoring = restoring_torque(
            self.state.rotation,
            self.state.angular_velocity,
            50000.0,
            10000.0,
        );
        self.torques.x += restoring.x;
        self.torques.y += restoring.y;
        self.torques.z += restoring.z;
    }

    fn integrate(&mut self, dt: f32) {
        // Guards: mass/inertia from config must be positive finite, otherwise
        // /mass -> inf -> NaN spreads over state and network.
        let mass = if self.config.mass.is_finite() && self.config.mass > 1e-6 {
            self.config.mass
        } else {
            1.0
        };
        let inertia = [
            if self.config.inertia_tensor[0].is_finite() && self.config.inertia_tensor[0] > 1e-6 {
                self.config.inertia_tensor[0]
            } else {
                1.0
            },
            if self.config.inertia_tensor[1].is_finite() && self.config.inertia_tensor[1] > 1e-6 {
                self.config.inertia_tensor[1]
            } else {
                1.0
            },
            if self.config.inertia_tensor[2].is_finite() && self.config.inertia_tensor[2] > 1e-6 {
                self.config.inertia_tensor[2]
            } else {
                1.0
            },
        ];
        let acceleration = self.forces / mass;
        self.state.velocity += acceleration * dt;
        self.state.position += self.state.velocity * dt;

        let angular_acceleration = Vec3f::new(
            self.torques.x / inertia[0],
            self.torques.y / inertia[1],
            self.torques.z / inertia[2],
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
        )
        .normalize();
    }

    pub fn apply_impulse(&mut self, impulse: Vec3f, point: Vec3f) {
        // Bug №159: non-finite impulses or degenerate mass/inertia from bad
        // configs must not divide into inf/NaN that then spreads over the
        // network state.
        if !impulse.is_finite() {
            return;
        }
        let mass = if self.config.mass.is_finite() && self.config.mass > 1e-6 {
            self.config.mass
        } else {
            1.0
        };
        let inertia = [
            if self.config.inertia_tensor[0].is_finite() && self.config.inertia_tensor[0] > 1e-6 {
                self.config.inertia_tensor[0]
            } else {
                1.0
            },
            if self.config.inertia_tensor[1].is_finite() && self.config.inertia_tensor[1] > 1e-6 {
                self.config.inertia_tensor[1]
            } else {
                1.0
            },
            if self.config.inertia_tensor[2].is_finite() && self.config.inertia_tensor[2] > 1e-6 {
                self.config.inertia_tensor[2]
            } else {
                1.0
            },
        ];
        self.state.velocity += impulse / mass;
        let r = point
            - (self.state.rotation.mul_vec3(self.config.center_of_mass) + self.state.position);
        let torque = r.cross(impulse);
        self.state.angular_velocity += Vec3f::new(
            torque.x / inertia[0],
            torque.y / inertia[1],
            torque.z / inertia[2],
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

    // Bug №159: rotation.x/.z are QUATERNION components, not Euler angles — the
    // old getters returned garbage. Extract real roll/pitch via the standard
    // quaternion formulas instead.
    pub fn get_heel_angle(&self) -> f32 {
        let q = self.state.rotation;
        (2.0 * (q.w * q.x + q.y * q.z)).atan2(1.0 - 2.0 * (q.x * q.x + q.y * q.y))
    }

    pub fn get_trim_angle(&self) -> f32 {
        let q = self.state.rotation;
        (2.0 * (q.w * q.y - q.z * q.x)).clamp(-1.0, 1.0).asin()
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

impl Default for WaveSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl WaveSystem {
    pub fn new() -> Self {
        Self {
            waves: Vec::new(),
            time: 0.0,
        }
    }

    pub fn add_wave(&mut self, wave: Wave) {
        // Bug №161: wavelength=0 made k = inf and poisoned every height with
        // NaN. Reject degenerate waves at the door.
        if wave.wavelength.is_finite()
            && wave.wavelength > 0.0
            && wave.amplitude.is_finite()
            && wave.direction.is_finite()
        {
            self.waves.push(wave);
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.time += dt;
    }

    pub fn get_height(&self, position: Vec3f) -> f32 {
        let mut height = 0.0;
        for wave in &self.waves {
            if wave.wavelength.is_nan() || wave.wavelength <= 0.0 || !wave.amplitude.is_finite() {
                continue;
            }
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
            if wave.wavelength.is_nan() || wave.wavelength <= 0.0 || !wave.amplitude.is_finite() {
                continue;
            }
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

/// Roll about x and pitch about y, from a quaternion.
///
/// Bug №195: these were read straight off `rotation.x` and `rotation.z`, which
/// are quaternion components, not angles. A quaternion component is not a signed
/// angle, so the "spring" was a constant pull toward q.x = 0 rather than toward
/// upright, and its sign could push a capsized hull the wrong way. The same trap
/// is documented in `get_heel_angle` (Bug №159); these are the same conversions.
fn roll_and_pitch(rotation: Quatf) -> (f32, f32) {
    let roll = (2.0 * (rotation.w * rotation.x + rotation.y * rotation.z))
        .atan2(1.0 - 2.0 * (rotation.x * rotation.x + rotation.y * rotation.y));
    let pitch = (2.0 * (rotation.w * rotation.y - rotation.z * rotation.x))
        .clamp(-1.0, 1.0)
        .asin();
    (roll, pitch)
}

/// The upright-restoring part of the wave response, as a pure function.
///
/// Split out so it can be checked on its own: inside `calculate_wave_forces` it is
/// added to every other force in the frame, so asserting on the total would test
/// the whole force model instead of this term.
fn restoring_torque(
    rotation: Quatf,
    angular_velocity: Vec3f,
    stiffness: f32,
    damping: f32,
) -> Vec3f {
    let (roll, pitch) = roll_and_pitch(rotation);
    Vec3f::new(
        -roll * stiffness - angular_velocity.x * damping,
        -pitch * stiffness - angular_velocity.y * damping,
        -angular_velocity.z * damping,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const STIFFNESS: f32 = 50000.0;
    const DAMPING: f32 = 10000.0;

    fn restoring_for(rotation: Quatf) -> Vec3f {
        restoring_torque(rotation, Vec3f::ZERO, STIFFNESS, DAMPING)
    }

    fn rolled(angle: f32) -> Quatf {
        Quatf::from_axis_angle(Vec3f::new(1.0, 0.0, 0.0), angle)
    }

    fn trimmed(angle: f32) -> Quatf {
        Quatf::from_axis_angle(Vec3f::new(0.0, 1.0, 0.0), angle)
    }

    /// Bug №195: `rotation.x` and `rotation.z` are quaternion components, not
    /// angles. These assert behaviour rather than axis names, because the old
    /// labels were themselves wrong — the torque the old code called "pitch" was
    /// applied to x, which is the roll axis.
    #[test]
    fn an_upright_ship_gets_no_restoring_torque() {
        let t = restoring_for(Quatf::IDENTITY);
        assert!(
            t.x.abs() < 1e-3 && t.y.abs() < 1e-3,
            "a level ship was given a restoring torque: {:?}",
            t
        );
    }

    /// A hull rolled about x must be pushed back toward level. The sign is the
    /// whole point: a quaternion component's sign is not an angle's sign, so the
    /// old constant pull could drive a capsized hull further over.
    #[test]
    fn a_rolled_ship_is_pushed_back_toward_level() {
        let angle = 0.6_f32;
        let t = restoring_for(rolled(angle));
        assert!(
            t.x < 0.0,
            "a +{angle} rad roll produced {:?}, which does not restore it",
            t
        );
    }

    /// The mirror, so a sign error cannot pass unnoticed.
    #[test]
    fn rolling_the_other_way_torques_the_other_way() {
        let t = restoring_for(rolled(-0.6));
        assert!(
            t.x > 0.0,
            "a negative roll produced {:?}, which does not restore it",
            t
        );
    }

    /// Trim is pitch about y and must restore independently of roll.
    #[test]
    fn a_trimmed_ship_is_pushed_back_toward_level() {
        let t = restoring_for(trimmed(0.4));
        assert!(
            t.y < 0.0,
            "a +0.4 rad trim produced {:?}, which does not restore it",
            t
        );
    }

    /// The defect in one assertion, and the reason a ratio test cannot catch it.
    ///
    /// The old code multiplied a quaternion component by the stiffness, so it
    /// produced `-sin(angle / 2) * stiffness`. A correct spring produces
    /// `-angle * stiffness`. The *ratio* between two different angles is nearly
    /// the same for both (sin(0.1)/sin(0.5) is 0.208, and 0.2/1.0 is 0.200), so
    /// comparing one torque against another proves nothing. Comparing against the
    /// analytic value separates them by a factor of two at 1 rad.
    #[test]
    fn the_restoring_torque_tracks_the_angle_not_the_quaternion_component() {
        for angle in [0.2_f32, 0.6, 1.0, 2.0] {
            let t = restoring_for(rolled(angle)).x;
            let expected = -angle * STIFFNESS;
            let buggy = -(angle / 2.0).sin() * STIFFNESS;

            assert!(
                (t - expected).abs() < 1.0,
                "at {angle} rad expected about {expected}, got {t}"
            );
            assert!(
                (t - buggy).abs() > 1000.0,
                "at {angle} rad the torque {t} still matches the old sin(angle/2) form {buggy}"
            );
        }
    }

    /// Capsized: rolled past 90°. Quaternion component and angle disagree most
    /// here, and the hull must still be pushed back.
    #[test]
    fn a_capsized_hull_is_still_restored() {
        let t = restoring_for(rolled(2.0));
        assert!(
            t.x < 0.0,
            "a capsized hull got no restoring torque: {:?}",
            t
        );
    }

    /// The old code also put a restoring term on z, where a quaternion component
    /// says nothing about rotation about z. Now z carries damping only, so a pure
    /// roll about z produces no torque at all — and that is correct, because
    /// decomposing it lands on the roll/pitch axes.
    #[test]
    fn tilt_about_z_is_no_longer_mistaken_for_a_restoring_torque() {
        let t = restoring_for(Quatf::from_axis_angle(Vec3f::new(0.0, 0.0, 1.0), 0.5));
        assert!(
            t.z.abs() < 1e-3,
            "a quaternion z component is being treated as a z-axis angle: {:?}",
            t
        );
    }

    /// Damping must oppose angular velocity, and it is the only term that can
    /// appear with a zero restoring angle.
    #[test]
    fn damping_opposes_angular_velocity() {
        let t = restoring_torque(
            Quatf::IDENTITY,
            Vec3f::new(2.0, -3.0, 4.0),
            STIFFNESS,
            DAMPING,
        );
        assert!(t.x < 0.0, "damping did not oppose +x spin");
        assert!(t.y > 0.0, "damping did not oppose -y spin");
        assert!(t.z < 0.0, "damping did not oppose +z spin");
    }

    /// The reported angles must match the angles that were applied, which is
    /// what `get_heel_angle` / `get_trim_angle` already promised (Bug №159).
    #[test]
    fn the_recovered_angles_match_what_was_applied() {
        let q = rolled(0.6);
        let (roll, pitch) = roll_and_pitch(q);
        assert!((roll - 0.6).abs() < 1e-3, "roll recovered as {roll}");
        assert!(pitch.abs() < 1e-3, "a pure roll reported pitch {pitch}");

        let q = trimmed(0.4);
        let (roll, pitch) = roll_and_pitch(q);
        assert!((pitch - 0.4).abs() < 1e-3, "pitch recovered as {pitch}");
        assert!(roll.abs() < 1e-3, "a pure pitch reported roll {roll}");
    }

    /// Bug №725: a class file predating the field must load through the
    /// explicit default — the historical hardcoded `1000.0` — and not through
    /// f32's derived `0.0`, which is "no buoyancy at all".
    #[test]
    fn a_class_file_without_hull_area_loads_the_hardcoded_default() {
        let mut value = serde_json::to_value(ShipPhysicsConfig::default())
            .expect("the default config serializes");
        let obj = value.as_object_mut().expect("the config serializes as an object");
        let removed = obj.remove("hull_area");
        assert_eq!(
            removed,
            Some(serde_json::json!(1000.0)),
            "the default itself must stay the historical value"
        );
        // Vec3f does not have a Default-derived wire form at issue here; the
        // full object minus the one key is exactly an old class file's entry.
        let config: ShipPhysicsConfig =
            serde_json::from_value(value).expect("a class file without the field loads");
        assert_eq!(config.hull_area, 1000.0);
    }

    /// Bug №725: a declared area is taken, so a second class can differ from
    /// the first without touching physics.rs.
    #[test]
    fn a_declared_hull_area_is_taken_over_the_default() {
        let mut value = serde_json::to_value(ShipPhysicsConfig::default())
            .expect("the default config serializes");
        value
            .as_object_mut()
            .expect("an object")
            .insert("hull_area".to_string(), serde_json::json!(2300.0));
        let config: ShipPhysicsConfig =
            serde_json::from_value(value).expect("the override loads");
        assert_eq!(config.hull_area, 2300.0);
    }

    /// Bug №725: the hydrostatics integrate over the class's declared area,
    /// not over the old hardcoded `1000.0` — this test is the one that would
    /// go red if the literal moved back into `calculate_hydrostatics`.
    #[test]
    fn hydrostatics_uses_the_declared_hull_area() {
        let config =
            ShipPhysicsConfig { hull_area: 2000.0, ..ShipPhysicsConfig::default() };
        let mut physics = ShipPhysics::new(config);
        // The default state floats at y=0, the default waterline is 5 m:
        // 5 m of draft over a 2000 m² waterline area.
        physics.update(1.0 / 30.0, 0.0, 0.0);
        assert_eq!(
            physics.get_submerged_volume(),
            10000.0,
            "5 m draft over a 2000 m² waterline"
        );

        for broken in [0.0, -300.0, f32::NAN] {
            let config =
                ShipPhysicsConfig { hull_area: broken, ..ShipPhysicsConfig::default() };
            let mut physics = ShipPhysics::new(config);
            physics.update(1.0 / 30.0, 0.0, 0.0);
            assert_eq!(
                physics.get_submerged_volume(),
                0.0,
                "a {broken} declaration must read as no waterline area"
            );
        }
    }
}
