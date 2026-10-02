//! Light Probe
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Light probes store precomputed lighting information for static objects.
//! They are used for Image-Based Lighting (IBL) and global illumination.

use glam::Vec3;
use std::sync::Arc;

/// Light probe type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum LightProbeType {
    /// Spherical harmonic probe (for diffuse lighting)
    #[default]
    SphericalHarmonics,
    /// Cube map probe (for reflections)
    CubeMap,
    /// Combined probe (both SH and cube map)
    Combined,
}


/// Light probe
pub struct LightProbe {
    pub name: String,
    pub probe_type: LightProbeType,
    pub position: Vec3,
    pub range: f32,
    pub enabled: bool,
    /// Spherical harmonic coefficients (9 coefficients for 3 bands)
    pub sh_coefficients: Vec<Vec3>,
    /// Cube map for reflections
    pub cube_map: Option<Arc<crate::rhi::resource::Texture>>,
    /// Probe influence volume
    pub volume: LightProbeVolume,
}

impl LightProbe {
    pub fn new(name: &str, probe_type: LightProbeType, position: Vec3) -> Self {
        Self {
            name: name.to_string(),
            probe_type,
            position,
            range: 10.0,
            enabled: true,
            sh_coefficients: vec![Vec3::ZERO; 9],
            cube_map: None,
            volume: LightProbeVolume::Sphere { radius: 5.0 },
        }
    }

    pub fn with_range(mut self, range: f32) -> Self {
        self.range = range;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_volume(mut self, volume: LightProbeVolume) -> Self {
        self.volume = volume;
        self
    }

    /// Bake lighting into the probe
    pub fn bake(&mut self) {
        // In actual implementation, this would:
        // 1. Capture environment lighting at this position
        // 2. Project it onto spherical harmonics
        // 3. Or render a cube map

        // For now, this is a placeholder
    }

    /// Get lighting at a position
    pub fn get_light(&self, _position: Vec3, _normal: Vec3) -> Vec3 {
        // Calculate lighting based on position and normal
        // This would use the SH coefficients or sample the cube map

        // For now, return white
        Vec3::ONE
    }

    /// Check if position is inside the probe's volume
    pub fn contains(&self, position: Vec3) -> bool {
        match self.volume {
            LightProbeVolume::Sphere { radius } => (position - self.position).length() <= radius,
            LightProbeVolume::Box { min, max } => {
                position.x >= min.x
                    && position.x <= max.x
                    && position.y >= min.y
                    && position.y <= max.y
                    && position.z >= min.z
                    && position.z <= max.z
            }
        }
    }
}

impl Default for LightProbe {
    fn default() -> Self {
        Self::new("default", LightProbeType::SphericalHarmonics, Vec3::ZERO)
    }
}

/// Light probe volume type
#[derive(Debug, Clone, Copy)]
pub enum LightProbeVolume {
    /// Spherical volume
    Sphere { radius: f32 },
    /// Box volume
    Box { min: Vec3, max: Vec3 },
}

impl Default for LightProbeVolume {
    fn default() -> Self {
        Self::Sphere { radius: 5.0 }
    }
}

/// Light probe configuration
#[derive(Debug, Clone)]
pub struct LightProbeConfig {
    pub max_probes: usize,
    pub probe_type: LightProbeType,
    pub bake_resolution: u32,
    pub update_frequency: LightProbeUpdateFrequency,
}

impl Default for LightProbeConfig {
    fn default() -> Self {
        Self {
            max_probes: 64,
            probe_type: LightProbeType::SphericalHarmonics,
            bake_resolution: 128,
            update_frequency: LightProbeUpdateFrequency::OnDemand,
        }
    }
}

/// Light probe update frequency
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum LightProbeUpdateFrequency {
    /// Update every frame
    EveryFrame,
    /// Update when static objects change
    OnStaticChange,
    /// Update on demand
    #[default]
    OnDemand,
    /// Never update (baked at level load)
    Never,
}


/// Bug №183: an upper bound on the probe count. The grid allocates
/// `grid_size.x * y * z` probes, so a large or nonsensical grid is an
/// allocation bomb rather than an error.
pub const MAX_PROBE_COUNT: usize = 1 << 20;

/// Light probe grid (for large environments)
pub struct LightProbeGrid {    pub config: LightProbeConfig,
    pub probes: Vec<LightProbe>,
    pub grid_size: Vec3,
    pub cell_size: Vec3,
}

impl LightProbeGrid {
    /// Bug №183: `grid_size` with a zero or negative component made the
    /// constructor's `0..grid_size.x as usize` loop either empty or enormous,
    /// and `cell_size` with a zero component made `get_probe` divide by zero
    /// on every call. Both are now rejected at construction, where the mistake
    /// is still attributable, instead of surfacing as a panic or a silent
    /// empty grid much later.
    pub fn try_new(
        config: LightProbeConfig,
        grid_size: Vec3,
        cell_size: Vec3,
    ) -> Option<Self> {
        if !(grid_size.x.is_finite()
            && grid_size.y.is_finite()
            && grid_size.z.is_finite()
            && grid_size.x >= 1.0
            && grid_size.y >= 1.0
            && grid_size.z >= 1.0)
        {
            return None;
        }
        if !(cell_size.x.is_finite()
            && cell_size.y.is_finite()
            && cell_size.z.is_finite()
            && cell_size.x > 0.0
            && cell_size.y > 0.0
            && cell_size.z > 0.0)
        {
            return None;
        }
        // The probe count is a product of three usizes; an absurd grid would
        // try to allocate a huge vector. Bound it rather than OOM.
        let total = grid_size.x as usize * grid_size.y as usize * grid_size.z as usize;
        if total > MAX_PROBE_COUNT {
            return None;
        }
        Some(Self::new_unchecked(config, grid_size, cell_size))
    }

