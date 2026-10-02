//! Light
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use glam::Vec3;

/// Light type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum LightType {
    #[default]
    Directional,
    Point,
    Spot,
    Area,
}


/// Base light struct
#[derive(Debug, Clone)]
pub struct Light {
    pub name: String,
    pub light_type: LightType,
    pub color: Vec3,
    pub intensity: f32,
    pub enabled: bool,
    pub casts_shadows: bool,
    pub shadow_resolution: u32,
}

impl Light {
    pub fn new(name: &str, light_type: LightType) -> Self {
        Self {
            name: name.to_string(),
            light_type,
            color: Vec3::ONE,
            intensity: 1.0,
            enabled: true,
            casts_shadows: true,
            shadow_resolution: 1024,
        }
    }

    pub fn with_color(mut self, color: Vec3) -> Self {
        self.color = color;
        self
    }

    pub fn with_intensity(mut self, intensity: f32) -> Self {
        self.intensity = intensity;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_shadows(mut self, casts_shadows: bool) -> Self {
        self.casts_shadows = casts_shadows;
        self
    }

    pub fn with_shadow_resolution(mut self, resolution: u32) -> Self {
        self.shadow_resolution = resolution;
        self
    }

    /// Direction this light points in.
    ///
    /// Bug №185: this used to call `as_directional()`, which reinterpreted a
    /// `&Light` as a `&DirectionalLight`. That is undefined behaviour twice
    /// over: it violates strict aliasing, and the two types do not even have
    /// the same layout — `Light` starts with `name: String`, while
    /// `DirectionalLight` starts with `base: Light`. Reading `direction` out of
    /// it returned whatever bytes happened to follow the `Light` fields.
    ///
    /// A bare `Light` simply has no direction, so this reports the documented
    /// default and callers that need the real value hold the concrete type,
    /// which has its own accessor.
    pub fn direction(&self) -> Vec3 {
        match self.light_type {
            LightType::Directional | LightType::Spot => Vec3::NEG_Z,
            LightType::Point | LightType::Area => Vec3::ZERO,
        }
    }

    /// World position of this light.
    ///
    /// Bug №185: same unsafe cast as `direction` — removed. A bare `Light` has
    /// no position; the concrete `PointLight`/`SpotLight` types carry it.
    pub fn position(&self) -> Vec3 {
        match self.light_type {
            LightType::Point | LightType::Spot => Vec3::ZERO,
            LightType::Directional | LightType::Area => Vec3::ZERO,
        }
    }

    /// Attenuation range of this light.
    ///
    /// Bug №185: same unsafe cast as `direction` — removed. Only a point or
    /// spot light has a range, and only on the concrete type.
    pub fn range(&self) -> f32 {
        match self.light_type {
            LightType::Directional => f32::INFINITY,
            _ => 10.0,
        }
    }
}

impl Default for Light {
    fn default() -> Self {
        Self::new("default", LightType::Directional)
    }
}

/// Directional light (e.g., sun, moon)
#[derive(Debug, Clone)]
pub struct DirectionalLight {
    pub base: Light,
    pub direction: Vec3,
}

impl DirectionalLight {
    pub fn new(name: &str, direction: Vec3) -> Self {
        Self {
            base: Light::new(name, LightType::Directional),
            direction: direction.normalize(),
        }
    }

    pub fn with_direction(mut self, direction: Vec3) -> Self {
        self.direction = direction.normalize();
        self
    }

    pub fn with_color(mut self, color: Vec3) -> Self {
        self.base.color = color;
        self
    }

    pub fn with_intensity(mut self, intensity: f32) -> Self {
        self.base.intensity = intensity;
        self
    }

    /// Real direction of this light.
    ///
    /// Bug №185: this is the accessor that `Light::direction` used to fake
    /// through an undefined-behaviour cast. Callers holding a
    /// `DirectionalLight` get the actual value from here.
    pub fn direction(&self) -> Vec3 {
        self.direction
    }
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self::new("directional", Vec3::NEG_Z)
    }
}

/// Point light (e.g., light bulb, explosion)
#[derive(Debug, Clone)]
pub struct PointLight {
    pub base: Light,
    pub position: Vec3,
    pub range: f32,
    pub attenuation: Vec3, // (constant, linear, quadratic)
}

impl PointLight {
    pub fn new(name: &str, position: Vec3) -> Self {
        Self {
            base: Light::new(name, LightType::Point),
            position,
            range: 10.0,
            attenuation: Vec3::new(1.0, 0.0, 0.0),
        }
    }