    fn new_unchecked(config: LightProbeConfig, grid_size: Vec3, cell_size: Vec3) -> Self {
        let mut probes = Vec::new();

        // Create probes for each cell
        for x in 0..grid_size.x as usize {
            for y in 0..grid_size.y as usize {
                for z in 0..grid_size.z as usize {
                    let position = Vec3::new(
                        x as f32 * cell_size.x,
                        y as f32 * cell_size.y,
                        z as f32 * cell_size.z,
                    );
                    let name = format!("probe_{}_{}_{}", x, y, z);
                    probes.push(LightProbe::new(&name, config.probe_type, position));
                }
            }
        }

        Self {
            config,
            probes,
            grid_size,
            cell_size,
        }
    }

    pub fn new(config: LightProbeConfig, grid_size: Vec3, cell_size: Vec3) -> Self {
        Self::try_new(config, grid_size, cell_size)
            .expect("light probe grid needs a positive integer grid_size, a positive cell_size and a bounded probe count")
    }

    /// Get the probe at a position.
    ///
    /// Bug №183: the cell index used to be `(position.x / cell_size.x).floor()
    /// as usize`. A negative coordinate makes `floor()` negative, and `as usize`
    /// on a negative float wraps to an enormous index (saturating in Rust, so
    /// `usize::MAX`), so the lookup silently returned `None` for the entire
    /// negative half-space. The `x/y/z` values were also never clamped to
    /// `grid_size`, so a position past the last cell produced an index that
    /// happened to alias a different cell or, after the wrap, a wildly out of
    /// range one.
    ///
    /// The cell is now derived with explicit bounds, and an out-of-grid
    /// position returns `None` instead of aliasing.
    pub fn get_probe(&self, position: Vec3) -> Option<&LightProbe> {
        // A non-finite position has no cell.
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            return None;
        }
        if !(self.cell_size.x > 0.0 && self.cell_size.y > 0.0 && self.cell_size.z > 0.0) {
            return None;
        }

        let coord = |value: f32, cell: f32, size: f32| -> Option<usize> {
            // Negative positions are outside the grid: the grid starts at the
            // origin and only spans +x/+y/+z.
            if value < 0.0 {
                return None;
            }
            let cell_index = (value / cell).floor();
            if cell_index < 0.0 {
                return None;
            }
            let index = cell_index as usize;
            if index >= size as usize {
                return None;
            }
            Some(index)
        };

        let x = coord(position.x, self.cell_size.x, self.grid_size.x)?;
        let y = coord(position.y, self.cell_size.y, self.grid_size.y)?;
        let z = coord(position.z, self.cell_size.z, self.grid_size.z)?;

        let index = x + y * self.grid_size.x as usize + z * (self.grid_size.x * self.grid_size.y) as usize;
        self.probes.get(index)
    }

    /// Bake all probes
    pub fn bake_all(&mut self) {
        for probe in &mut self.probes {
            probe.bake();
        }
    }
}