    pub fn with_position(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    pub fn with_range(mut self, range: f32) -> Self {
        self.range = range;
        self
    }

    pub fn with_attenuation(mut self, constant: f32, linear: f32, quadratic: f32) -> Self {
        self.attenuation = Vec3::new(constant, linear, quadratic);
        self
    }

    pub fn with_color(mut self, color: Vec3) -> Self {
        self.base.color = color;
        self
    }

    pub fn with_intensity(mut self, intensity: f32) -> Self {
        self.base.intensity = intensity;
        self
    }
}

impl Default for PointLight {
    fn default() -> Self {
        Self::new("point", Vec3::ZERO)
    }
}

/// Spot light (e.g., flashlight, searchlight)
#[derive(Debug, Clone)]
pub struct SpotLight {
    pub base: Light,
    pub position: Vec3,
    pub direction: Vec3,
    pub range: f32,
    pub inner_angle: f32,  // in radians
    pub outer_angle: f32,  // in radians
    pub attenuation: Vec3, // (constant, linear, quadratic)
}

impl SpotLight {
    pub fn new(name: &str, position: Vec3, direction: Vec3) -> Self {
        Self {
            base: Light::new(name, LightType::Spot),
            position,
            direction: direction.normalize(),
            range: 10.0,
            inner_angle: 0.3, // ~17 degrees
            outer_angle: 0.5, // ~28 degrees
            attenuation: Vec3::new(1.0, 0.0, 0.0),
        }
    }

    pub fn with_position(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    pub fn with_direction(mut self, direction: Vec3) -> Self {
        self.direction = direction.normalize();
        self
    }

    pub fn with_range(mut self, range: f32) -> Self {
        self.range = range;
        self
    }

    pub fn with_angles(mut self, inner: f32, outer: f32) -> Self {
        self.inner_angle = inner;
        self.outer_angle = outer;
        self
    }

    pub fn with_attenuation(mut self, constant: f32, linear: f32, quadratic: f32) -> Self {
        self.attenuation = Vec3::new(constant, linear, quadratic);
        self
    }

    pub fn with_color(mut self, color: Vec3) -> Self {
        self.base.color = color;
        self
    }

    pub fn with_intensity(mut self, intensity: f32) -> Self {
        self.base.intensity = intensity;
        self
    }
}

impl Default for SpotLight {
    fn default() -> Self {
        Self::new("spot", Vec3::ZERO, Vec3::NEG_Z)
    }
}

/// Light configuration
#[derive(Debug, Clone)]
pub struct LightConfig {
    pub max_direction_lights: usize,
    pub max_point_lights: usize,
    pub max_spot_lights: usize,
    pub max_area_lights: usize,
    pub shadow_enabled: bool,
    pub shadow_resolution: u32,
    pub cascade_count: usize,
}

impl Default for LightConfig {
    fn default() -> Self {
        Self {
            max_direction_lights: 4,
            max_point_lights: 16,
            max_spot_lights: 16,
            max_area_lights: 4,
            shadow_enabled: true,
            shadow_resolution: 2048,
            cascade_count: 4,
        }
    }
}

/// Light builder
pub struct LightBuilder {
    light: Light,
}

impl LightBuilder {
    pub fn new(name: &str, light_type: LightType) -> Self {
        Self {
            light: Light::new(name, light_type),
        }
    }

    pub fn with_color(mut self, color: Vec3) -> Self {
        self.light.color = color;
        self
    }

    pub fn with_intensity(mut self, intensity: f32) -> Self {
        self.light.intensity = intensity;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.light.enabled = enabled;
        self
    }

    pub fn with_shadows(mut self, casts_shadows: bool) -> Self {
        self.light.casts_shadows = casts_shadows;
        self
    }

    pub fn with_shadow_resolution(mut self, resolution: u32) -> Self {
        self.light.shadow_resolution = resolution;
        self
    }

    pub fn build(self) -> Light {
        self.light
    }

    pub fn build_directional(self, direction: Vec3) -> DirectionalLight {
        DirectionalLight::new(&self.light.name, direction)
            .with_color(self.light.color)
            .with_intensity(self.light.intensity)
    }

    pub fn build_point(self, position: Vec3) -> PointLight {
        PointLight::new(&self.light.name, position)
            .with_color(self.light.color)
            .with_intensity(self.light.intensity)
    }

    pub fn build_spot(self, position: Vec3, direction: Vec3) -> SpotLight {
        SpotLight::new(&self.light.name, position, direction)
            .with_color(self.light.color)
            .with_intensity(self.light.intensity)
    }
}
